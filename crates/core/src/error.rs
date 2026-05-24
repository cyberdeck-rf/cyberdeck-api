//! Types d'erreur unifiés via [`thiserror`].
//!
//! Toutes les fonctions publiques de la crate renvoient un [`Result<T>`]
//! aliasé sur [`Error`].  La propagation se fait avec `?` sans conversion
//! manuelle grâce aux variantes `#[from]`.

use std::io;

/// Alias pratique — `Result<T> = std::result::Result<T, Error>`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Erreurs possibles côté hôte.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Échec d'I/O sur le port série (déconnexion, permission, etc.).
    #[error("serial I/O: {0}")]
    SerialIo(#[from] io::Error),

    /// Erreur de parsing JSON sur une trame reçue ou de sérialisation à
    /// l'émission. La cause inclut typiquement le numéro de ligne.
    #[error("JSON codec: {0}")]
    Json(#[from] serde_json::Error),

    /// Le firmware a renvoyé un message NACK avec une raison textuelle
    /// (issu de `schema::R_*` côté firmware).
    #[error("device NACK: {reason}")]
    Nack {
        /// Raison renvoyée par le firmware (par ex. `"bad_module"`).
        reason: String,
    },

    /// Pas de réponse du firmware dans la fenêtre attendue.
    #[error("timeout waiting for response (seq={seq})")]
    Timeout {
        /// Numéro de séquence attendu.
        seq: u64,
    },

    /// Réception d'un message inattendu avant la réponse attendue.
    /// Inclus pour debug — en pratique on les ignore et on continue à pomper.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),

    /// Aucun port série compatible n'a été trouvé en auto-détection.
    #[error("no compatible device found (looking for STMicroelectronics VID 0x0483)")]
    NoDevice,

    /// Erreur générique de la couche transport (au-delà des I/O brutes).
    #[error("transport: {0}")]
    Transport(String),

    /// Hexadécimal mal formé dans un champ `payload_hex`.
    #[error("invalid hex payload: {0}")]
    BadHex(String),
}
