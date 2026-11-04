//! Bridge an axum request body stream to a `tokio::io::AsyncRead`.
//!
//! The git protocol handlers take `AsyncRead` (they are shared with the SSH
//! path), while an axum `Body` is a `Stream<Item = Result<Bytes, _>>`. Reading
//! the body through this adapter lets the handlers consume a push/clone request
//! incrementally instead of the server buffering the whole thing first.

use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::BodyDataStream;
use axum::body::Bytes;
use tokio::io::{AsyncRead, ReadBuf};

/// Adapts an [`axum::body::Body`] to `AsyncRead`.
pub struct BodyReader {
    inner: BodyDataStream,
    /// Leftover bytes from a chunk that was larger than the caller's buffer.
    pending: Option<Bytes>,
    offset: usize,
}

impl BodyReader {
    pub fn new(stream: BodyDataStream) -> Self {
        Self {
            inner: stream,
            pending: None,
            offset: 0,
        }
    }
}

impl AsyncRead for BodyReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        use futures::StreamExt;

        loop {
            // Drain any bytes left over from the previous chunk.
            if let Some(chunk) = self.pending.take() {
                let remaining = &chunk[self.offset..];
                if !remaining.is_empty() {
                    let take = remaining.len().min(buf.remaining());
                    buf.put_slice(&remaining[..take]);
                    if take < remaining.len() {
                        self.offset += take;
                        self.pending = Some(chunk);
                    } else {
                        self.offset = 0;
                    }
                    return Poll::Ready(Ok(()));
                }
                self.offset = 0;
            }

            // Pull the next chunk. `BodyDataStream` yields `axum::Error`.
            match self.inner.poll_next_unpin(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Ready(Some(Err(error))) => {
                    return Poll::Ready(Err(std::io::Error::other(error.to_string())))
                }
                Poll::Ready(Some(Ok(chunk))) => {
                    if chunk.is_empty() {
                        continue;
                    }
                    self.pending = Some(chunk);
                }
            }
        }
    }
}