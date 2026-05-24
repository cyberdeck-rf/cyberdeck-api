//! `cyberdeck-cli inject <module> <freq> --payload <hex>` — injecte une
//! trame fake dans la RX ring du module ciblé.
//!
//! Utile pour valider le pipeline firmware sans antenne réelle :
//! l'INJECT déclenche un FRAME async que le drain_rx_frames() côté firmware
//! renvoie immédiatement.  En mode loopback firmware (`make USE_LOOPBACK=1`)
//! on peut tester end-to-end sans toucher au matériel RF.

use anyhow::Result;
use cyberdeck_api::{Module, RfFrame};

pub async fn run(
    target: &super::Target,
    module: Module,
    freq: u32,
    payload_hex: &str,
    rssi: i16,
    snr: i8,
) -> Result<()> {
    // Validation hex côté hôte AVANT de l'envoyer — évite un round-trip NACK
    // qui ne dit pas où l'octet est foireux.
    if !payload_hex.len().is_multiple_of(2) {
        anyhow::bail!("payload_hex must have even length");
    }
    for c in payload_hex.chars() {
        if !c.is_ascii_hexdigit() {
            anyhow::bail!("invalid hex char '{}' in payload", c);
        }
    }

    let mut client = super::open_client(target).await?;
    let _ = client.handshake().await?;
    // Pour que l'INJECT soit pris (le mock vérifie `is_listening`), on arme
    // d'abord le module.  Si tu n'en veux pas, le firmware fait quand même
    // un start_listen implicite dans h_inject().
    let _ = client.start_listen(module, freq).await?;
    let frame = RfFrame {
        module,
        freq,
        rssi,
        snr,
        payload_hex: payload_hex.to_uppercase(),
        ts_ms: 0,  // ignoré par le firmware en INJECT (réécrit côté carte)
    };
    let ack = client.inject(&frame).await?;
    println!("✓ injected (seq={})", ack.seq);
    Ok(())
}
