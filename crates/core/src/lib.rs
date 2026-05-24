//! `cyberdeck-api` — bibliothèque hôte du système RF Cyberdeck.
//!
//! Cette crate expose le **client** Rust qui communique avec le firmware
//! cyberdeck (cf. repo voisin `cyberdeck/`) via la liaison USB-CDC, en NDJSON
//! conforme au protocole JSON v1 défini dans
//! `cyberdeck/src/proto/schema.hpp` + `commands.hpp`.
//!
//! # Architecture
//!
//! ```text
//!   +----------------------+
//!   | cyberdeck-cli (bin)  |    cyberdeck-cli status / scan / ...
//!   +----------+-----------+
//!              | dyn dispatch via [`Client`]
//!   +----------v-----------+
//!   |     [`Client<T>`]    |    façade async high-level
//!   +----------+-----------+
//!              | trait
//!   +----------v-----------+
//!   |     [`Transport`]    |    SerialTransport (CDC) | UsbBulkTransport (v1)
//!   +----------+-----------+
//!              | NDJSON
//!   +----------v-----------+
//!   |   firmware over USB  |
//!   +----------------------+
//! ```
//!
//! # Quickstart
//!
//! ```no_run
//! use cyberdeck_api::{Client, SerialTransport, Result};
//!
//! # async fn run() -> Result<()> {
//! let t = SerialTransport::auto_detect()?;
//! let mut client = Client::new(t);
//! let caps = client.handshake().await?;
//! println!("{caps:#?}");
//! # Ok(())
//! # }
//! ```
//!
//! # Threading
//!
//! `Client` n'est pas `Sync` — on suppose UN consommateur par carte. Pour
//! plusieurs consommateurs (UI + log writer), créez un `mpsc` qui broadcasti
//! les messages issus de `Client::messages()`.

// Lints projet — rigoureux mais raisonnable.
#![warn(rust_2018_idioms, missing_docs, clippy::pedantic)]
#![allow(clippy::module_name_repetitions, clippy::missing_errors_doc)]

pub mod error;
pub mod proto;
pub mod transport;
pub mod client;
pub mod plugins;
pub mod rules;
pub mod pentest;

// Ré-exports du moteur de règles pentest (cf. `rules/mod.rs`).
pub use crate::rules::{
    Finding, MatchSpec, Matcher, RollingBuffer, Rule, RulePack, RulesEngine,
    Severity, StatefulOp,
};

pub use crate::error::{Error, Result};
pub use crate::client::Client;
pub use crate::transport::Transport;
pub use crate::transport::tcp::TcpTransport;
#[cfg(feature = "serial")]
pub use crate::transport::serial::SerialTransport;
pub use crate::proto::schema::{
    Cmd, Module, RfFrame, Capabilities, ModuleInfo, Ack, Nack, Message,
    CAP_RX, CAP_TX, CAP_SNIFF, CAP_EMU, PROTO_VERSION,
};
