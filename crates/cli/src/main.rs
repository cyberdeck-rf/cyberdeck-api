//! `cyberdeck-cli` — front en ligne de commande pour le firmware WraithRF.
//!
//! Le binaire encapsule [`cyberdeck_api`] derrière une interface `clap` :
//! ```text
//!   cyberdeck-cli [--port PORT] <subcommand>
//! ```
//!
//! Subcommands :
//!   * `status` — handshake puis affiche les capacités du firmware
//!   * `scan <module> <freq>` — arme un module et stream les FRAME
//!   * `inject <module> <freq> --payload <hex>` — pousse une trame fake
//!   * `list-ports` — énumère les ports série visibles (debug)
//!
//! Auto-détection du port via le VID STMicroelectronics (`0x0483`) — voir
//! [`SerialTransport::auto_detect`].

#![warn(rust_2018_idioms, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};
use cyberdeck_api::Module;

/// CLI principale.  Deux transports possibles :
///   - `--port <path>`  : USB-CDC sur le port série (default si aucun flag).
///   - `--tcp <host:port>` : TCP vers l'émulateur cyberdeck-emu (dockerisé).
/// Les deux sont mutuellement exclusifs (`conflicts_with = "tcp"` au niveau
/// clap).  Sans flag, on tente l'auto-détection USB-CDC.
#[derive(Parser, Debug)]
#[command(
    name = "cyberdeck-cli",
    about = "Host-side CLI for the WraithRF RF Cyberdeck firmware (serial or TCP emu)",
    version,
)]
struct Cli {
    /// Override l'auto-détection USB-CDC. Ex: `/dev/cu.usbmodem14102`,
    /// `COM5`, `/dev/ttyACM0`.
    #[arg(long, global = true, conflicts_with = "tcp")]
    port: Option<String>,

    /// Connexion TCP vers l'émulateur (`docker compose up -d` côté
    /// cyberdeck-api/).  Format `host:port`, par défaut `127.0.0.1:17017`.
    #[arg(long, global = true, conflicts_with = "port")]
    tcp: Option<String>,

    /// Active les logs verbeux (RUST_LOG=debug équivalent).
    #[arg(long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Handshake et affiche les capacités firmware.
    Status,

    /// Arme un module en écoute et stream les FRAME en JSON pretty.
    Scan {
        /// Module RF cible (sx1262, st25r3916, ...).
        module: Module,
        /// Fréquence en Hz (e.g. 868100000 pour 868.1 MHz).
        freq: u32,
        /// Durée en secondes ; 0 = infini (Ctrl-C pour sortir).
        #[arg(long, default_value_t = 0)]
        duration: u64,
    },

    /// Pousse une trame fake dans la RX ring du module (mode loopback).
    Inject {
        /// Module RF cible.
        module: Module,
        /// Fréquence en Hz.
        freq: u32,
        /// Payload hexa (sans `0x`, longueur paire).
        #[arg(long)]
        payload: String,
        /// RSSI à simuler.
        #[arg(long, default_value_t = -60)]
        rssi: i16,
        /// SNR à simuler.
        #[arg(long, default_value_t = 0)]
        snr: i8,
    },

    /// Liste tous les ports série visibles avec leur VID/produit.
    ListPorts,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    let target = commands::Target::from_flags(cli.port.as_deref(), cli.tcp.as_deref());

    match cli.cmd {
        Command::Status                                  => commands::status::run(&target).await,
        Command::Scan { module, freq, duration }         => commands::scan::run(&target, module, freq, duration).await,
        Command::Inject { module, freq, payload, rssi, snr }
            => commands::inject::run(&target, module, freq, &payload, rssi, snr).await,
        Command::ListPorts                                => commands::list_ports::run(),
    }
}

fn init_tracing(verbose: bool) {
    use tracing_subscriber::EnvFilter;
    let filter = if verbose {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"))
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
}
