use derive_builder::Builder;
use futures_channel::oneshot;
#[allow(unused_imports)]
use tracing::{debug, error, info, log, trace, warn};
// WAITING incorrect error from rust-analyzer https://github.com/rust-analyzer/rust-analyzer/issues/6038
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::{ffi::OsStr, sync::Arc};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

mod messages;
pub use self::messages::*;
mod pending;
use self::pending::Pending;

#[derive(Builder)]
pub struct Config {
    // External bug: MPV never starts fullscreen under ChromeOS Linux container, even if pressing "f" later works.
    #[builder(default = "true")]
    fullscreen: bool,
}

// MPV runs the mpv video player in a subprocess and observes the playback progress.
pub struct MPV {
    child: tokio::process::Child,
    ipc: Arc<IPCState>,
    ipc_task: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

// separate state needed by read_from_mpv so that MPV doesn't end up in cyclic reference hell where the value for ipc_task depends on MPV. it was either this or Option<task::JoinHandle<...>> and this was less ugly.
struct IPCState {
    write_socket: Arc<tokio::sync::Mutex<tokio::net::unix::OwnedWriteHalf>>,
    pending: Arc<tokio::sync::Mutex<Pending<IPCResult>>>,
    events_sender: std::sync::Weak<tokio::sync::broadcast::Sender<MPVEvent>>,
}

#[derive(Error, Debug)]
pub enum StartError {
    #[error("socket create error: {0}")]
    SocketCreate(std::io::Error),
    #[error("starting MPV: {0}")]
    StartingMPV(std::io::Error),
}

impl Config {
    pub fn play(&self, path: &OsStr) -> Result<MPV, StartError> {
        let (socket, child_socket) = match tokio::net::UnixStream::pair() {
            Ok(pair) => pair,
            Err(error) => return Err(StartError::SocketCreate(error)),
        };

        // RUST-WART I find it hard to believe that Rust does not offer any mechanism to pass a UnixStream to a child process as an fd, without resorting to unsafe.
        let child_file = unsafe {
            let child_fd = child_socket.as_raw_fd();
            std::fs::File::from_raw_fd(child_fd)
        };
        let mut cmd = tokio::process::Command::new("mpv");
        cmd
            // RUST-WART I seem to be unable to pass FDs other than 0/1/2, without managing the fork+exec myself?
            .arg("--input-ipc-client=fd://0")
            .arg("--no-input-terminal")
            .stdin(child_file);
        if self.fullscreen {
            cmd.arg("--fullscreen");
        }
        // TODO make non-absolute paths to start with "./" so mpv won't parse them as URLs
        cmd.arg("--").arg(path);
        let child = match cmd.spawn() {
            Ok(proc) => proc,
            Err(error) => return Err(StartError::StartingMPV(error)),
        };
        // Ensure the FD lives past the fork.
        drop(child_socket);

        let pending = Arc::new(tokio::sync::Mutex::new(Pending::new() as Pending<IPCResult>));
        let (events_sender, events_receiver) = tokio::sync::broadcast::channel(100);
        let events_sender = Arc::new(events_sender);

        let (socket_reader, socket_writer) = socket.into_split();
        let ipc = Arc::new(IPCState {
            write_socket: Arc::new(tokio::sync::Mutex::new(socket_writer)),
            pending,
            events_sender: Arc::downgrade(&events_sender),
        });
        let ipc_task = {
            let ipc = ipc.clone();
            tokio::task::spawn(async move {
                ipc.read_from_mpv(socket_reader, events_receiver, events_sender)
                    .await
            })
        };

        let mpv = MPV {
            child,
            ipc,
            ipc_task,
        };
        Ok(mpv)
    }
}

impl MPV {
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }

    pub async fn events(&self) -> tokio::sync::broadcast::Receiver<MPVEvent> {
        match self.ipc.events_sender.upgrade() {
            Some(sender) => sender.subscribe(),
            None => {
                // trying to subscribe after mpv has already exited.
                // make a dummy channel and close it.
                let (sender, _receiver) = tokio::sync::broadcast::channel(1);
                sender.subscribe()
            }
        }
    }
}

#[derive(Error, Debug)]
pub enum IPCError {
    #[error("error from MPV: {0}")]
    FromMPV(String),
    #[error("JSON serialization: {0}")]
    JSONSerialize(serde_json::Error),
    #[error("network: {0}")]
    Network(std::io::Error),
    #[error("disconnected")]
    Disconnected,
}

