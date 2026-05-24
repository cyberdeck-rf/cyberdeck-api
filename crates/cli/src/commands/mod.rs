//! Sous-commandes de la CLI.

pub mod status;
pub mod scan;
pub mod inject;
pub mod list_ports;

use anyhow::Result;
use cyberdeck_api::{Client, SerialTransport, Transport, TcpTransport};

/// Choix de transport résolu depuis les flags `--port` / `--tcp`.
///
/// On utilise un `Box<dyn Transport>` côté Client car les deux types diffèrent
/// (SerialTransport vs TcpTransport).  L'overhead d'un appel vtable par
/// `read_line` / `write_line` est négligeable face à la latence USB/TCP.
#[derive(Debug, Clone)]
pub enum Target {
    /// USB-CDC via tokio-serial.  `None` = auto-détection (VID 0x0483).
    Serial { port: Option<String> },
    /// TCP vers cyberdeck-emu.  `addr` = `host:port`.
    Tcp { addr: String },
}

impl Target {
    /// Résout les flags CLI.  Priorité : `--tcp` > `--port` > auto-détection.
    pub fn from_flags(port: Option<&str>, tcp: Option<&str>) -> Self {
        match (port, tcp) {
            (_, Some(addr)) => Target::Tcp { addr: addr.to_string() },
            (Some(p), None) => Target::Serial { port: Some(p.to_string()) },
            (None, None)    => Target::Serial { port: None },
        }
    }
}

/// Ouvre un client async vers la cible.  La factorisation passe par un
/// `Box<dyn Transport>` qui homogénéise les deux implémentations.
pub async fn open_client(target: &Target) -> Result<Client<Box<dyn Transport>>> {
    let transport: Box<dyn Transport> = match target {
        Target::Serial { port: Some(p) } => Box::new(SerialTransport::open(p)?),
        Target::Serial { port: None }    => Box::new(SerialTransport::auto_detect()?),
        Target::Tcp    { addr }           => Box::new(TcpTransport::connect(addr).await?),
    };
    Ok(Client::new(transport))
}
