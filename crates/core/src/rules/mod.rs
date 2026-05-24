//! Moteur de règles pentest passif (rules engine).
//!
//! # Vue d'ensemble
//!
//! Le moteur consomme chaque [`crate::RfFrame`] reçue depuis le firmware et
//! évalue un pack de règles déclaratives décrites en TOML
//! (`catalog/default_rules.toml`).  Chaque règle qui matche émet un
//! [`Finding`] — exposé en interne (autres crates) puis à Dart via
//! `mobile-app/rust/src/api/`.
//!
//! Deux familles de matchers :
//!
//! * **stateless** ([`Matcher`]) : opère sur une seule frame (range fréquence,
//!   longueur du payload, regex hex, prefix hex, etc.).
//! * **stateful** ([`StatefulOp`]) : agrège plusieurs frames sur une fenêtre
//!   glissante par module (replay, rolling code immobile, burst en fréquence…).
//!
//! Le pack est mutable à chaud (cf. `replace_pack`) — c'est ainsi que
//! l'updater HTTPS distant (M5, autre agent) substitue les règles sans
//! recompiler.
//!
//! # Architecture
//!
//! ```text
//!  firmware  ──RfFrame──▶  ClientTask  ──▶  RulesEngine::evaluate(&f)
//!                                                │
//!                                                ▼
//!                                          Vec<Finding>  ──▶ broadcast<AppFinding>
//!                                                │
//!                                                ▼
//!                                          Dart Stream<AppFinding>
//! ```
//!
//! # Références
//!
//! * Cheatsheet pentest projet : `slide_cheatsheet.typ`
//! * Cf. CVE-2017-13077 (KRACK), CVE-2019-9506 (KNOB), Kamkar RollJam DEF CON 23.

pub mod schema;
pub mod matcher;
pub mod stateful;
pub mod finding;
pub mod engine;
pub mod remote;

pub use schema::*;
pub use matcher::*;
pub use stateful::*;
pub use finding::*;
pub use engine::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::schema::{Module, RfFrame};

    fn make_frame(module: Module, freq: u32, payload_hex: &str, ts_ms: u64) -> RfFrame {
        RfFrame {
            module,
            freq,
            rssi: -60,
            snr:  8,
            payload_hex: payload_hex.to_string(),
            ts_ms,
        }
    }

    #[test]
    fn default_pack_parses() {
        let engine = RulesEngine::from_default().expect("default pack must parse");
        let (_ver, count) = engine.pack_info();
        assert_eq!(count, 15, "default pack must contain exactly 15 rules");
    }

    #[test]
    fn em4102_triggers_rfid_lf_001() {
        let engine = RulesEngine::from_default().expect("default pack must parse");
        let frame = make_frame(Module::RfidLf, 125_000, "ABCDEF1234", 1_000);
        let findings = engine.evaluate(&frame);
        assert!(
            findings.iter().any(|f| f.rule_id == "RFID-LF-001"),
            "expected RFID-LF-001 to trigger, got: {:?}",
            findings.iter().map(|f| &f.rule_id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn three_identical_subghz_frames_trigger_001() {
        let engine = RulesEngine::from_default().expect("default pack must parse");
        let payload = "DEADBEEF";
        // 1st frame: pushes to buffer, not enough history yet.
        let _ = engine.evaluate(&make_frame(Module::Sx1262, 433_920_000, payload, 1));
        // 2nd frame: still only 2 identical.
        let _ = engine.evaluate(&make_frame(Module::Sx1262, 433_920_000, payload, 2));
        // 3rd frame: window of 3 → must trigger SUBGHZ-001.
        let findings = engine.evaluate(&make_frame(Module::Sx1262, 433_920_000, payload, 3));
        assert!(
            findings.iter().any(|f| f.rule_id == "SUBGHZ-001"),
            "expected SUBGHZ-001 to trigger on 3 identical frames, got: {:?}",
            findings.iter().map(|f| &f.rule_id).collect::<Vec<_>>()
        );
    }
}
