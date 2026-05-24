//! Plugin NFC 13.56 MHz (ST ST25R3916).
//!
//! Cheatsheet d'attaques connues :
//! * **Mifare Crypto1 reversed** — Nohl & Plötz, 24C3 2007.
//! * **Hardnested** — Meijer & Verdult, ACM CCS 2015.
//! * **FM11RF08S backdoor** — Teuwen/Quarkslab IACR 2024.
//! * **Unsaflok** — Wouters et al., 2024 (clone hôtels MIFARE Classic).

use super::{DecodedFrame, Plugin};
use crate::proto::schema::{Module, RfFrame};

/// Plugin singleton.
pub struct Nfc;

impl Plugin for Nfc {
    fn module(&self) -> Module { Module::St25r3916 }
    fn human_name(&self) -> &'static str { "NFC 13.56 MHz (ST25R3916)" }
    fn known_attacks(&self) -> &'static [&'static str] {
        &[
            "Mifare Crypto1 break (Nohl 2007)",
            "Hardnested (Meijer 2015)",
            "FM11RF08S backdoor (Quarkslab 2024)",
            "Unsaflok (Wouters 2024)",
        ]
    }

    fn decode(&self, frame: &RfFrame) -> DecodedFrame {
        // Pour Mifare Classic : 4 ou 7 octets UID en début de payload.
        let n = frame.payload_hex.len() / 2;
        let summary = if n >= 4 && n <= 10 {
            format!("Probable ISO 14443 UID candidate: {}", &frame.payload_hex)
        } else {
            format!("NFC payload {} bytes", n)
        };
        DecodedFrame {
            summary,
            details: serde_json::json!({
                "freq_hz": frame.freq,
                "rssi_dbm": frame.rssi,
                "payload_hex": frame.payload_hex,
            }),
        }
    }
}
