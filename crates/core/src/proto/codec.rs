//! Framing NDJSON : un objet JSON par ligne terminée par `\n`.
//!
//! On expose deux helpers :
//!   * Sérialisation : [`encode_line`] sérialise un `serde_json::Value`
//!     et ajoute le `\n` final.
//!   * Décodage : [`parse_line`] parse une slice et renvoie un
//!     [`crate::proto::Message`].
//!
//! Pour le streaming par ligne, on délègue à [`tokio_util::codec::LinesCodec`]
//! côté transport (cf. `transport/serial.rs`) — ce module ne fait que la
//! sérialisation/désérialisation d'une ligne unique.

use crate::error::{Error, Result};
use crate::proto::schema::Message;

/// Sérialise un `Value` en NDJSON (terminator `\n` inclus).  Taille max
/// imposée à 1024 octets par alignement avec le firmware (`LINE_OUT_CAP`).
pub fn encode_line(v: &serde_json::Value) -> Result<Vec<u8>> {
    let mut out = serde_json::to_vec(v)?;
    if out.len() + 1 > 1024 {
        return Err(Error::Transport(format!(
            "line too long ({} bytes, max 1024)",
            out.len()
        )));
    }
    out.push(b'\n');
    Ok(out)
}

/// Parse une ligne NDJSON en [`Message`].  La ligne ne doit PAS contenir
/// le `\n` terminal (le decoder le strip).
pub fn parse_line(line: &[u8]) -> Result<Message> {
    serde_json::from_slice(line).map_err(Error::from)
}

/// Marqueur de type pour les decoders tokio (pour usage futur).
#[derive(Debug, Default)]
pub struct NdjsonCodec;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::schema::{Cmd, Module};
    use serde_json::json;

    #[test]
    fn encode_then_parse_roundtrip_capabilities() {
        let line = serde_json::to_vec(&json!({
            "cmd": "CAPABILITIES",
            "seq": 1,
            "proto_version": 1,
            "fw_version": "0.1.0",
            "hw": "stm32f407vg-disco",
            "modules": [
                { "id": 0, "name": "sx1262",    "caps": 7 },
                { "id": 1, "name": "st25r3916", "caps": 15 },
            ]
        })).unwrap();
        let msg = parse_line(&line).expect("parses");
        match msg {
            Message::Capabilities(c) => {
                assert_eq!(c.seq, 1);
                assert_eq!(c.proto_version, 1);
                assert_eq!(c.modules.len(), 2);
                assert_eq!(c.modules[0].name, "sx1262");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_frame_roundtrip() {
        let line = br#"{"cmd":"FRAME","module":"sx1262","freq":868100000,"rssi":-72,"snr":-3,"payload_hex":"DEADBEEF","ts_ms":1234}"#;
        let m = parse_line(line).expect("parses");
        match m {
            Message::Frame(f) => {
                assert_eq!(f.module, Module::Sx1262);
                assert_eq!(f.freq, 868_100_000);
                assert_eq!(f.payload_hex, "DEADBEEF");
                assert_eq!(f.payload_bytes().unwrap(), vec![0xDE, 0xAD, 0xBE, 0xEF]);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn build_handshake() {
        let v = crate::proto::request::handshake(42);
        assert_eq!(v["cmd"], "HANDSHAKE");
        assert_eq!(v["seq"], 42);
        assert_eq!(v["proto_version"], 1);
    }

    #[test]
    fn cmd_serializes_to_screaming_snake() {
        let s = serde_json::to_string(&Cmd::QueryFrames).unwrap();
        assert_eq!(s, "\"QUERY_FRAMES\"");
    }
}
