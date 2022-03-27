use serde_json::json;
use std::ffi::OsStr;
use tracing::info;

use mpv_remote::MPV;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let mpv = MPV::builder()
        .fullscreen(false)
        .build()?
        .play(OsStr::new("/home/tv/tmp/z.mkv"))?;

    info!(
        "client name is %{:#?}",
        mpv.command(json!(["client_name"]))
            .await
            .expect("client_name bork")
    );
    mpv.command(json!(["observe_property", 42, "time-pos"]))
        .await
        .expect("observe bork");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    mpv.close().await?;
    Ok(())
}
