#[allow(unused_imports)]
use async_std::io::prelude::*;
use async_std::io::Cursor;
use std::{concat, env, include_bytes};

pub fn wasm() -> Cursor<&'static [u8]> {
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/wasm/choosy_frontend_bg.wasm"));
    return Cursor::new(bytes);
}

pub fn wasm_js() -> Cursor<&'static [u8]> {
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/wasm/choosy_frontend.js"));
    return Cursor::new(bytes);
}
