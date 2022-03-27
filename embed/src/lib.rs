use std::{concat, env, include_bytes};

pub fn wasm() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/wasm/choosy_frontend_bg.wasm"))
}

pub fn wasm_js() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/wasm/choosy_frontend.js"))
}
