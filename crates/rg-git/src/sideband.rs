//! Git sideband-64k protocol implementation.
//!
//! Sideband multiplexes multiple data streams (data, progress, error)
//! over a single channel. Each pkt-line has a 1-byte band number prefix:
//! - Band 1: Packfile data
//! - Band 2: Progress messages (displayed on stderr by client)
//! - Band 3: Error messages (fatal errors)

use anyhow::Result;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::pkt_line::{write_flush, write_pkt_line, PktLine};

/// Maximum sideband data size per pkt-line (65519 bytes for sideband-64k).
const SIDEBAND_MAX: usize = 65519;

/// Write sideband data (band 1) — used for packfile data.
pub async fn write_sideband_data<W: AsyncWrite + Unpin>(writer: &mut W, data: &[u8]) -> Result<()> {
    for chunk in data.chunks(SIDEBAND_MAX) {
        let mut payload = vec![1u8]; // band 1
        payload.extend_from_slice(chunk);
        write_pkt_line(writer, &PktLine::Data(payload)).await?;
    }
    writer.flush().await?;
    Ok(())
}

/// Write sideband progress (band 2) — displayed on client stderr.
pub async fn write_sideband_progress<W: AsyncWrite + Unpin>(
    writer: &mut W,
    message: &str,
) -> Result<()> {
    let mut payload = vec![2u8]; // band 2
    payload.extend_from_slice(message.as_bytes());
    write_pkt_line(writer, &PktLine::Data(payload)).await?;
    Ok(())
}

/// Write sideband error (band 3) — fatal error message.
pub async fn write_sideband_error<W: AsyncWrite + Unpin>(
    writer: &mut W,
    message: &str,
) -> Result<()> {
    let mut payload = vec![3u8]; // band 3
    payload.extend_from_slice(message.as_bytes());
    write_pkt_line(writer, &PktLine::Data(payload)).await?;
    Ok(())
}

/// Write the sideband flush packet (signals end of multiplexed data).
pub async fn write_sideband_flush<W: AsyncWrite + Unpin>(writer: &mut W) -> Result<()> {
    write_flush(writer).await
}
