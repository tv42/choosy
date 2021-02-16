#[allow(unused_imports)]
use async_std::prelude::*;
use async_std::stream::StreamExt;
use async_std::sync::Arc;
use async_std::task;
use choosy_embed;
use choosy_protocol as proto;
use http_types::mime;
use scopeguard;
use tide::log;
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

mod file_list;

#[derive(Clone, PartialEq)]
struct File {}

struct State {
    files: file_list::List,
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
    let mut stream = state.files.change_batches();
    loop {
        let batch = stream.next().await;
        for item in batch {
            conn.send_json(&proto::WSEvent::FileChange(item)).await?;
        }
    }
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
            ws::Message::Text(_) | ws::Message::Binary(_) => {
                log::debug!("websocket unexpected input");
                return Err(tide::Error::from_str(StatusCode::BadRequest, "TODO"));
            }
        }
    }
    log::debug!("websocket end of stream");
    Ok(())
}

async fn debug_add_file(mut req: Request<Arc<State>>) -> tide::Result {
    let body = req.body_string().await?;
    let state = req.state();
    state
        .files
        .update(vec![proto::FileChange::Add { name: body }].iter())
        .await;
    let resp = Response::new(StatusCode::Ok);
    Ok(resp)
}

#[async_std::main]
async fn main() -> anyhow::Result<()> {
    use anyhow::Context;

    log::with_level(log::LevelFilter::Debug);

    let state = Arc::new(State {
        files: file_list::List::new(),
    });

    state
        .files
        .update(
            vec![
                proto::FileChange::Add {
                    name: "foo".to_string(),
                },
                proto::FileChange::Add {
                    name: "quux".to_string(),
                },
                proto::FileChange::Add {
                    name: "bar".to_string(),
                },
            ]
            .iter(),
        )
        .await;

    let mut app = tide::with_state(state);
    app.at("/choosy_frontend_bg.wasm").get(wasm_bg);
    app.at("/choosy_frontend.js").get(wasm_js);
    app.at("/").get(index_html);
    app.at("/ws").get(ws::Handle::new(websocket));
    app.at("/api/debug/add-file").post(debug_add_file);

    use std::{env, net::TcpListener, os::unix::io::FromRawFd, os::unix::io::RawFd};
    let listen_fd = env::var("LISTEN_FD").context("LISTEN_FD must be set in environment")?;
    let fd: RawFd = listen_fd
        .parse()
        .context("LISTEN_FD must be a file descriptor")?;
    app.listen(unsafe { TcpListener::from_raw_fd(fd) }).await?;
    Ok(())
}