// TODO change the Value to avoid unmarshaling to wrong thing, type safety, somehow per-command types
pub type IPCResult = Result<serde_json::Value, IPCError>;

impl IPCState {
    async fn read_from_mpv(
        &self,
        read_socket: tokio::net::unix::OwnedReadHalf,
        mut events_receiver: tokio::sync::broadcast::Receiver<MPVEvent>,
        events_sender: Arc<tokio::sync::broadcast::Sender<MPVEvent>>,
    ) -> tokio::io::Result<()> {
        let reader = tokio::io::BufReader::new(read_socket);
        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            match serde_json::from_str::<MPVEnvelope>(&line) {
                Ok(env) => match env {
                    MPVEnvelope::Event(event) => {
                        debug!(message = "mpv event", ?event);
                        events_sender
                            .send(event)
                            .expect("internal error: broadcast channel cannot be closed yet");
                        // we're forced to keep one receiver just to be able to clone it on demand, but that means items are left buffered when idle.
                        // consume from that sender, just to clear room.
                        // https://github.com/smol-rs/async-broadcast/issues/2 & https://docs.rs/tokio/1.17.0/tokio/sync/broadcast/fn.channel.html
                        while events_receiver.try_recv().is_ok() {
                            // nothing
                        }
                    }
                    MPVEnvelope::Response(response) => {
                        debug!(message = "mpv response", ?response);
                        let waiting = {
                            let mut guard = self.pending.lock().await;
                            guard.get(response.request_id)
                        };
                        match waiting {
                            None => {
                                error!(
                                    message = "unrecognized id from MPV",
                                    request_id = response.request_id,
                                    ?response,
                                );
                            }
                            Some(sender) => {
                                let result = response.result.map_err(IPCError::FromMPV);
                                debug!(message = "sending result", ?result);
                                match sender.send(result) {
                                    Ok(()) => {
                                        debug!(message = "successfully sent!");
                                    }
                                    Err(payload) => {
                                        debug!(
                                            message = "MPV command unexpectedly canceled early",
                                            request_id = response.request_id,
                                            result = ?payload,
                                        );
                                    }
                                }
                            }
                        }
                    }
                },
                Err(error) => {
                    debug!(message = "unrecognized mpv message", ?error, json = %line);
                }
            }
        }

        // report connection loss to all pending
        {
            let mut guard = self.pending.lock().await;
            guard.close();
        }
        drop(events_sender);

        Ok(())
    }
}

impl MPV {
    pub async fn command(&self, command: serde_json::Value) -> IPCResult {
        let (id_option, receiver) = {
            let mut guard = self.ipc.pending.lock().await;
            guard.insert()
        };
        if let Some(request_id) = id_option {
            // send
            let wire_command = Command {
                request_id,
                command,
                async_: true,
            };
            debug!(message="sending command",
                command=?wire_command,
            );
            let mut wire_data =
                serde_json::to_vec(&wire_command).map_err(IPCError::JSONSerialize)?;
            wire_data.push(b'\n');
            let mut guard = self.ipc.write_socket.lock().await;
            guard
                .write_all(&wire_data)
                .await
                .map_err(IPCError::Network)?
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
    TaskError(tokio::task::JoinError),
    #[error("IPC worker error: {0}")]
    IpcError(std::io::Error),
    #[error("process error: {0}")]
    ProcessError(std::io::Error),
    #[error("MPV exited with error: {0}")]
    MPVExitStatus(std::process::ExitStatus),
}

impl MPV {
    pub async fn close(mut self) -> Result<(), CloseError> {
        // TODO try sending an ipc quit first

        // RUST-WART process::Child can't do SIGTERM, idiots. https://github.com/rust-lang/rust/issues/41822
        //
        // let _ignore_kill_error = self.child.kill();
        unsafe {
            if let Some(id) = self.child.id() {
                let _ignore_kill_error = libc::kill(id as i32, libc::SIGTERM);
            }
        }

        self.ipc_task
            .await
            .map_err(CloseError::TaskError)?
            .map_err(CloseError::IpcError)?;
        let exit_status = self.child.wait().await.map_err(CloseError::ProcessError)?;
        if !exit_status.success() {
            return Err(CloseError::MPVExitStatus(exit_status));
        }
        Ok(())
    }
}
