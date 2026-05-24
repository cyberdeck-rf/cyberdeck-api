//! Façade haut niveau — [`Client`] expose des méthodes asynchrones par
//! commande du protocole.
//!
//! # Pattern d'usage
//!
//! ```no_run
//! use cyberdeck_api::{Client, SerialTransport, Module, Result};
//! # async fn run() -> Result<()> {
//! let mut client = Client::new(SerialTransport::auto_detect()?);
//! let caps = client.handshake().await?;
//! client.start_listen(Module::Sx1262, 868_100_000).await?;
//! while let Some(frame) = client.next_frame().await? {
//!     println!("{frame:?}");
//! }
//! # Ok(()) }
//! ```
//!
//! # Modèle de session
//!
//! Le client maintient un compteur `seq` monotone. Chaque commande sortante
//! reçoit un `seq` unique ; on l'utilise pour matcher la réponse ACK/NACK.
//! Les messages async (FRAME, LOG) sont collectés dans une file interne et
//! restitués via [`Client::next_frame`] (filtré sur FRAME) ou
//! [`Client::next_message`] (tous types).

use crate::error::{Error, Result};
use crate::proto::request;
use crate::proto::schema::{Ack, Capabilities, Message, Module, RfFrame};
use crate::transport::Transport;
use std::collections::VecDeque;
use std::time::Duration;
use tokio::time::timeout;

/// Default timeout pour une réponse synchrone (ACK / NACK / CAPABILITIES).
/// Le firmware répond typiquement en < 10 ms ; 2 s laisse de la marge pour
/// un USB endormi qui se réveille.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

/// Client async vers un firmware cyberdeck.
pub struct Client<T: Transport> {
    transport: T,
    next_seq:  u64,
    /// File des messages reçus mais pas encore consommés. Sert quand on
    /// reçoit un FRAME alors qu'on attendait un ACK : on garde le FRAME pour
    /// le prochain `next_frame()` et on continue à pomper pour l'ACK.
    pending:   VecDeque<Message>,
}

impl<T: Transport> Client<T> {
    /// Construit un nouveau client. Le `seq` démarre à 1 (0 est réservé aux
    /// messages async émis par le firmware sans corrélation côté hôte).
    pub fn new(transport: T) -> Self {
        Self { transport, next_seq: 1, pending: VecDeque::new() }
    }

