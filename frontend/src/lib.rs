// the html! macro failed to build without this
#![recursion_limit = "256"]

use choosy_protocol as proto;
use std::collections::BTreeMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use yew::prelude::*;

mod backoff;

mod websocket;
use self::websocket::WebSocket;

mod console {
    pub use weblog::{console_error as error, console_info as info};
}

struct Model {
    ws: Option<WebSocket>,
    ws_backoff: backoff::Backoff,
    search: Rc<str>,
    search_re: regex::Regex,
    files: BTreeMap<Rc<str>, ()>,
}

enum Msg {
    UpdateSearch { s: String },
    Play { filename: Rc<str> },
    ConnectWebSocket,
    WebSocketOpened,
    WebSocketClosed,
    WebSocketJsonParseError(serde_json::Error),
    FromServer(proto::WSEvent),
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

impl Component for Model {
    type Message = Msg;
    type Properties = ();
    fn create(ctx: &Context<Self>) -> Self {
        ctx.link().send_message(Msg::ConnectWebSocket);
        Self {
            ws: None,
            ws_backoff: backoff::Backoff::new(),
            search: Rc::from(""),
            search_re: build_search_re(""),
            files: BTreeMap::new(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::UpdateSearch { s } => {
                self.search_re = build_search_re(&s);
                self.search = Rc::from(s);
            }
            Msg::WebSocketJsonParseError(error) => {
                console::info!(&format!("error reading from server: {:?}", error));
            }
            Msg::FromServer(msg) => {
                match msg {
                    proto::WSEvent::FileChange(changes) => {
                        for change in changes {
                            match change {
                                proto::FileChange::ClearAll => {
                                    self.files.clear();
                                }
                                proto::FileChange::Add { name } => {
                                    self.files.insert(Rc::from(name), ());
                                }
                                proto::FileChange::Del { name } => {
                                    self.files.remove(&Rc::from(name));
                                }
                            }
                        }
                    }
                };
            }
            Msg::WebSocketOpened => {
                console::info!("WebSocket connection opened");
                self.ws_backoff.success();
            }
            Msg::WebSocketClosed => {
                self.ws = None;

                // trigger a reconnect, after a delay
                let delay = self.ws_backoff.delay();
                console::info!(&format!(
                    "WebSocket connection closed, retrying in {:?}...",
                    delay,
                ));
                let callback = ctx.link().callback(|_| Msg::ConnectWebSocket);

                use gloo::timers::callback::Timeout;
                let js_callback = move || {
                    callback.emit(());
                };
                let timeout = Timeout::new(delay.as_millis() as u32, js_callback);
                timeout.forget();
            }
            Msg::ConnectWebSocket => match self.ws {
                Some(_) => {
                    console::info!("asked to connect websocket, but it's already connected",);
                }
                None => {
                    let host = {
                        let window = web_sys::window().expect("must have JS window");
                        let document = window.document().expect("must have JS document");
                        let location = document.location().expect("must have JS document.location");
                        location
                            .host()
                            .expect("must have JS document.location.host")
                    };
                    let url = format!("ws://{}/ws", host);

                    // let on_open = |_event| {};
                    let cb_open = ctx.link().callback_once(|_event| Msg::WebSocketOpened);
                    let on_open = move |event| cb_open.emit(event);
                    let on_error = |event| {
                        console::error!(format!("WebSocket closing due to error: {:?}", event));
                    };
                    let cb_close = ctx.link().callback_once(|_event| Msg::WebSocketClosed);
                    let on_close = move |event| cb_close.emit(event);
                    let cb_message =
                        ctx.link().callback(|buf: Vec<u8>| {
                            match serde_json::from_slice::<proto::WSEvent>(&buf) {
                                Err(error) => Msg::WebSocketJsonParseError(error),
                                Ok(msg) => Msg::FromServer(msg),
                            }
                        });
                    let on_message = move |event: web_sys::MessageEvent| {
                        // Do more work here, as `yew::html::Callback` *has to* return a message, and we conveniently ignore unrecognized things.
                        if let Ok(array_buf) = event.data().dyn_into::<js_sys::ArrayBuffer>() {
                            let buf = js_sys::Uint8Array::new(&array_buf).to_vec();
                            cb_message.emit(buf);
                        } else {
                            console::error!("unexpected WebSocket message type", event.data());
                        }
                    };
                    let ws = WebSocket::new(&url, on_open, on_error, on_close, on_message).unwrap();
                    self.ws = Some(ws);
                }
            },
            Msg::Play { filename } => match &self.ws {
                None => {
                    console::info!(&format!("asked to play but not connected: {:?}", filename));
                    ctx.link().send_message(Msg::ConnectWebSocket);
                }
                Some(ws) => {
                    let cmd = proto::WSCommand::Play {
                        filename: filename.to_string(),
                    };
                    match ws.send_json(&cmd) {
                        Ok(_) => {}
                        Err(error) => {
                            console::error!(format!("websocket send: {:?}", error));
                        }
                    };
                }
            },
        };
        true
    }

    fn changed(&mut self, _ctx: &Context<Self>) -> bool {
        // Should only return "true" if new properties are different to
        // previously received properties.
        // This component has no properties so we will always return "false".
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let entries = self
            .files
            .iter()
            .filter(|(name, _)| self.search_re.is_match(name))
            // TODO relax this once the web UI agrees to be responsive enough
            .take(1000);
        let oninput = ctx.link().callback(|event: InputEvent| {
            let target = event.target().expect("oninput event must have target");
            let search = target.unchecked_into::<web_sys::HtmlInputElement>().value();
            Msg::UpdateSearch { s: search }
        });
        html! {
            <>
                <div style="position: sticky; top: 0;">
                    <input
                        placeholder="Search"
                        value={yew::virtual_dom::AttrValue::from(self.search.clone())}
                        oninput={oninput}
                        // border-box makes borders be within width, not outside it
                        style="width: 100%;"
                    />
                </div>
                <ul style="padding-right: 10px;">
                  {for entries.map(|(filename, _)| {
                    let tmp = filename.clone();
                    let callback = ctx.link().callback(move |_| Msg::Play { filename: tmp.clone() });
                    html! {
                        <li onclick={callback}>{filename}</li>
                    }
                    })}
                </ul>
            </>
        }
    }
}

#[wasm_bindgen(start)]
pub fn run_app() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));

    yew::start_app::<Model>();
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
