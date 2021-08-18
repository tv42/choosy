#[allow(unused_imports)]
use async_std::prelude::*;
use async_std::stream::StreamExt;
use async_std::sync::Arc;
use async_std::sync::Mutex;
use async_std::task;
use choosy_embed;
use choosy_protocol as proto;
use http_types::mime;
#[allow(unused_imports)]
use kv_log_macro::{debug, error, info, log, trace, warn};
use listenfd::ListenFd;
use mpv_remote::MPV;
use scopeguard;
use serde_json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use structopt::StructOpt;
#[allow(unused_imports)]
use tide::prelude::*;
use tide::Body;
use tide::Request;
use tide::Response;
use tide::StatusCode;

mod ws {
    // clean up import names
    pub use tide_websockets::{Message, WebSocket as Handle, WebSocketConnection as Conn};
}

mod config;
mod database;
mod file_scanner;
use config::Config;

#[derive(Clone, PartialEq)]
struct File {}

struct State {
    config: Config,
    media: database::MediaDb,
    playing: Mutex<Option<MPV>>,
}

async fn wasm_bg(_req: Request<Arc<State>>) -> tide::Result {
    let mut resp = Response::new(StatusCode::Ok);
    resp.set_content_type(mime::WASM);
    resp.set_body(Body::from_reader(choosy_embed::wasm().clone(), None));
    Ok(resp)
}

async fn wasm_js(_req: Request<Arc<State>>) -> tide::Result {
    let mut resp = Response::new(StatusCode::Ok);
    resp.set_content_type(mime::JAVASCRIPT);
    resp.set_body(Body::from_reader(choosy_embed::wasm_js(), None));
    Ok(resp)
}

async fn index_html(_req: Request<Arc<State>>) -> tide::Result {
    let mut resp = Response::new(StatusCode::Ok);
    resp.set_content_type(mime::HTML);
    let bytes = include_bytes!("../../frontend/static/index.html");
    resp.set_body(Body::from_bytes(bytes.to_vec()));
    Ok(resp)
}

async fn ws_list_events(state: Arc<State>, conn: ws::Conn) -> tide::Result<()> {
    // We need to start watching for changes before scanning the existing data, or we might miss an update.
    // We need to send the existing data first, before processing watch events, to avoid races.
    // However, subscriber must be consumed expediently or it will block database writers.
    // Hence, we buffer the subscriber events while the initial scan is working.
    // In this use case, we can coalesce the events by key.
    // That might consume a lot of memory, but in this use case we're roughly bounded by the number of files changes noticed in one scan, which we assume we can already hold in memory elsewhere.
    let subscriber = state.media.watch_prefix("");

    // Do sled tree watching in a separate thread, with a channel between them, to notice slow consumers and kick them out, instead of blocking the database.
    let (sender, receiver) = std::sync::mpsc::sync_channel(1000);
    let events_thread = std::thread::spawn(move || -> Result<(), anyhow::Error> {
        // TODO do i care about exact error? at least db error!

        // TODO use iterator chaining here

        let mut buffered_events = BTreeMap::new();

        // First, scan the database for existing entries.
        {
            for result in state.media.scan_prefix("") {
                let (key, item) = result?;
                if item.exists {
                    // Since this is the initial state dump, we don't need to send Del changes.

                    // TODO Get rid of the batching, as we now stream events.
                    let batch = vec![proto::FileChange::Add {
                        name: String::from_utf8_lossy(&key.as_ref()).into_owned(),
                    }];
                    sender.try_send(batch)?;
                }

                // Pump the sled subscriber events to avoid blocking writers.
                while let Ok(event) = subscriber.next_timeout(Duration::from_nanos(0)) {
                    let key = match &event {
                        sleigh::Event::Insert { key, .. } => key.clone(),
                        sleigh::Event::Remove { key } => key.clone(),
                    };
                    buffered_events.insert(key, event);
                }
            }
        }

        // Then send buffered events.
        {
            for (key, event) in buffered_events.into_iter() {
                let name = String::from_utf8_lossy(&key.as_ref()).into_owned();
                let change = match event {
                    sleigh::Event::Insert { key: _, value } => {
                        if value.exists {
                            proto::FileChange::Add { name }
                        } else {
                            proto::FileChange::Del { name }
                        }
                    }
                    sleigh::Event::Remove { key: _ } => proto::FileChange::Del { name },
                };
                let batch = vec![change];
                sender.try_send(batch)?;
            }
        }

        // Finally, stream events as they happen.
        //
        // If the client goes away, this may stick around until the next database mutation, which might not come in a long while.
        for result in subscriber {
            let event = result?;
            let change = match event {
                sleigh::Event::Insert { key, value } => {
                    let name = String::from_utf8_lossy(&key.as_ref()).into_owned();
                    if value.exists {
                        proto::FileChange::Add { name }
                    } else {
                        proto::FileChange::Del { name }
                    }
                }
                sleigh::Event::Remove { key } => proto::FileChange::Del {
                    name: String::from_utf8_lossy(&key.as_ref()).into_owned(),
                },
            };
            let batch = vec![change];
            sender.try_send(batch)?;
        }
        Ok(())
    });

    let ws_thread = std::thread::spawn(move || -> tide::Result<()> {
        for batch in receiver {
            async_std::task::block_on(async {
                conn.send_json(&proto::WSEvent::FileChange(batch)).await
            })?;
        }
        Ok(())
    });

    events_thread
        .join()
        .expect("internal error: thread join error")?;
    ws_thread
        .join()
        .expect("internal error: thread join error")?;
    Ok(())
}

