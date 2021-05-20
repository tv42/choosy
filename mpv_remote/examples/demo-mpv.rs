#[allow(unused_imports)]
use async_std::prelude::*;
use kv_log_macro::info;
use serde_json::json;
use std::ffi::OsStr;

use mpv_remote::MPV;

// fn process_event(event: mpv::Event) {
//     info!("event", {event: event});
// }

#[async_std::main]
async fn main() -> anyhow::Result<()> {
    femme::with_level(femme::LevelFilter::Debug);

    let mpv = MPV::builder().fullscreen(false).build()?.play(
        OsStr::new("/home/tv/tmp/z.mkv"),
        // process_event,
    )?;

    info!(
        "client name is %{:#?}",
        mpv.command(json!(["client_name"]))
            .await
            .expect("client_name bork")
    );
    mpv.command(json!(["observe_property", 42, "time-pos"]))
        .await
        .expect("observe bork");
    async_std::task::sleep(std::time::Duration::from_secs(5)).await;
    mpv.close().await?;
    Ok(())
}
