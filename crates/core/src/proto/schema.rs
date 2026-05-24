//! Types sérialisables au format JSON v1.
//!
//! # Mapping avec le firmware
//!
//! | Côté Rust                | Côté firmware (schema.hpp)        |
//! |--------------------------|-----------------------------------|
//! | [`Module::Sx1262`]       | `radio::Module::Sx1262`           |
//! | [`Cmd::Handshake`]       | `schema::V_HANDSHAKE`             |
//! | [`RfFrame.module`]       | `radio::RfFrame::module`          |
//! | etc.                     |                                   |
//!
//! Les noms textuels qui transitent sur le bus :
//! * `Module` → `"sx1262"`, `"st25r3916"`, … (snake_case)
//! * `Cmd`    → `"HANDSHAKE"`, `"START_LISTEN"`, … (SCREAMING_SNAKE_CASE)
//!
//! Côté hôte les chaînes circulent **en clair** — l'obfuscation des littéraux
//! est une protection firmware-only (cf. `cyberdeck/src/obf/encrypted_string.hpp`).

use serde::{Deserialize, Serialize};

/// Version protocolaire. Bump quand un changement breaking est introduit.
pub const PROTO_VERSION: u32 = 1;

// ---- Capability bitmask (miroir de cyberdeck/src/radio/capabilities.hpp) ---
/// Le module peut recevoir des trames RF.
pub const CAP_RX:    u32 = 1 << 0;
/// Le module peut émettre des trames RF.
pub const CAP_TX:    u32 = 1 << 1;
/// Le module supporte la capture passive (sniff).
pub const CAP_SNIFF: u32 = 1 << 2;
/// Le module peut émuler une carte / badge (NFC, RFID LF).
pub const CAP_EMU:   u32 = 1 << 3;

// ---- Modules RF -------------------------------------------------------------
/// Identifiant d'un transceiver RF côté firmware.
///
/// L'ordre numérique correspond exactement à `radio::Module` côté firmware ;
/// il sert d'index dans la table de dispatch et dans le tableau renvoyé par
/// `CAPABILITIES`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
pub enum Module {
    /// Sub-GHz LoRa/FSK 433+868 MHz (Semtech SX1262).
    Sx1262,
    /// NFC 13.56 MHz reader/writer/emulator (ST ST25R3916).
    St25r3916,
    /// WiFi 6 / BLE 5 / Zigbee / Thread / Matter (Espressif ESP32-C6).
    Esp32C6,
    /// LoRa 2.4 GHz / ELRS (Semtech SX1280).
    Sx1280,
    /// 125 kHz RFID LF discret (PWM + MOSFET + comparateur).
    RfidLf,
}

impl Module {
    /// Tous les modules dans l'ordre canonique de la table firmware.
    pub const ALL: [Module; 5] = [
        Module::Sx1262,
        Module::St25r3916,
        Module::Esp32C6,
        Module::Sx1280,
        Module::RfidLf,
    ];

    /// Renvoie la string de protocole (matche la sérialisation serde).
    #[must_use]
    pub fn as_protocol_str(&self) -> &'static str {
        match self {
            Module::Sx1262    => "sx1262",
            Module::St25r3916 => "st25r3916",
            Module::Esp32C6   => "esp32c6",
            Module::Sx1280    => "sx1280",
            Module::RfidLf    => "rfid_lf",
        }
    }
}

// ---- Verbes protocole -------------------------------------------------------
/// Verbes utilisés sur le champ `"cmd"` du JSON v1.
///
/// La sérialisation produit la forme SCREAMING_SNAKE_CASE attendue par le
/// firmware (cf. `schema::V_*` côté firmware).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Cmd {
    // Host → Device
    /// Demande de handshake — déclenche `CAPABILITIES` en retour.
    Handshake,
    /// Injection d'une trame RF fake dans la RX ring d'un module.
    Inject,
    /// Arme un module en écoute sur une fréquence.
    StartListen,
    /// Désarme un module.
    Stop,
    /// Drain de la RX ring (tous les modules ou un seul).
    QueryFrames,
    /// Émission RF (mock pour l'instant).
    Tx,
    /// État (uptime, queues, etc.).
    Status,
    /// Réinitialisation des queues et désarmement de tous les modules.
    Reset,

    // Device → Host
    /// Réponse au HANDSHAKE — liste les modules et capacités.
    Capabilities,
    /// Trame RF reçue (synchrone après INJECT, ou async via QUERY_FRAMES).
    Frame,
    /// Acquittement.
    Ack,
    /// Erreur — voir [`Nack::reason`] pour la cause.
    Nack,
    /// Confirmation d'émission.
    TxDone,
    /// Message de log (gated par compilation côté firmware).
    Log,
}

