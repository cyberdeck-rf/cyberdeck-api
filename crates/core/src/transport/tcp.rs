//! Transport TCP — utilisé pour parler à l'émulateur `cyberdeck-emu`.
//!
//! Le firmware réel s'expose en USB-CDC (cf. [`super::serial`]), mais en
//! phase développement on lance un conteneur Docker qui simule la carte et
//! parle le même NDJSON sur TCP.  Cela permet de développer la mobile-app
//! et de faire des démos sans matériel.
//!
//! # Format
//!
//! Strictement identique au transport série : un objet JSON par ligne,
//! séparateur `\n`, max 1024 octets.  Le wire-protocol est byte-pour-byte
//! le même, seul le médium change.
//!
//! # Exemple
//!
//! ```no_run
//! use cyberdeck_api::{Client, transport::tcp::TcpTransport, Result};
//!
//! # async fn run() -> Result<()> {
//! let t = TcpTransport::connect("127.0.0.1:17017").await?;
//! let mut client = Client::new(t);
//! let caps = client.handshake().await?;
//! println!("{caps:#?}");
//! # Ok(()) }
//! ```

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;

use crate::error::{Error, Result};
use crate::transport::Transport;

/// Transport TCP/NDJSON.
pub struct TcpTransport {
    reader: BufReader<tokio::io::ReadHalf<TcpStream>>,
    writer: BufWriter<tokio::io::WriteHalf<TcpStream>>,
}

impl TcpTransport {
    /// Ouvre une connexion TCP vers `addr` (`host:port`).  Désactive
    /// Nagle (TCP_NODELAY) pour minimiser la latence — on échange des
    /// petits paquets JSON, agréger les écritures dégraderait l'UX
    /// temps réel.
    pub async fn connect(addr: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr)
            .await
            .map_err(|e| Error::Transport(format!("connect {addr}: {e}")))?;
        stream
            .set_nodelay(true)
            .map_err(|e| Error::Transport(format!("set_nodelay: {e}")))?;
        let (rh, wh) = tokio::io::split(stream);
        Ok(Self {
            reader: BufReader::with_capacity(2048, rh),
            writer: BufWriter::with_capacity(2048, wh),
        })
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn read_line(&mut self) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(256);
        let n = self.reader.read_until(b'\n', &mut buf).await?;
        if n == 0 {
            return Err(Error::Transport("EOF on TCP socket".into()));
        }
        while matches!(buf.last(), Some(b'\n' | b'\r')) {
            buf.pop();
        }
        Ok(buf)
    }

    async fn write_line(&mut self, line: &[u8]) -> Result<()> {
        self.writer.write_all(line).await?;
        if !line.ends_with(b"\n") {
            self.writer.write_all(b"\n").await?;
        }
        self.writer.flush().await?;
        Ok(())
    }
}
