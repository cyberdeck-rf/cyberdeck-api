//! `cyberdeck-cli status` — handshake + affichage joli des capacités.
//!
//! Utilise [`comfy_table`] pour rendre une table ASCII propre. Format de
//! sortie inspiré de `cargo --version` + `rustup show`.

use anyhow::Result;
use comfy_table::{ContentArrangement, Table, presets::UTF8_BORDERS_ONLY};
use cyberdeck_api::{CAP_EMU, CAP_RX, CAP_SNIFF, CAP_TX};

pub async fn run(target: &super::Target) -> Result<()> {
    let mut client = super::open_client(target).await?;
    let caps = client.handshake().await?;
    let status = client.status().await?;

    println!(
        "✓ connected — fw {fw}, hw {hw}, proto v{pv}, uptime {ms} ms",
        fw = caps.fw_version,
        hw = caps.hw,
        pv = caps.proto_version,
        ms = status.uptime_ms.unwrap_or(0),
    );

    let mut table = Table::new();
    table
        .load_preset(UTF8_BORDERS_ONLY)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["#", "name", "capabilities"]);

    for m in &caps.modules {
        let mut caps_str = String::new();
        if m.caps & CAP_RX    != 0 { caps_str.push_str("RX ");    }
        if m.caps & CAP_TX    != 0 { caps_str.push_str("TX ");    }
        if m.caps & CAP_SNIFF != 0 { caps_str.push_str("SNIFF "); }
        if m.caps & CAP_EMU   != 0 { caps_str.push_str("EMU ");   }
        table.add_row(vec![m.id.to_string(), m.name.clone(), caps_str.trim().to_string()]);
    }

    println!("{table}");
    Ok(())
}
