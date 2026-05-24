//! Helpers pour construire les requêtes host → device.
//!
//! Plutôt qu'une enum union des paramètres (lourde à maintenir et à
//! sérialiser avec `#[serde(flatten)]`), on construit les objets JSON
//! directement via `serde_json::json!` — chaque commande a son builder
//! qui injecte les bons champs.  L'ordre des champs ne compte pas pour le
//! parser firmware (RFC 8259 §4 stipule que les objets JSON sont des
//! collections non ordonnées).
//!
//! Le numéro de `seq` est généré au call site (cf. [`crate::client::Client`])
//! par un compteur monotone interne.

use crate::proto::schema::{Cmd, Module};
use serde_json::{Value, json};

/// Construit un objet JSON de requête à partir d'un verbe et de paramètres.
///
/// Les paramètres sont fusionnés au top-level — `params` doit être un
/// `json!({ ... })`.  Renvoie le `Value` prêt à être sérialisé.
#[must_use]
pub fn build(cmd: Cmd, seq: u64, params: Value) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("cmd".into(), serde_json::to_value(cmd).expect("Cmd serializes"));
    obj.insert("seq".into(), Value::from(seq));
    if let Value::Object(m) = params {
        for (k, v) in m {
            obj.insert(k, v);
        }
    }
    Value::Object(obj)
}

/// `{"cmd":"HANDSHAKE","seq":N,"proto_version":1}`
#[must_use]
pub fn handshake(seq: u64) -> Value {
    build(Cmd::Handshake, seq, json!({ "proto_version": super::schema::PROTO_VERSION }))
}

/// `{"cmd":"INJECT","seq":N,"module":...,"freq":...,"rssi":...,"snr":...,"payload_hex":"..."}`
#[must_use]
pub fn inject(seq: u64, module: Module, freq: u32, rssi: i16, snr: i8, payload_hex: &str) -> Value {
    build(Cmd::Inject, seq, json!({
        "module": module,
        "freq":   freq,
        "rssi":   rssi,
        "snr":    snr,
        "payload_hex": payload_hex,
    }))
}

/// `{"cmd":"START_LISTEN","seq":N,"module":...,"freq":...}`
#[must_use]
pub fn start_listen(seq: u64, module: Module, freq: u32) -> Value {
    build(Cmd::StartListen, seq, json!({ "module": module, "freq": freq }))
}

/// `{"cmd":"STOP","seq":N,"module":...}`
#[must_use]
pub fn stop(seq: u64, module: Module) -> Value {
    build(Cmd::Stop, seq, json!({ "module": module }))
}

/// `{"cmd":"QUERY_FRAMES","seq":N[,"module":...][,"max":...]}`
#[must_use]
pub fn query_frames(seq: u64, module: Option<Module>, max: Option<u64>) -> Value {
    let mut p = serde_json::Map::new();
    if let Some(m) = module { p.insert("module".into(), serde_json::to_value(m).unwrap()); }
    if let Some(n) = max    { p.insert("max".into(), Value::from(n)); }
    build(Cmd::QueryFrames, seq, Value::Object(p))
}

/// `{"cmd":"TX","seq":N,"module":...,"freq":...,"payload_hex":"..."}`
#[must_use]
pub fn tx(seq: u64, module: Module, freq: u32, payload_hex: &str) -> Value {
    build(Cmd::Tx, seq, json!({ "module": module, "freq": freq, "payload_hex": payload_hex }))
}

/// `{"cmd":"STATUS","seq":N}`
#[must_use]
pub fn status(seq: u64) -> Value {
    build(Cmd::Status, seq, json!({}))
}

/// `{"cmd":"RESET","seq":N}`
#[must_use]
pub fn reset(seq: u64) -> Value {
    build(Cmd::Reset, seq, json!({}))
}
