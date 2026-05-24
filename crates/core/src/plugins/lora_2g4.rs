//! Plugin LoRa 2.4 GHz (Semtech SX1280) — ELRS FPV, IoT 2.4 GHz, etc.
//!
//! Cheatsheet :
//! * **ADR spoofing** — ChirpOTLE, ACM WiSec 2020.
//! * **BlackoutADR** — Hidawi et al. 2025 (battery drain LoRaWAN, défait
//!   les IDS ML CNN/LSTM/BiLSTM).

use super::{DecodedFrame, Plugin};
use crate::proto::schema::{Module, RfFrame};

/// Plugin singleton.
pub struct Lora2g4;

impl Plugin for Lora2g4 {
    fn module(&self) -> Module { Module::Sx1280 }
    fn human_name(&self) -> &'static str { "LoRa 2.4 GHz / ELRS (SX1280)" }
    fn known_attacks(&self) -> &'static [&'static str] {
        &["ADR spoofing (ChirpOTLE 2020)", "BlackoutADR (Hidawi 2025)"]
    }

    fn decode(&self, frame: &RfFrame) -> DecodedFrame {
        DecodedFrame {
            summary: format!(
                "LoRa 2.4 GHz payload {} bytes, RSSI {} dBm, SNR {} dB",
                frame.payload_hex.len() / 2, frame.rssi, frame.snr,
            ),
            details: serde_json::json!({
                "freq_hz": frame.freq,
                "rssi_dbm": frame.rssi,
                "snr_db": frame.snr,
                "payload_hex": frame.payload_hex,
            }),
        }
    }
}
