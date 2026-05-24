//! Plugin WiFi 6 / BLE 5 / Zigbee / Thread / Matter (Espressif ESP32-C6).
//!
//! Cheatsheet :
//! * **KRACK** — Vanhoef & Piessens, ACM CCS 2017 (WPA2 4-way handshake).
//! * **FragAttacks** — Vanhoef, USENIX 2021.
//! * **KNOB** (CVE-2019-9506) — Antonioli, USENIX 2019.
//! * **ZigBee worm** — Ronen et al., IEEE S&P 2017 (propagation Hue→Hue).

use super::{DecodedFrame, Plugin};
use crate::proto::schema::{Module, RfFrame};

/// Plugin singleton.
pub struct WifiBle;

impl Plugin for WifiBle {
    fn module(&self) -> Module { Module::Esp32C6 }
    fn human_name(&self) -> &'static str { "WiFi 6 / BLE / Zigbee (ESP32-C6)" }
    fn known_attacks(&self) -> &'static [&'static str] {
        &[
            "KRACK (Vanhoef 2017)",
            "FragAttacks (Vanhoef 2021)",
            "KNOB CVE-2019-9506",
            "ZigBee worm (Ronen 2017)",
        ]
    }

    fn decode(&self, frame: &RfFrame) -> DecodedFrame {
        // `frame.freq` est u32 → portée max 4.29 GHz, suffisante pour 2.4 GHz
        // mais pas 5 GHz.  Le support 5 GHz est de toute façon v2.0 (specs
        // RF_Cyberdeck_Specifications_Decisions.md → Out of scope v1) ;
        // quand on rajoutera le 5 GHz on bumpera `RfFrame.freq` vers u64 dans
        // schema.rs + schema.hpp en synchro.
        let kind = match frame.freq {
            2_400_000_000..=2_500_000_000 => "2.4 GHz (WiFi/BLE/Zigbee)",
            _ => "Unknown band",
        };
        DecodedFrame {
            summary: format!("{kind} payload {} bytes", frame.payload_hex.len() / 2),
            details: serde_json::json!({
                "band": kind,
                "freq_hz": frame.freq,
                "rssi_dbm": frame.rssi,
                "payload_hex": frame.payload_hex,
            }),
        }
    }
}