async fn websocket(req: tide::Request<Arc<State>>, mut conn: ws::Conn) -> Result<(), tide::Error> {
    let state = req.state();
    let list_events = task::Builder::new()
        .name("ws_list_events".to_string())
        .spawn(ws_list_events(state.clone(), conn.clone()))?;
    let _list_events_guard = scopeguard::guard((), |_| {
        // without explicit cancellation, the websocket is kept alive even after garbage input by the outgoing events

        // jump through hoops to call an async thing in a non-async context
        task::Builder::new()
            .name("ws_list_events cancel".to_string())
            .spawn(async { list_events.cancel().await })
            .unwrap();
    });

    while let Some(Ok(msg)) = conn.next().await {
        match msg {
            ws::Message::Ping(_) | ws::Message::Close(_) => {
                // handled automatically by lower levels
            }
            ws::Message::Pong(_) => {
                // ignore
            }
            ws::Message::Text(input) => {
                let command: proto::WSCommand = match serde_json::from_str(&input) {
                    Ok(cmd) => cmd,
                    Err(error) => {
                        debug!("websocket invalid JSON", {
                            error: log::kv::Value::capture_error(&error),
                            input: input,
                        });
                        return Err(tide::Error::from_str(
                            StatusCode::BadRequest,
                            "invalid JSON",
                        ));
                    }
                };
                websocket_command(state, command).await;
            }
            ws::Message::Binary(_) => {
                debug!("websocket unexpected input");
                return Err(tide::Error::from_str(StatusCode::BadRequest, "TODO"));
            }
        }
    }
    debug!("websocket end of stream");
    Ok(())
}

async fn websocket_command(state: &Arc<State>, command: proto::WSCommand) {
    // don't sleep for long, this blocks websocket message reading (to prevent command reordering)
    match command {
        proto::WSCommand::Play { filename } => {
            debug!("play file", { filename: filename });
            // Confirm that the file is in our state.files
            let exists = match state.media.get(&filename).expect("database error") {
                Some(item) => item.exists,
                None => false,
            };
            if !exists {
                // We might have removed the file concurrently, so this is not always an "attack".
                warn!("browser submitted invalid file", {
                    filename: &filename,
                });
                return;
            }
            let mut events = {
                let mut playing_guard = state.playing.lock().await;
                if playing_guard.is_some() {
                    debug!("already playing");
                    // TODO shut down old player and start new?
                    // don't want to lose place, maybe always run with --save-position-on-quit
                    //
                    // if not above, then inform frontend of error? then again, maybe i should just make "is playing" state visible to it, not as response to this.
                    return;
                }
                let mut mpv_builder = MPV::builder();
                mpv_builder.fullscreen(state.config.fullscreen);
                let mpv_config = match mpv_builder.build() {
                    Ok(builder) => builder,
                    Err(error) => {
                        warn!("error configuring MPV", {
                            error: log::kv::Value::capture_error(&error),
                        });
                        return;
                    }
                };
                // This is safe because we've confirmed the file is in our list of files, and that prevents hostile inputs.
                //
                // TODO Currently, validation checks that they are currently included in our known files, which is racy.
                //
                // RUST-WART No clean way to lexically prevent "/evil", "../evil" without reading symlinks and whatnot?
                let mut path = PathBuf::new();
                path.push(&state.config.path);
                path.push(&filename);
                let path = path.into_os_string();
                let mpv = match mpv_config.play(&path) {
                    Ok(mpv) => mpv,
                    Err(error) => {
                        warn!("cannot play media", {
                            filename: filename,
                            error: log::kv::Value::capture_error(&error),
                        });
                        return;
                    }
                };
                let events = mpv.events().await;
                *playing_guard = Some(mpv);
                events
            };

            let state = state.clone();
            task::spawn(async move {
                while let Some(event) = events.next().await {
                    debug!("mpv event", {
                        event: log::kv::Value::capture_debug(&event),
                    });
                }

                let mut playing_guard = state.playing.lock().await;
                // unset playing and return old value, so we can consume it in close
                let previous = std::mem::replace(&mut *playing_guard, None);
                // TODO nothing says it's still the *same* mpv; this is brittle.
                //
                // Try to get something where self.playing is a data structure
                // that "becomes None" when this task exits.
                match previous {
                    None => {
                        // no idea how that happened
                        debug!("internal error: playing is unexpectedly not set");
                        return;
                    }
                    Some(mpv) => match mpv.close().await {
                        Err(error) => warn!("mpv error", {
                            error: log::kv::Value::capture_error(&error),
                        }),
                        Ok(_) => {}
                    },
                }
            });
        }
    }
}

