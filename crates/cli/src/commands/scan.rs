//! `cyberdeck-cli scan <module> <freq> [--duration N]` — arme un module
//! puis stream les FRAME async sur stdout.
//!
//! Format de sortie : une ligne JSON par FRAME, pretty-printé via serde_json.
//! Ctrl-C pour arrêter quand `--duration 0`.

use anyhow::Result;
use cyberdeck_api::Module;
use std::time::{Duration, Instant};

pub async fn run(target: &super::Target, module: Module, freq: u32, duration_s: u64) -> Result<()> {
    let mut client = super::open_client(target).await?;
    let _ = client.handshake().await?;  // sanity check
    client.start_listen(module, freq).await?;
    tracing::info!(?module, freq, duration_s, "scanning");

    let deadline = if duration_s == 0 {
        None
    } else {
        Some(Instant::now() + Duration::from_secs(duration_s))
    };

    loop {
        if let Some(d) = deadline {
            if Instant::now() >= d { break; }
        }
        match client.next_frame().await? {
            Some(f) => {
                // Stream NDJSON sur stdout : un objet par ligne, pipe-friendly.
                println!("{}", serde_json::to_string(&f)?);
            }
            None => break,
        }
    }

    let _ = client.stop(module).await;
    Ok(())
}