    /// Numéro de séquence pour la prochaine commande sortante.
    fn alloc_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }

    /// Émet une requête JSON et attend la première réponse correspondant à
    /// `expected_seq`. Tous les messages async (FRAME, LOG) reçus entre-temps
    /// sont stockés dans `pending`.
    async fn round_trip(
        &mut self,
        value: serde_json::Value,
        expected_seq: u64,
    ) -> Result<Message> {
        let line = crate::proto::codec::encode_line(&value)?;
        self.transport.write_line(&line).await?;

        timeout(DEFAULT_TIMEOUT, async {
            loop {
                let raw = self.transport.read_line().await?;
                if raw.is_empty() { continue; }
                let msg = crate::proto::codec::parse_line(&raw)?;
                match &msg {
                    // Messages corrélés au seq sortant.
                    Message::Capabilities(c)  if c.seq == expected_seq => return Ok(msg),
                    Message::Ack(a)            if a.seq == expected_seq => return Ok(msg),
                    Message::Nack(n)           if n.seq == expected_seq => return Ok(msg),
                    Message::TxDone(t)         if t.seq == expected_seq => return Ok(msg),
                    // Tout le reste : on bufferise.
                    _ => self.pending.push_back(msg),
                }
            }
        }).await
        .map_err(|_| Error::Timeout { seq: expected_seq })?
    }

    /// Envoie HANDSHAKE et renvoie le payload [`Capabilities`].
    pub async fn handshake(&mut self) -> Result<Capabilities> {
        let seq = self.alloc_seq();
        match self.round_trip(request::handshake(seq), seq).await? {
            Message::Capabilities(c) => Ok(c),
            Message::Nack(n) => Err(Error::Nack { reason: n.reason }),
            other => Err(Error::UnexpectedMessage(format!("{other:?}"))),
        }
    }

    /// Arme un module en écoute sur une fréquence donnée.  Renvoie l'`Ack`.
    pub async fn start_listen(&mut self, module: Module, freq: u32) -> Result<Ack> {
        let seq = self.alloc_seq();
        self.expect_ack(request::start_listen(seq, module, freq), seq).await
    }

    /// Désarme un module.
    pub async fn stop(&mut self, module: Module) -> Result<Ack> {
        let seq = self.alloc_seq();
        self.expect_ack(request::stop(seq, module), seq).await
    }

    /// Injecte une trame fake (utile en mode loopback firmware ou pour tests).
    pub async fn inject(&mut self, frame: &RfFrame) -> Result<Ack> {
        let seq = self.alloc_seq();
        let req = request::inject(
            seq, frame.module, frame.freq, frame.rssi, frame.snr, &frame.payload_hex,
        );
        self.expect_ack(req, seq).await
    }

    /// Drain explicite côté firmware (renvoie potentiellement plusieurs FRAME
    /// puis un ACK).  Cette méthode renvoie les FRAME reçues entre l'émission
    /// et la fin de l'ACK.
    pub async fn query_frames(&mut self, module: Option<Module>, max: Option<u64>) -> Result<Vec<RfFrame>> {
        let seq = self.alloc_seq();
        let line = crate::proto::codec::encode_line(&request::query_frames(seq, module, max))?;
        self.transport.write_line(&line).await?;
        let mut frames = Vec::new();
        timeout(DEFAULT_TIMEOUT, async {
            loop {
                let raw = self.transport.read_line().await?;
                if raw.is_empty() { continue; }
                let msg = crate::proto::codec::parse_line(&raw)?;
                match msg {
                    Message::Frame(f) => frames.push(f),
                    Message::Ack(a) if a.seq == seq => return Ok::<_, Error>(()),
                    Message::Nack(n) if n.seq == seq => return Err(Error::Nack { reason: n.reason }),
                    other => self.pending.push_back(other),
                }
            }
        }).await
        .map_err(|_| Error::Timeout { seq })??;
        Ok(frames)
    }

    /// Émission RF (mock) — renvoie l'`Ack` (et reçoit un TX_DONE en async).
    pub async fn tx(&mut self, module: Module, freq: u32, payload_hex: &str) -> Result<Ack> {
        let seq = self.alloc_seq();
        self.expect_ack(request::tx(seq, module, freq, payload_hex), seq).await
    }

    /// `STATUS` → `Ack` (le firmware met l'uptime dans le champ ack).
    pub async fn status(&mut self) -> Result<Ack> {
        let seq = self.alloc_seq();
        self.expect_ack(request::status(seq), seq).await
    }

    /// Reset / clear des queues côté firmware.
    pub async fn reset(&mut self) -> Result<Ack> {
        let seq = self.alloc_seq();
        self.expect_ack(request::reset(seq), seq).await
    }

    // ---- Streaming des messages async ----------------------------------

    /// Lit le prochain FRAME (depuis la file bufferisée ou directement
    /// depuis le transport). Renvoie `None` si EOF transport.
    pub async fn next_frame(&mut self) -> Result<Option<RfFrame>> {
        // Vide d'abord la file des messages déjà reçus.
        while let Some(m) = self.pending.pop_front() {
            if let Message::Frame(f) = m {
                return Ok(Some(f));
            }
        }
        // Sinon pomper le transport.
        loop {
            let raw = self.transport.read_line().await?;
            if raw.is_empty() { continue; }
            match crate::proto::codec::parse_line(&raw)? {
                Message::Frame(f) => return Ok(Some(f)),
                other => self.pending.push_back(other),
            }
        }
    }

    /// Variante : renvoie le prochain message quel que soit son type (FRAME,
    /// ACK orphelin, TX_DONE…).
    pub async fn next_message(&mut self) -> Result<Message> {
        if let Some(m) = self.pending.pop_front() { return Ok(m); }
        let raw = self.transport.read_line().await?;
        crate::proto::codec::parse_line(&raw)
    }

    // ---- helpers internes ----------------------------------------------

    async fn expect_ack(&mut self, value: serde_json::Value, seq: u64) -> Result<Ack> {
        match self.round_trip(value, seq).await? {
            Message::Ack(a) => Ok(a),
            Message::Nack(n) => Err(Error::Nack { reason: n.reason }),
            other => Err(Error::UnexpectedMessage(format!("{other:?}"))),
        }
    }
}

impl<T: Transport> Client<T> {
    /// Indication non-fiable du nombre de messages async en attente côté
    /// hôte (utile pour les UI qui veulent indiquer "x frames pending").
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}