#[derive(structopt::StructOpt, Debug)]
#[structopt(
    name = "choosy",
    about = "Choose a file and play it with mpv",
    no_version
)]
struct Opt {
    /// Load configuration from file
    #[structopt(long, parse(from_os_str))]
    config: PathBuf,

    /// Database location
    #[structopt(long, parse(from_os_str))]
    database: PathBuf,
}

#[async_std::main]
async fn main() -> anyhow::Result<()> {
    use anyhow::Context;

    tide::log::with_level(tide::log::LevelFilter::Debug);

    let opt = Opt::from_args();
    let config = Config::load(opt.config).context("error loading config file")?;
    let db = sled::open(opt.database).context("error opening database")?;
    let tree = db
        .open_tree("media")
        .context("error opening database table for media")?;
    let media = database::MediaDb::new(tree.clone());
    let state = Arc::new(State {
        config: config.clone(),
        media,
        playing: Mutex::new(None),
    });

    let _file_scanner = {
        let state = state.clone();
        std::thread::spawn(move || {
            loop {
                let path = Path::new(&state.config.path);
                let found = file_scanner::scan(path);
                let files: BTreeSet<String> = found.collect();
                // debug!("files", { files: log::kv::Value::capture_debug(&files) });
                let db = state.media.scan_prefix("");
                let merge = itertools::merge_join_by(db, files, |result, file_path| match result {
                    Ok((key, _item)) => key.cmp(&sled::IVec::from(file_path.as_str())),
                    Err(_) => return std::cmp::Ordering::Less,
                });
                for merged in merge {
                    // debug!("merge", { merged: log::kv::Value::capture_debug(&merged) });
                    use itertools::EitherOrBoth::*;
                    match merged {
                        Left(Err(error)) => warn!("file scanner: database error", {
                            error: log::kv::Value::capture_error(&error),
                        }),
                        Left(Ok((key, item))) => {
                            // Found in database, not on filesystem.
                            if item.exists {
                                let result = state
                                    .media
                                    .merge(key, &vec![database::media::Op::Exists(false)]);
                                match result {
                                    Ok(_) => (),
                                    Err(error) => warn!("file scanner: database error", {
                                        error: log::kv::Value::capture_error(&error),
                                    }),
                                }
                            }
                        }
                        Right(file_path) => {
                            // Found on filesystem, not in database
                            let result = state
                                .media
                                .merge(file_path, &vec![database::media::Op::Exists(true)]);
                            match result {
                                Ok(_) => (),
                                Err(error) => warn!("file scanner: database error", {
                                    error: log::kv::Value::capture_error(&error),
                                }),
                            }
                        }
                        Both(Err(error), _) => warn!("file scanner: database error", {
                            error: log::kv::Value::capture_error(&error),
                        }),
                        Both(Ok((key, item)), _file_path) => {
                            // Found in both; ensure database says exists=true.
                            if !item.exists {
                                let result = state
                                    .media
                                    .merge(key, &vec![database::media::Op::Exists(true)]);
                                match result {
                                    Ok(_) => (),
                                    Err(error) => warn!("file scanner: database error", {
                                        error: log::kv::Value::capture_error(&error),
                                    }),
                                }
                            }
                        }
                    }
                }
                // TODO inotify
                std::thread::sleep(Duration::from_secs(9));
            }
        })
    };

    let mut app = tide::with_state(state);
    app.at("/choosy_frontend_bg.wasm").get(wasm_bg);
    app.at("/choosy_frontend.js").get(wasm_js);
    app.at("/").get(index_html);
    app.at("/ws").get(ws::Handle::new(websocket));

    let mut fds = ListenFd::from_env();
    let listener = fds
        .take_tcp_listener(0)
        .context("LISTEN_FDS must set up listening sockets")?
        .ok_or(anyhow::anyhow!("LISTEN_FDS must have set up a TCP socket"))?;
    app.listen(listener).await?;
    Ok(())
}
