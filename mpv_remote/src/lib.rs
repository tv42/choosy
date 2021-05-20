use async_std::io;
use async_std::os::unix::net::UnixStream;
#[allow(unused_imports)]
use async_std::prelude::*;
use async_std::process;
use async_std::sync::Mutex;
use async_std::task;
use futures_channel::oneshot;
#[allow(unused_imports)]
use kv_log_macro::{debug, error, info, log, trace, warn};
// WAITING incorrect error from rust-analyzer https://github.com/rust-analyzer/rust-analyzer/issues/6038
use std::os::unix::io::FromRawFd;
use std::os::unix::io::IntoRawFd;
use std::{ffi::OsStr, sync::Arc};
use thiserror::Error;

mod messages;
pub use self::messages::*;
mod pending;
use self::pending::Pending;

// MPV runs the mpv video player in a subprocess and observes the playback progress.
pub struct MPV {
    child: process::Child,
    ipc: Arc<IPCState>,
    ipc_task: task::JoinHandle<Result<(), io::Error>>,
}

// separate state needed by read_from_mpv so that MPV doesn't end up in cyclic reference hell where the value for ipc_task depends on MPV. it was either this or Option<task::JoinHandle<...>> and this was less ugly.
struct IPCState {
    read_socket: UnixStream,
    write_socket: Arc<Mutex<UnixStream>>,
    pending: Arc<Mutex<Pending<IPCResult>>>,
    events_sender: async_broadcast::Sender<MPVEvent>,
    // Receiver.recv is a &mut self method, but IPCState::read_from_mpv needs to run concurrently with MPV::events, so lock around it. Locking around clone is unnecessary but too hard to avoid.
    events_receiver: Arc<Mutex<async_broadcast::Receiver<MPVEvent>>>,
}

#[derive(Error, Debug)]
pub enum StartError {
    #[error("socket create error: {0}")]
    SocketCreate(std::io::Error),
    #[error("task spawning error: {0}")]
    SpawningTask(std::io::Error),
    #[error("starting MPV: {0}")]
    StartingMPV(std::io::Error),
}

impl MPV {
    pub fn new(path: &OsStr) -> Result<MPV, StartError> {
        let (socket, child_socket) = match UnixStream::pair() {
            Ok(pair) => pair,
            Err(error) => return Err(StartError::SocketCreate(error)),
        };

        // RUST-WART I find it hard to believe that Rust does not offer any mechanism to pass a UnixStream to a child process as an fd, without resorting to unsafe.
        let child_file = unsafe {
            let child_fd = child_socket.into_raw_fd();
            std::fs::File::from_raw_fd(child_fd)
        };
        let child = match process::Command::new("mpv")
            // RUST-WART I seem to be unable to pass FDs other than 0/1/2, without managing the fork+exec myself?
            .arg("--input-ipc-client=fd://0")
            .arg("--no-input-terminal")
            .stdin(child_file)
            // TODO full screen
            .arg("--")
            // TODO make non-absolute paths to start with "./" so mpv won't parse them as URLs
            .arg(path)
            .spawn()
        {
            Ok(proc) => proc,
            Err(error) => return Err(StartError::StartingMPV(error)),
        };

        let pending = Arc::new(Mutex::new(Pending::new() as Pending<IPCResult>));
        let (events_sender, events_receiver) = async_broadcast::broadcast(100);

        let ipc = Arc::new(IPCState {
            read_socket: socket.clone(),
            write_socket: Arc::new(Mutex::new(socket)),
            pending,
            events_sender,
            events_receiver: Arc::new(Mutex::new(events_receiver)),
        });
        let ipc_task = {
            let ipc = ipc.clone();
            task::Builder::new()
                .name("mpv-ipc".to_string())
                .spawn(async move { ipc.read_from_mpv().await })
                .map_err(|e| StartError::SpawningTask(e))?
        };

        let mpv = MPV {
            child,
            ipc,
            ipc_task,
        };
        Ok(mpv)
    }

    pub async fn events(&self) -> async_broadcast::Receiver<MPVEvent> {
        let guard = self.ipc.events_receiver.lock().await;
        let r = guard.clone();
        r
    }
}

#[derive(Error, Debug)]
pub enum IPCError {
    #[error("error from MPV: {0}")]
    FromMPV(String),
    #[error("JSON serialization: {0}")]
    JSONSerialize(serde_json::Error),
    #[error("network: {0}")]
    Network(io::Error),
    #[error("disconnected")]
    Disconnected,
}

