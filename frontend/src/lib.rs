// the html! macro failed to build without this
#![recursion_limit = "256"]

use choosy_protocol as proto;
use gloo_net::http::Request;
use std::collections::BTreeMap;
use std::rc::Rc;
use tracing::{error, info};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use yew::prelude::*;

struct Model {
    search: Rc<str>,
    files: BTreeMap<Rc<str>, ()>,
}

enum Msg {
    UpdateSearch {
        search: Rc<str>,
    },
    SearchResult {
        result: Result<proto::SearchResponse, gloo_net::Error>,
    },
    Play {
        filename: Rc<str>,
    },
}

fn build_url(relative: &str) -> Result<web_sys::Url, JsValue> {
    let base_url = {
        let window = web_sys::window().expect("must have JS window");
        let document = window.document().expect("must have JS document");
        let location = document.location().expect("must have JS document.location");
        location
            .href()
            .expect("must have JS document.location.href")
    };
    web_sys::Url::new_with_base(relative, &base_url)
}

fn build_search_url(search: &str) -> String {
    let url = build_url("/search").expect("programmer error: hardcoded URL is invalid");
    let query = url.search_params();
    query.set("q", search);
    url.set_search(
        &query
            .to_string()
            .as_string()
            .expect("internal error: bad URL query stringification"),
    );
    url.to_string()
        .as_string()
        .expect("internal error: bad URL stringification")
}

fn build_play_url() -> String {
    let url = build_url("/play").expect("programmer error: hardcoded URL is invalid");
    url.to_string()
        .as_string()
        .expect("internal error: bad URL stringification")
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        ctx.link().send_message(Msg::UpdateSearch {
            search: Rc::from(""),
        });
        Self {
            search: Rc::from(""),
            files: BTreeMap::new(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::UpdateSearch { search } => {
                self.search = search.clone();
                ctx.link().send_future(async move {
                    let result = {
                        let url = build_search_url(&search);
                        let resp = Request::get(&url).send().await;
                        match resp {
                            Ok(response) => response.json::<proto::SearchResponse>().await,
                            Err(error) => Err(error),
                        }
                    };
                    Msg::SearchResult { result }
                });
            }

            Msg::SearchResult { result } => {
                match result {
                    Err(error) => {
                        error!(message = "search failed", ?error);
                    }
                    Ok(response) => {
                        self.files.clear();
                        self.files.extend(response.items.iter().map(|item| {
                            // Do not ask me why this has to be here.
                            // All I know is it didn't work, and I copied this from `Rc::from` for `From<String>`.
                            let s = &item.filename[..];
                            (Rc::from(s), ())
                        }));
                    }
                }
            }
            Msg::Play { filename } => {
                // TODO indicate things in UI somehow, SSE for play status?
                info!(message = "playing", filename = filename.as_ref());
            }
        };
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let entries = self.files.iter();
        let oninput = ctx.link().callback_future(|event: InputEvent| async move {
            let target = event.target().expect("oninput event must have target");
            let search: String = target.unchecked_into::<web_sys::HtmlInputElement>().value();
            Msg::UpdateSearch {
                search: Rc::from(search),
            }
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
                    let callback = ctx.link().callback_future(move |_| {
                        let tmp = tmp.clone();
                        async move {
                            let url = build_play_url();
                            let cmd = proto::PlayCommand{
                                filename: tmp.to_string()
                            };
                            let buf = serde_json::to_vec(&cmd).expect("JSON serialize of PlayCommand must work");
                            let arr = js_sys::Uint8Array::from(&buf[..]);
                            let resp = Request::post(&url)
                                .header("content-type", "application/json")
                                .body(arr)
                                .send().await;
                            match resp {
                                Ok(_response) => {
                                    // TODO status of playback as SSE?
                                },
                                Err(error) => {
                                    error!(message="requesting play failed", ?error);
                                },
                            }
                            Msg::Play { filename: tmp }
                        }
                    });
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
    tracing_wasm::set_as_global_default();

    yew::start_app::<Model>();
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
