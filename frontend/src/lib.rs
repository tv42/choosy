// the html! macro failed to build without this
#![recursion_limit = "256"]

use anyhow;
use choosy_protocol as proto;
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;
use websocket::WebSocketStatus;
use yew::callback::Callback;
use yew::format::Json;
use yew::prelude::*;
use yew::services::websocket;

mod backoff;

struct Model {
    link: ComponentLink<Self>,
    ws: Option<websocket::WebSocketTask>,
    ws_backoff: backoff::Backoff,
    search: String,
    search_re: regex::Regex,
    files: BTreeMap<String, ()>,
}

enum Msg {
    UpdateSearch { s: String },
    ServerError(anyhow::Error),
    FromServer(proto::WSEvent),
    WebSocketStatus(WebSocketStatus),
    ConnectWebSocket,
    Play { filename: String },
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
    fn create(_: Self::Properties, link: ComponentLink<Self>) -> Self {
        link.send_message(Msg::ConnectWebSocket);
        Self {
            link,
            ws: None,
            ws_backoff: backoff::Backoff::new(),
            search: "".to_string(),
            search_re: build_search_re(""),
            files: BTreeMap::new(),
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::UpdateSearch { s } => {
                self.search_re = build_search_re(&s);
                self.search = s;
            }
            Msg::ServerError(error) => {
                yew::services::ConsoleService::info(&format!(
                    "error reading from server: {:?}",
                    error
                ));
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
                                    self.files.insert(name, ());
                                }
                                proto::FileChange::Del { name } => {
                                    self.files.remove(&name);
                                }
                            }
                        }
                    }
                };
            }
            Msg::WebSocketStatus(status) => {
                yew::services::ConsoleService::info(&format!("ws status: {:?}", status));
                match status {
                    WebSocketStatus::Opened => {
                        self.ws_backoff.success();
                    }
                    WebSocketStatus::Closed | WebSocketStatus::Error => {
                        self.ws = None;

                        // trigger a reconnect, after a delay
                        let delay = self.ws_backoff.delay();
                        yew::services::ConsoleService::info(&format!(
                            "WebSocket connection {:?}, retrying in {:?}...",
                            status, delay,
                        ));
                        let callback = self.link.callback(|_| Msg::ConnectWebSocket);

                        // Yew TimeoutService has a weird API where the timer is cancelled if you drop the returned task, and it provides no way to avoid that. Just assume web-sys and go directly to the underlying, slightly saner, API.
                        use gloo::timers::callback::Timeout;
                        let js_callback = move || {
                            callback.emit(());
                        };
                        let timeout = Timeout::new(delay.as_millis() as u32, js_callback);
                        timeout.forget();
                    }
                }
            }
            Msg::ConnectWebSocket => match self.ws {
                Some(_) => {
                    yew::services::ConsoleService::info(
                        "asked to connect websocket, but it's already connected",
                    );
                }
                None => {
                    let ws_msg = self.link.callback(|text: yew::format::Text| {
                        let Json(result) = Json::from(text);
                        match result {
                            Ok(data) => Msg::FromServer(data),
                            Err(error) => Msg::ServerError(error),
                        }
                    });
                    let ws_status = self.link.callback(|status| Msg::WebSocketStatus(status));
                    let host = yew::utils::host().unwrap();
                    let url = format!("ws://{}/ws", host);
                    let ws = websocket::WebSocketService::connect_text(
                        &url,
                        ws_msg,
                        Callback::from(ws_status),
                    )
                    .unwrap();
                    self.ws = Some(ws);
                }
            },
            Msg::Play { filename } => match &mut self.ws {
                None => {
                    yew::services::ConsoleService::info(&format!(
                        "asked to play but not connected: {:?}",
                        filename
                    ));
                    self.link.send_message(Msg::ConnectWebSocket);
                }
                Some(ws) => {
                    let cmd = proto::WSCommand::Play { filename };
                    ws.send(Json(&cmd));
                }
            },
        }
        true
    }

    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
        // Should only return "true" if new properties are different to
        // previously received properties.
        // This component has no properties so we will always return "false".
        false
    }

    fn view(&self) -> Html {
        let entries = self
            .files
            .iter()
            .filter(|(name, _)| self.search_re.is_match(name))
            // TODO relax this once the web UI agrees to be responsive enough
            .take(1000);
        html! {
            <>
                <input
                    placeholder="Search"
                    // WAITING store self.search as Rc<str> to avoid
                    // copying string contents on every view, needs
                    // yew support
                    //
                    // https://github.com/yewstack/yew/issues/1851
                    value=self.search.clone()
                    oninput=self.link.callback(|e: yew::InputData| Msg::UpdateSearch{s: e.value})
                    style="width: 100%;"
                />
                <ul>
                  {for entries.map(|(filename,_)| self.item_view(filename))}
                </ul>
            </>
        }
    }
}

impl Model {
    fn item_view(&self, filename: &str) -> Html {
        let n = filename.to_string();
        html! {
            <li onclick=self.link.callback(move |_| Msg::Play { filename: n.clone() })>{filename}</li>
        }
    }
}

#[wasm_bindgen(start)]
pub fn run_app() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));

    App::<Model>::new().mount_to_body();
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