// TODO change the Value to avoid unmarshaling to wrong thing, type safety, somehow per-command types
pub type IPCResult = Result<serde_json::Value, IPCError>;

impl IPCState {
    async fn read_from_mpv(&self) -> io::Result<()> {
        let reader = io::BufReader::new(&self.read_socket);
        let mut lines = reader.lines();
        while let Some(line) = lines.next().await {
            let line = line?;
            match serde_json::from_str::<MPVEnvelope>(&line) {
                Ok(env) => match env {
                    MPVEnvelope::Event(event) => {
                        debug!("mpv event", {
                            event: log::kv::Value::capture_debug(&event)
                        });
                        self.events_sender
                            .broadcast(event)
                            .await
                            .expect("internal error: broadcast channel cannot be closed yet");
                        // we're forced to keep one receiver just to be able to clone it on demand, but that means items are left buffered when idle. consume from that sender, just to clear room. https://github.com/smol-rs/async-broadcast/issues/2
                        let mut guard = self.events_receiver.lock().await;
                        while let Ok(_) = guard.try_recv() {
                            // nothing
                        }
                    }
                    MPVEnvelope::Response(response) => {
                        debug!("mpv response", {
                            response: log::kv::Value::capture_debug(&response)
                        });
                        let waiting = {
                            let mut guard = self.pending.lock().await;
                            guard.get(response.request_id)
                        };
                        match waiting {
                            None => {
                                error!("unrecognized id from MPV", {
                                    request_id: response.request_id,
                                    response: log::kv::Value::capture_debug(&response)
                                });
                            }
                            Some(sender) => {
                                let result = response.result.map_err(|msg| IPCError::FromMPV(msg));
                                debug!("sending result", {
                                    result: log::kv::Value::capture_debug(&result)
                                });
                                match sender.send(result) {
                                    Ok(()) => {
                                        debug!("successfully sent!");
                                    }
                                    Err(payload) => {
                                        debug!("MPV command unexpectedly canceled early", {
                                            request_id: response.request_id,
                                            result: log::kv::Value::capture_debug(&payload),
                                        });
                                    }
                                }
                            }
                        }
                    }
                },
                Err(error) => {
                    debug!("unrecognized mpv message", {
                        error: log::kv::Value::capture_error(&error),
                        json: line,
                    });
                }
            }
        }

        // report connection loss to all pending
        {
            let mut guard = self.pending.lock().await;
            guard.close();
        }
        self.events_sender.close();

        Ok(())
    }
}

impl MPV {
    pub async fn command(&self, command: serde_json::Value) -> IPCResult {
        let (id_option, receiver) = {
            let mut guard = self.ipc.pending.lock().await;
            guard.insert()
        };
        if let Some(id) = id_option {
            // send
            let wire_command = Command {
                request_id: id,
                command: command,
                async_: true,
            };
            let mut wire_data =
                serde_json::to_vec(&wire_command).map_err(|e| IPCError::JSONSerialize(e))?;
            wire_data.push('\n' as u8);
            debug!("sending command", {
                // RUST-WART i can only get a numeric display out of this!
                data: log::kv::Value::capture_debug(&wire_data),
            });
            let mut guard = self.ipc.write_socket.lock().await;
            guard
                .write_all(&wire_data)
                .await
                .map_err(|e| IPCError::Network(e))?
        }
        match receiver.await {
            Err(error) => match error {
                oneshot::Canceled => Err(IPCError::Disconnected),
            },
            Ok(result) => result,
        }
    }
}

#[derive(Error, Debug)]
pub enum CloseError {
    #[error("task spawning error: {0}")]
    TaskError(std::io::Error),
    #[error("process error: {0}")]
    ProcessError(std::io::Error),
    #[error("MPV exited with error: {0}")]
    MPVExitStatus(process::ExitStatus),
}

impl MPV {
    pub async fn close(mut self) -> Result<(), CloseError> {
        // TODO try sending an ipc quit first

        // RUST-WART process::Child can't do SIGTERM, idiots. https://github.com/rust-lang/rust/issues/41822
        //
        // let _ignore_kill_error = self.child.kill();
        unsafe {
            let _ignore_kill_error = libc::kill(self.child.id() as i32, libc::SIGTERM);
        }

        self.ipc_task.await.map_err(|e| CloseError::TaskError(e))?;
        let exit_status = self
            .child
            .status()
            .await
            .map_err(|e| CloseError::ProcessError(e))?;
        if !exit_status.success() {
            return Err(CloseError::MPVExitStatus(exit_status));
        }
        Ok(())
    }
}