// ---- Réponses device → host -------------------------------------------------

/// Description d'un module dans la réponse CAPABILITIES.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    /// Index ordinal dans `radio::Module` (0..5).
    pub id:   u8,
    /// Nom canonique (matche [`Module::as_protocol_str`]).
    pub name: String,
    /// Bitmask des capacités (combinaison de `CAP_RX`, `CAP_TX`, …).
    pub caps: u32,
}

/// Réponse au handshake — annonce les modules et la version firmware.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    /// Numéro de séquence echo du HANDSHAKE.
    pub seq: u64,
    /// Version du protocole supportée (toujours `PROTO_VERSION` pour v1).
    pub proto_version: u32,
    /// Version sémantique du firmware.
    pub fw_version: String,
    /// Identifiant matériel court (ex. `"stm32f407vg-disco"`).
    pub hw: String,
    /// Liste des modules présents (mocks en v0, réels en v1).
    pub modules: Vec<ModuleInfo>,
}

/// Une trame RF reçue depuis un transceiver (mock ou réel).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfFrame {
    /// Module source.
    pub module: Module,
    /// Fréquence (Hz). Le firmware n'interprète pas cette valeur.
    pub freq: u32,
    /// RSSI en dBm (négatif pour les valeurs reçues, e.g. -72).
    pub rssi: i16,
    /// SNR en dB.
    pub snr: i8,
    /// Payload en hexadécimal majuscule (sans préfixe `0x`).
    pub payload_hex: String,
    /// Timestamp millisecondes (SysTick côté firmware).
    pub ts_ms: u64,
}

impl RfFrame {
    /// Décode `payload_hex` en `Vec<u8>`. Renvoie une erreur si non hexa
    /// valide ou si la longueur n'est pas paire.
    pub fn payload_bytes(&self) -> Result<Vec<u8>, crate::Error> {
        let s = &self.payload_hex;
        if !s.len().is_multiple_of(2) {
            return Err(crate::Error::BadHex("odd length".into()));
        }
        let mut out = Vec::with_capacity(s.len() / 2);
        for i in (0..s.len()).step_by(2) {
            let byte = u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|e| crate::Error::BadHex(e.to_string()))?;
            out.push(byte);
        }
        Ok(out)
    }

    /// Encode `bytes` en hex majuscule (compatible avec le firmware).
    #[must_use]
    pub fn encode_hex(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push(char::from_digit(u32::from(b >> 4), 16).unwrap().to_ascii_uppercase());
            out.push(char::from_digit(u32::from(b & 0x0F), 16).unwrap().to_ascii_uppercase());
        }
        out
    }
}

/// Acquittement (positif).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ack {
    /// Numéro de séquence echo de la commande d'origine.
    pub seq: u64,
    /// Uptime firmware en ms (présent uniquement sur la réponse à STATUS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_ms: Option<u64>,
}

/// Acquittement négatif — la commande a échoué.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nack {
    /// Numéro de séquence echo de la commande d'origine.
    pub seq: u64,
    /// Raison textuelle (matche `schema::R_*` côté firmware).
    pub reason: String,
}

/// Confirmation d'émission RF.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxDone {
    /// Numéro de séquence echo de la commande TX.
    pub seq: u64,
    /// Module ayant émis.
    pub module: Module,
    /// Timestamp millisecondes de l'émission.
    pub ts_ms: u64,
}

// ---- Enum unifiée des messages reçus depuis le firmware --------------------
//
// Le firmware émet toujours un objet avec un champ `cmd` qui désigne le type
// de message. `#[serde(tag = "cmd")]` exploite cette discriminante pour
// dispatcher automatiquement à la bonne variante.

/// Tout message qui peut être reçu du firmware.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Message {
    /// Voir [`Capabilities`].
    Capabilities(Capabilities),
    /// Voir [`RfFrame`].
    Frame(RfFrame),
    /// Voir [`Ack`].
    Ack(Ack),
    /// Voir [`Nack`].
    Nack(Nack),
    /// Voir [`TxDone`].
    TxDone(TxDone),
}
