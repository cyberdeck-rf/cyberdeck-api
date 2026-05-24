//! Plugin RFID LF 125 kHz (frontend discret PWM + MOSFET + comparateur,
//! même architecture que Flipper Zero).
//!
//! Cheatsheet :
//! * **EM4102 / HID ProxCard II** — Westhues 2006. ID en clair en ASK, aucune
//!   crypto. Lecture passive 5-15 cm, clonage sur T5577.

use super::{DecodedFrame, Plugin};
use crate::proto::schema::{Module, RfFrame};

/// Plugin singleton.
pub struct RfidLf;

impl Plugin for RfidLf {
    fn module(&self) -> Module { Module::RfidLf }
    fn human_name(&self) -> &'static str { "RFID LF 125 kHz" }
    fn known_attacks(&self) -> &'static [&'static str] {
        &["EM4102/HID ProxCard II read+clone (Westhues 2006)"]
    }

    fn decode(&self, frame: &RfFrame) -> DecodedFrame {
        DecodedFrame {
            summary: format!("LF 125 kHz badge dump (hex {})", &frame.payload_hex),
            details: serde_json::json!({
                "freq_hz": frame.freq,
                "payload_hex": frame.payload_hex,
            }),
        }
    }
}
