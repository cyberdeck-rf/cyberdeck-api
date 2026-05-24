//! Couche protocole JSON v1.
//!
//! Cette couche est en **miroir strict** du firmware :
//! [`cyberdeck/src/proto/schema.hpp`](../../cyberdeck/src/proto/schema.hpp) et
//! [`commands.hpp`](../../cyberdeck/src/proto/commands.hpp). Tout ajout d'un
//! verbe ou d'une clé doit se faire en parallèle dans les deux repos —
//! voir `docs/protocol-v1.md` pour le contrat de référence.
//!
//! Le protocole est encadré en **NDJSON** (Newline-Delimited JSON) :
//! un objet JSON par ligne, terminator `\n`, max 1024 octets/ligne.
//! Référence : <https://github.com/ndjson/ndjson-spec>.

pub mod schema;
pub mod request;
pub mod codec;

pub use schema::*;
pub use codec::NdjsonCodec;
