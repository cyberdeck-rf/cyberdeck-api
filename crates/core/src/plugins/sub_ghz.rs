//! Plugin Sub-GHz (Semtech SX1262 — 433 + 868 MHz LoRa/FSK).
//!
//! Cheatsheet d'attaques connues (cf. `cyberdeck/../slide_cheatsheet.typ`) :
//! * **RollJam** — Kamkar, DEF CON 2015. Jam + record, replay.
//! * **Jam-Listen-Replay** — Ibrahim et al., 2019.
//! * **RKE cloning** — Gesteira et al., 2025 ; SDR exclusivement.

use super::{DecodedFrame, Plugin};
use crate::proto::schema::{Module, RfFrame};

/// Plugin singleton.
pub struct SubGhz;

impl Plugin for SubGhz {
    fn module(&self) -> Module { Module::Sx1262 }
    fn human_name(&self) -> &'static str { "Sub-GHz LoRa/FSK (SX1262)" }
    fn known_attacks(&self) -> &'static [&'static str] {
        &["RollJam (Kamkar 2015)", "Jam-Listen-Replay (Ibrahim 2019)", "RKE replay"]
    }

    fn decode(&self, frame: &RfFrame) -> DecodedFrame {
        // Heuristique très simple : 868 MHz + payload court → probable beacon LoRa.
        let band = if frame.freq < 500_000_000 { "433 MHz ISM" } else { "868 MHz ISM" };
        DecodedFrame {
            summary: format!(
                "{band} payload {} bytes, RSSI {} dBm, SNR {} dB",
                frame.payload_hex.len() / 2, frame.rssi, frame.snr,
            ),
            details: serde_json::json!({
                "band": band,
                "freq_hz": frame.freq,
                "rssi_dbm": frame.rssi,
                "snr_db": frame.snr,
                "payload_hex": frame.payload_hex,
            }),
        }
    }
}
