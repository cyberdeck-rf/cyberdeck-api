//! Transports — abstraction du canal physique vers le firmware (ou son
//! émulation).
//!
//! Implémentations livrées :
//! * [`serial::SerialTransport`] (feature `serial`, on par défaut) — USB-CDC
//!   en NDJSON via [tokio-serial].  C'est le mode "vrai matériel" :
//!   `/dev/cu.usbmodem*` (macOS), `/dev/ttyACMx` (Linux), `COMx` (Windows).
//! * [`tcp::TcpTransport`] (toujours dispo) — TCP en NDJSON.  C'est le mode
//!   "émulateur dockerisé" : pointe vers `cyberdeck-emu` (cf.
//!   `cyberdeck-api/crates/emu/`) qui écoute par défaut sur le port 17017.
//!
//! Le binaire `cyberdeck-emu` désactive la feature `serial` pour éviter la
//! dépendance libudev/libusb (image Docker plus mince).
//!
//! [tokio-serial]: https://docs.rs/tokio-serial

#[cfg(feature = "serial")]
pub mod serial;
pub mod tcp;

use async_trait::async_trait;
use crate::error::Result;

/// Canal NDJSON bidirectionnel vers le firmware (ou émulateur).
///
/// L'implémenteur garantit que chaque appel à `read_line` renvoie un buffer
/// contenant UNE seule ligne complète (sans le `\n` terminal), et que
/// chaque appel à `write_line` envoie une ligne complète (ajoute le `\n`
/// si nécessaire).
#[async_trait]
pub trait Transport: Send {
    /// Lit la prochaine ligne NDJSON. Bloque jusqu'à dispo.
    async fn read_line(&mut self) -> Result<Vec<u8>>;

    /// Écrit une ligne NDJSON (ajoute `\n` automatiquement si absent).
    async fn write_line(&mut self, line: &[u8]) -> Result<()>;
}

// Blanket impl : un `Box<dyn Transport>` est lui-même un `Transport`.  Permet
// au consommateur (Client, CLI, mobile-app) d'unifier plusieurs transports
// concrets derrière une seule abstraction sans réécrire de wrapper.
#[async_trait]
impl<T: Transport + ?Sized> Transport for Box<T> {
    async fn read_line(&mut self) -> Result<Vec<u8>> {
        (**self).read_line().await
    }
    async fn write_line(&mut self, line: &[u8]) -> Result<()> {
        (**self).write_line(line).await
    }
}
