//! Git pkt-line protocol implementation.
//!
//! Pkt-line format: 4 hex digits for length (including the 4-byte header),
//! followed by payload data. A line of "0000" is a flush packet.

use std::fmt;

use anyhow::{bail, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

/// Maximum payload size per pkt-line (65516 bytes, per git protocol).
pub const MAX_PKT_LINE_LEN: usize = 65516;

/// A pkt-line: either data or a flush packet.
#[derive(Debug, Clone)]
pub enum PktLine {
    Data(Vec<u8>),
    Flush,
}

impl PktLine {
    /// Create a data pkt-line from bytes.
    pub fn data(data: &[u8]) -> Self {
        PktLine::Data(data.to_vec())
    }

    /// Create a text pkt-line from a string (with trailing newline).
    pub fn text(text: &str) -> Self {
        let mut v = text.as_bytes().to_vec();
        if !v.ends_with(b"\n") {
            v.push(b'\n');
        }
        PktLine::Data(v)
    }
}

impl fmt::Display for PktLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PktLine::Data(data) => {
                // Try to display as text
                match std::str::from_utf8(data) {
                    Ok(s) => write!(f, "Data({})", s.trim_end()),
                    Err(_) => write!(f, "Data({} bytes)", data.len()),
                }
            }
            PktLine::Flush => write!(f, "Flush"),
        }
    }
}

/// Write a single pkt-line to a writer.
pub async fn write_pkt_line<W: AsyncWrite + Unpin>(writer: &mut W, pkt: &PktLine) -> Result<()> {
    match pkt {
        PktLine::Data(data) => {
            if data.len() > MAX_PKT_LINE_LEN {
                bail!(
                    "pkt-line data too large: {} bytes (max {})",
                    data.len(),
                    MAX_PKT_LINE_LEN
                );
            }
            let len = data.len() + 4; // +4 for the length header itself
            let header = format!("{:04x}", len);
            writer.write_all(header.as_bytes()).await?;
            writer.write_all(data).await?;
        }
        PktLine::Flush => {
            writer.write_all(b"0000").await?;
        }
    }
    Ok(())
}

/// Write a flush packet.
pub async fn write_flush<W: AsyncWrite + Unpin>(writer: &mut W) -> Result<()> {
    writer.write_all(b"0000").await?;
    Ok(())
}

/// Read a single pkt-line from a buffered reader.
pub async fn read_pkt_line<R: AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Result<PktLine> {
    let mut header = [0u8; 4];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            // Connection closed gracefully
            return Ok(PktLine::Flush);
        }
        Err(e) => return Err(e.into()),
    }

    let header_str = std::str::from_utf8(&header)?;
    let len: usize = match u32::from_str_radix(header_str, 16) {
        Ok(0) => return Ok(PktLine::Flush),
        Ok(n) => n as usize,
        Err(_) => bail!("invalid pkt-line header: {:?}", header),
    };

    if len < 4 {
        bail!("invalid pkt-line length: {}", len);
    }

    let payload_len = len - 4;
    if payload_len == 0 {
        return Ok(PktLine::Data(Vec::new()));
    }

    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).await?;
    Ok(PktLine::Data(payload))
}

/// Read pkt-lines until flush. Returns all data lines (excluding flush).
pub async fn read_pkt_lines_until_flush<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<Vec<PktLine>> {
    let mut lines = Vec::new();
    loop {
        let pkt = read_pkt_line(reader).await?;
        match pkt {
            PktLine::Flush => break,
            _ => lines.push(pkt),
        }
    }
    Ok(lines)
}

/// Read a single text line (non-flush pkt-line) as a string.
pub async fn read_text_line<R: AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Result<Option<String>> {
    let pkt = read_pkt_line(reader).await?;
    match pkt {
        PktLine::Flush => Ok(None),
        PktLine::Data(data) => {
            let text = String::from_utf8(data)?;
            Ok(Some(text))
        }
    }
}
