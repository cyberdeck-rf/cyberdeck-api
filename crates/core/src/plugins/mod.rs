//! Plugins par bande RF.
//!
//! Chaque plugin implémente [`Plugin`] et apporte la connaissance
//! protocole/décodage spécifique à un transceiver. En v0 les plugins sont
//! presque vides (`decode()` renvoie le payload brut) — ils servent surtout
//! d'**emplacement réservé** pour la phase 2 où on branchera des décodeurs
//! réels (Mifare Crypto1, EM4102, LoRa CR/SF, etc.).
//!
//! La symétrie 1:1 avec [`crate::Module`] est volontaire : un plugin par
//! module RF, indexable par le même enum.

pub mod sub_ghz;
pub mod nfc;
pub mod rfid_lf;
pub mod wifi_ble;
pub mod lora_2g4;

use crate::proto::schema::{Module, RfFrame};

/// Une trame décodée par un plugin — la forme `text` est destinée à
/// l'affichage utilisateur.  La forme `structured` (Value) sert au
/// pipeline UI / SQLite pour reconstruire des vues riches.
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    /// Description courte de la trame (e.g. `"Mifare Classic UID=04AA"`).
    pub summary: String,
    /// Détails structurés en JSON (libre, propre à chaque plugin).
    pub details: serde_json::Value,
}

/// Trait commun aux 5 plugins.  Les méthodes par défaut renvoient une
/// décode triviale ; un plugin spécifique peut overrider pour ajouter
/// son intelligence.
pub trait Plugin: Send + Sync {
    /// Module RF géré par ce plugin.
    fn module(&self) -> Module;

    /// Nom humain pour l'UI (e.g. `"Sub-GHz LoRa/FSK"`).
    fn human_name(&self) -> &'static str;

    /// Liste des attaques connues pour cette bande (référence cheatsheet).
    fn known_attacks(&self) -> &'static [&'static str] { &[] }

    /// Décodage d'une trame reçue.  Implémentation par défaut : renvoie
    /// juste le hex en `summary`.
    fn decode(&self, frame: &RfFrame) -> DecodedFrame {
        DecodedFrame {
            summary: format!("{} bytes hex: {}", frame.payload_hex.len() / 2, frame.payload_hex),
            details: serde_json::json!({
                "freq_hz": frame.freq,
                "rssi_dbm": frame.rssi,
                "snr_db": frame.snr,
            }),
        }
    }
}

/// Renvoie le plugin associé à un module donné.
#[must_use]
pub fn for_module(m: Module) -> &'static dyn Plugin {
    match m {
        Module::Sx1262    => &sub_ghz::SubGhz,
        Module::St25r3916 => &nfc::Nfc,
        Module::Esp32C6   => &wifi_ble::WifiBle,
        Module::Sx1280    => &lora_2g4::Lora2g4,
        Module::RfidLf    => &rfid_lf::RfidLf,
    }
}
