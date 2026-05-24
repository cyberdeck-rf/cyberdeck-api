//! Transport USB-CDC via [`tokio_serial`].
//!
//! Le firmware (TARGET=f407) expose un device USB CDC ACM qui apparaît sous
//! macOS comme `/dev/cu.usbmodem*`, sous Linux comme `/dev/ttyACMx`, et
//! sous Windows comme `COMx`.  La crate `serialport` permet l'énumération
//! cross-platform.
//!
//! # Auto-détection
//!
//! [`SerialTransport::auto_detect`] énumère les ports série et sélectionne le
//! premier qui annonce le VID `0x0483` (STMicroelectronics).  Si plusieurs
//! cartes STM sont branchées (ST-Link + cyberdeck), on filtre aussi sur le
//! `Product` string contenant `"RF Cyberdeck"`.

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio_serial::{SerialPortBuilderExt, SerialStream};

use crate::error::{Error, Result};
use crate::transport::Transport;

/// VID STMicroelectronics (toutes les cartes ST l'utilisent — ST-Link, CDC,
/// DFU, etc.). Cf. https://devicehunt.com/view/type/usb/vendor/0483
const ST_VID: u16 = 0x0483;

/// Baud rate côté serial host. Pour USB-CDC la valeur est purement nominale
/// (l'USB transporte par paquets de 64 octets), mais certains drivers refusent
/// un baud absurde.  115200 est universellement accepté.
const BAUD: u32 = 115_200;

/// Transport USB-CDC.
///
/// Internement on garde un `BufReader<SerialStream>` pour `read_line` (parsing
/// efficace avec `read_until(b'\n', ...)`) et un `BufWriter` pour les écritures
/// regroupées si besoin.
pub struct SerialTransport {
    reader: BufReader<tokio::io::ReadHalf<SerialStream>>,
    writer: BufWriter<tokio::io::WriteHalf<SerialStream>>,
}

impl SerialTransport {
    /// Ouvre un port série déjà identifié.
    pub fn open(port_name: &str) -> Result<Self> {
        let stream = tokio_serial::new(port_name, BAUD)
            .timeout(std::time::Duration::from_millis(50))
            .open_native_async()
            .map_err(|e| Error::Transport(format!("open {port_name}: {e}")))?;
        let (rh, wh) = tokio::io::split(stream);
        Ok(Self {
            reader: BufReader::with_capacity(2048, rh),
            writer: BufWriter::with_capacity(2048, wh),
        })
    }

    /// Énumère les ports série et ouvre la première carte WraithRF/STM
    /// trouvée.  Renvoie [`Error::NoDevice`] si aucun candidat n'est branché.
    pub fn auto_detect() -> Result<Self> {
        let ports = serialport::available_ports()
            .map_err(|e| Error::Transport(format!("enumerate ports: {e}")))?;
        for p in &ports {
            if let serialport::SerialPortType::UsbPort(info) = &p.port_type {
                if info.vid == ST_VID {
                    tracing::info!(port = %p.port_name, "auto-detect: found STM device");
                    return Self::open(&p.port_name);
                }
            }
        }
        Err(Error::NoDevice)
    }

    /// Liste les ports série pour affichage UI / debug. Le tuple est
    /// `(port_name, optional_product_string)`.
    pub fn list_candidates() -> Result<Vec<(String, Option<String>)>> {
        let ports = serialport::available_ports()
            .map_err(|e| Error::Transport(format!("enumerate ports: {e}")))?;
        Ok(ports.into_iter().map(|p| {
            let product = match p.port_type {
                serialport::SerialPortType::UsbPort(info) => info.product,
                _ => None,
            };
            (p.port_name, product)
        }).collect())
    }
}

#[async_trait]
impl Transport for SerialTransport {
    async fn read_line(&mut self) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(256);
        let n = self.reader.read_until(b'\n', &mut buf).await?;
        if n == 0 {
            return Err(Error::Transport("EOF on serial port".into()));
        }
        // Strip trailing \n and optional \r.
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
