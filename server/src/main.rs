use axum::extract::Query;
use axum::http::header::HeaderName;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::StatusCode;
use axum::response::Html;
use axum::Json;
use choosy_protocol as proto;
use listenfd::ListenFd;
use mpv_remote::MPV;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use structopt::StructOpt;
#[allow(unused_imports)]
use tracing::{debug, error, info, log, trace, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod database;
mod file_scanner;
use config::Config;

#[derive(Clone, PartialEq)]
struct File {}

struct State {
    config: Config,
    media: database::MediaDb,
    playing: tokio::sync::Mutex<Option<MPV>>,
}

async fn wasm_bg() -> (HeaderMap, &'static [u8]) {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("application/wasm"),
    );
    (headers, choosy_embed::wasm())
}

async fn wasm_js() -> (HeaderMap, &'static [u8]) {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("application/javascript"),
    );
    (headers, choosy_embed::wasm_js())
}

async fn index_html() -> Html<&'static [u8]> {
    let bytes = include_bytes!("../../frontend/static/index.html");
    Html(bytes)
}

fn build_search_re(query: &str) -> regex::Regex {
    let mut re = String::new();
    for fragment in query.split_whitespace() {
        if !re.is_empty() {
            re.push_str(".*");
        }
        re.push_str(&regex::escape(fragment));
    }
    regex::RegexBuilder::new(&re)
        .case_insensitive(true)
        .build()
        // silently match everything on trouble; there really shouldn't be any, as we're escaping the input
        .unwrap_or_else(|_| regex::Regex::new("").unwrap())
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn handle_search(
    state: Arc<State>,
    query: Query<SearchQuery>,
) -> Result<Json<proto::SearchResponse>, StatusCode> {
    let search_re = build_search_re(&query.q);

    let iter = state
        .media
        .scan_prefix("")
        .filter_map(|result| match result {
            Err(error) => Some(Err(error)),
            Ok((key, item)) => {
                if !item.exists {
                    return None;
                }

                let filename = String::from_utf8_lossy(key.as_ref()).into_owned();
                if !search_re.is_match(&filename) {
                    return None;
                }

                let hit = proto::SearchResult { filename };
                Some(Ok(hit))
            }
        })
        .take(1000);
    let items: Result<
        Vec<proto::SearchResult>,
        sleigh::GetError<<sleigh::encoding::Bincode as sleigh::encoding::Encoding>::Error>,
    > = iter.collect();
    let items = items.map_err(|error| {
        warn!(message = "database error", ?error);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let result = proto::SearchResponse { items };
    Ok(Json(result))
}

async fn handle_play(
    state: Arc<State>,
    Json(input): Json<proto::PlayCommand>,
) -> Result<(), StatusCode> {
    let filename = input.filename;
    debug!(message = "play file", %filename);
    // Confirm that the file is in our state.files
    let exists = match state.media.get(&filename).expect("database error") {
        Some(item) => item.exists,
        None => false,
    };
    if !exists {
        // We might have removed the file concurrently, so this is not always an "attack".
        warn!(message = "browser submitted invalid file", %filename);
        return Ok(());
    }
    let mut events = {
        let mut playing_guard = state.playing.lock().await;
        if playing_guard.is_some() {
            debug!("already playing");
            // TODO shut down old player and start new?
            // don't want to lose place, maybe always run with --save-position-on-quit
            //
            // if not above, then inform frontend of error? then again, maybe i should just make "is playing" state visible to it, not as response to this.
            return Ok(());
        }
        let mut mpv_builder = MPV::builder();
        mpv_builder.fullscreen(state.config.fullscreen);
        let mpv_config = match mpv_builder.build() {
            Ok(builder) => builder,
            Err(error) => {
                warn!(message = "error configuring MPV", ?error);
                return Ok(());
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
                warn!(message = "cannot play media", %filename, ?error);
                return Ok(());
            }
        };
        let events = mpv.events().await;
        *playing_guard = Some(mpv);
        events
    };

    let state = state.clone();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => debug!(message = "mpv event", ?event),
                Err(error) => match error {
                    tokio::sync::broadcast::error::RecvError::Closed => break,
                    tokio::sync::broadcast::error::RecvError::Lagged(count) => {
                        debug!(message = "mpv events receiver lagged", count);
                        break;
                    }
                },
            }
        }

        let mut playing_guard = state.playing.lock().await;
        // unset playing and return old value, so we can consume it in close
        let previous = std::mem::replace(&mut *playing_guard, None);
        // TODO nothing says it's still the *same* mpv; this is brittle.
        //
        // Try to get something where self.playing is a data structure
        // that "becomes None" when this task exits.
        if previous.is_none() {
            // no idea how that happened
            debug!("internal error: playing is unexpectedly not set");
            return;
        }
        let mpv = previous.unwrap();
        if let Err(error) = mpv.close().await {
            warn!(message = "mpv error", ?error);
        }
    });
    Ok(())
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

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    use anyhow::Context;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "choosy=debug,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

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
        playing: tokio::sync::Mutex::new(None),
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
                    Err(_) => std::cmp::Ordering::Less,
                });
                for merged in merge {
                    // debug!("merge", { merged: log::kv::Value::capture_debug(&merged) });
                    use itertools::EitherOrBoth::*;
                    match merged {
                        Left(Err(error)) => warn!(message = "file scanner: database error", ?error),
                        Left(Ok((key, item))) => {
                            // Found in database, not on filesystem.
                            if item.exists {
                                let result = state
                                    .media
                                    .merge(key, &vec![database::media::Op::Exists(false)]);
                                match result {
                                    Ok(_) => (),
                                    Err(error) => {
                                        warn!(message = "file scanner: database error", ?error)
                                    }
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
                                Err(error) => {
                                    warn!(message = "file scanner: database error", ?error)
                                }
                            }
                        }
                        Both(Err(error), _) => {
                            warn!(message = "file scanner: database error", ?error)
                        }
                        Both(Ok((key, item)), _file_path) => {
                            // Found in both; ensure database says exists=true.
                            if !item.exists {
                                let result = state
                                    .media
                                    .merge(key, &vec![database::media::Op::Exists(true)]);
                                match result {
                                    Ok(_) => (),
                                    Err(error) => {
                                        warn!(message = "file scanner: database error", ?error)
                                    }
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

    use axum::routing::{get, post};
    let app = axum::Router::new()
        .route("/choosy_frontend_bg.wasm", get(wasm_bg))
        .route("/choosy_frontend.js", get(wasm_js))
        .route("/", get(index_html))
        .route(
            "/search",
            get({
                let state = Arc::clone(&state);
                move |query| handle_search(state, query)
            }),
        )
        .route(
            "/play",
            post({
                let state = Arc::clone(&state);
                move |input| handle_play(state, input)
            }),
        )
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let mut fds = ListenFd::from_env();
    let listener = fds
        .take_tcp_listener(0)
        .context("LISTEN_FDS must set up listening sockets")?
        .ok_or_else(|| anyhow::anyhow!("LISTEN_FDS must have set up a TCP socket"))?;

    axum::Server::from_tcp(listener)?
        .serve(app.into_make_service())
        .await?;
    Ok(())
}
