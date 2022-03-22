//! WASM WebSocket wrapper that doesn't pretend to be async, since browser WebSockets sure ain't.[^1]
//! This radically simplifies lifetime management.
//!
//! [^1]: "If the data can't be sent (for example, because it needs to be buffered but the buffer is full), the socket is closed automatically.""
//!   -- <https://developer.mozilla.org/en-US/docs/Web/API/WebSocket/send>

use wasm_bindgen::{closure::Closure, JsCast};

mod console {
    pub use weblog::console_info as info;
}

#[derive(Debug)]
pub enum WebSocketError {
    BadUrl(js_sys::SyntaxError),
}

#[derive(Debug)]
pub enum WebSocketSendError {
    InvalidStateError(web_sys::DomException),
    JSON(serde_json::Error),
}

pub struct WebSocket {
    inner: web_sys::WebSocket,

    // Save callbacks here so their lifetime is the same as the WebSocket's.
    #[allow(dead_code)]
    on_open: Closure<dyn FnMut(web_sys::Event)>,
    #[allow(dead_code)]
    on_error: Closure<dyn FnMut(web_sys::Event)>,
    #[allow(dead_code)]
    on_close: Closure<dyn FnMut(web_sys::CloseEvent)>,
    #[allow(dead_code)]
    on_message: Closure<dyn Fn(web_sys::MessageEvent)>,
}

impl WebSocket {
    pub fn new(
        url: &str,
        on_open: impl 'static + FnOnce(web_sys::Event),
        on_error: impl 'static + FnOnce(web_sys::Event),
        on_close: impl 'static + FnOnce(web_sys::CloseEvent),
        on_message: impl 'static + Fn(web_sys::MessageEvent),
    ) -> Result<Self, WebSocketError> {
        return web_sys::WebSocket::new(url)
            .map_err(js_sys::SyntaxError::unchecked_from_js)
            .map_err(|error| WebSocketError::BadUrl(error))
            .map(|inner| -> WebSocket {
                // TODO could use Blob and stream to serde_json
                inner.set_binary_type(web_sys::BinaryType::Arraybuffer);

                let on_open = Closure::once(Box::new(on_open) as Box<dyn FnOnce(web_sys::Event)>);
                inner.set_onopen(Some(on_open.as_ref().unchecked_ref()));

                let on_error = Closure::once(Box::new(on_error) as Box<dyn FnOnce(web_sys::Event)>);
                inner.set_onerror(Some(on_error.as_ref().unchecked_ref()));

                let on_close =
                    Closure::once(Box::new(on_close) as Box<dyn FnOnce(web_sys::CloseEvent)>);
                inner.set_onclose(Some(on_close.as_ref().unchecked_ref()));

                let on_message =
                    Closure::wrap(Box::new(on_message) as Box<dyn Fn(web_sys::MessageEvent)>);
                inner.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

                Self {
                    inner,
                    on_open,
                    on_error,
                    on_close,
                    on_message,
                }
            });
    }

    pub fn send_json<M: serde::ser::Serialize>(
        &self,
        message: M,
    ) -> Result<(), WebSocketSendError> {
        let buf = serde_json::to_vec(&message).map_err(|error| WebSocketSendError::JSON(error))?;
        self.inner
            .send_with_u8_array(&buf)
            .map_err(web_sys::DomException::unchecked_from_js)
            .map_err(|error| WebSocketSendError::InvalidStateError(error))
    }
}
