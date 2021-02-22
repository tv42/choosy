// the html! macro failed to build without this
#![recursion_limit = "256"]

use anyhow;
use choosy_protocol as proto;
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;
use yew;
use yew::callback::Callback;
use yew::format::Json;
use yew::prelude::*;
use yew::services::websocket;

struct Model {
    link: ComponentLink<Self>,
    _ws: websocket::WebSocketTask,
    search: String,
    files: BTreeMap<String, ()>,
}

enum Msg {
    UpdateSearch { s: String },
    ServerError(anyhow::Error),
    FromServer(proto::WSEvent),
}

fn ws_notification(status: websocket::WebSocketStatus) {
    // TODO
    yew::services::ConsoleService::info(&format!("ws status: {:?}", status));
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();
    fn create(_: Self::Properties, link: ComponentLink<Self>) -> Self {
        let host = yew::utils::host().unwrap();
        let url = format!("ws://{}/ws", host);

        let ws_msg = link.callback(|text: yew::format::Text| {
            let Json(result) = Json::from(text);
            match result {
                Ok(data) => Msg::FromServer(data),
                Err(error) => Msg::ServerError(error),
            }
        });

        let ws = websocket::WebSocketService::connect_text(
            &url,
            ws_msg,
            Callback::from(ws_notification),
        )
        .unwrap();
        Self {
            link,
            _ws: ws,
            search: "".to_string(),
            files: BTreeMap::new(),
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::UpdateSearch { s } => {
                self.search = s;
            }
            Msg::ServerError(error) => {
                yew::services::ConsoleService::info(&format!(
                    "error reading from server: {:?}",
                    error
                ));
            }
            Msg::FromServer(msg) => {
                yew::services::ConsoleService::info(&format!("server says: {:?}", msg));
                match msg {
                    proto::WSEvent::FileChange(change) => match change {
                        proto::FileChange::ClearAll => {
                            self.files.clear();
                        }
                        proto::FileChange::Add { name } => {
                            self.files.insert(name, ());
                        }
                        proto::FileChange::Del { name } => {
                            self.files.remove(&name);
                        }
                    },
                };
            }
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
        html! {
            <>
                <input
                    placeholder="Search"
                    value=&self.search
                    oninput=self.link.callback(|e: yew::InputData| Msg::UpdateSearch{s: e.value})
                    style="width: 100%;"
                />
                <ul>
                    { for self.files.iter().filter(|(name, _)| name.contains(&self.search)).map(|(name,_)| html!{<li>{ name }</li>}) }
                </ul>
            </>
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
