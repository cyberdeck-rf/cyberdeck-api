//! `cyberdeck-cli list-ports` — diagnostic : énumère tous les ports série
//! visibles avec leur identifiant USB.

use anyhow::Result;
use comfy_table::{ContentArrangement, Table, presets::UTF8_BORDERS_ONLY};
use cyberdeck_api::SerialTransport;

pub fn run() -> Result<()> {
    let ports = SerialTransport::list_candidates()?;
    if ports.is_empty() {
        println!("(no serial ports detected)");
        return Ok(());
    }
    let mut table = Table::new();
    table
        .load_preset(UTF8_BORDERS_ONLY)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["port", "product"]);
    for (name, product) in ports {
        table.add_row(vec![name, product.unwrap_or_else(|| "<unknown>".into())]);
    }
    println!("{table}");
    Ok(())
}
