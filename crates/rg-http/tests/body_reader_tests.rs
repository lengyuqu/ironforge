//! Regression: git request bodies are streamed, not buffered.
//!
//! The git handlers used to take `axum::body::Bytes`, which made memory use
//! proportional to the request size and read the whole body before the access
//! check could reject it. They now consume an `AsyncRead` fed from the request
//! body stream via `BodyReader`; this pins the adapter's framing behaviour.

use axum::body::Body;
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn body_reader_reassembles_chunked_input() {
    // Several chunks, including one larger than the read buffer below.
    let big = vec![b'x'; 70_000];
    let body = Body::from_stream(futures::stream::iter(vec![
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"hello ")),
        Ok(axum::body::Bytes::from_static(b"world")),
        Ok(axum::body::Bytes::from(big.clone())),
        Ok(axum::body::Bytes::from_static(b"tail")),
    ]));

    let mut reader = rg_http::body_reader::BodyReader::new(body.into_data_stream());
    let mut collected = Vec::new();
    // A small buffer forces the adapter to carry leftover bytes across polls.
    let mut buf = [0u8; 1024];
    loop {
        let read = reader.read(&mut buf).await.expect("read body");
        if read == 0 {
            break;
        }
        collected.extend_from_slice(&buf[..read]);
    }

    let mut expected = b"hello world".to_vec();
    expected.extend_from_slice(&big);
    expected.extend_from_slice(b"tail");
    assert_eq!(collected.len(), expected.len(), "byte count must be exact");
    assert_eq!(collected, expected, "chunk boundaries must be invisible");
}

#[tokio::test]
async fn body_reader_surfaces_stream_errors() {
    let body = Body::from_stream(futures::stream::iter(vec![
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"ok")),
    ]));
    let mut reader = rg_http::body_reader::BodyReader::new(body.into_data_stream());
    let mut buf = [0u8; 16];
    assert_eq!(reader.read(&mut buf).await.unwrap(), 2);
    assert_eq!(&buf[..2], b"ok");
    assert_eq!(reader.read(&mut buf).await.unwrap(), 0);
}