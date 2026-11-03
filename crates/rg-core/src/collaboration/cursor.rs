//! Opaque, integrity-protected pagination cursors.
//!
//! A cursor is `base64url(json).base64url(hmac)`. The HMAC covers a *scope*
//! string chosen by the endpoint (repository, filter, sort order, format
//! version), so a cursor minted for one listing is rejected by another and a
//! tampered position is rejected rather than silently reinterpreted.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, SecondsFormat, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use super::error::{CollabError, CollabResult};

const KEY_DOMAIN: &[u8] = b"ironforge.collab.cursor.v1\0";

/// Position of the last row of a keyset page, as read back from the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeysetPosition {
    pub at: DateTime<Utc>,
    pub id: String,
}

#[derive(Serialize, Deserialize)]
struct Payload {
    s: String,
    t: String,
    i: String,
}

fn mac(key: &[u8], body: &[u8]) -> Hmac<Sha256> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(KEY_DOMAIN);
    mac.update(body);
    mac
}

pub fn encode(key: &[u8], scope: &str, position: &KeysetPosition) -> String {
    let payload = Payload {
        s: scope.to_string(),
        t: position.at.to_rfc3339_opts(SecondsFormat::Nanos, true),
        i: position.id.clone(),
    };
    let body = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).expect("cursor payload serializes"));
    let tag = URL_SAFE_NO_PAD.encode(mac(key, body.as_bytes()).finalize().into_bytes());
    format!("{body}.{tag}")
}

pub fn decode(key: &[u8], scope: &str, token: &str) -> CollabResult<KeysetPosition> {
    let invalid = || CollabError::bad_request("invalid cursor");
    let (body, tag) = token.split_once('.').ok_or_else(invalid)?;
    let tag = URL_SAFE_NO_PAD.decode(tag).map_err(|_| invalid())?;
    mac(key, body.as_bytes())
        .verify_slice(&tag)
        .map_err(|_| invalid())?;
    let raw = URL_SAFE_NO_PAD.decode(body).map_err(|_| invalid())?;
    let payload: Payload = serde_json::from_slice(&raw).map_err(|_| invalid())?;
    if payload.s != scope {
        return Err(invalid());
    }
    let at = DateTime::parse_from_rfc3339(&payload.t)
        .map_err(|_| invalid())?
        .with_timezone(&Utc);
    Ok(KeysetPosition { at, id: payload.i })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos() -> KeysetPosition {
        KeysetPosition {
            at: DateTime::parse_from_rfc3339("2026-11-04T01:02:03.123456789Z")
                .unwrap()
                .with_timezone(&Utc),
            id: "0b0e7f3a-0000-4000-8000-000000000001".into(),
        }
    }

    #[test]
    fn round_trips_with_nanosecond_precision() {
        let token = encode(b"k", "tasks:1", &pos());
        assert_eq!(decode(b"k", "tasks:1", &token).unwrap(), pos());
    }

    #[test]
    fn rejects_other_scope_other_key_and_tampering() {
        let token = encode(b"k", "tasks:1", &pos());
        assert!(decode(b"k", "tasks:2", &token).is_err(), "scope is bound");
        assert!(decode(b"other", "tasks:1", &token).is_err(), "key is bound");

        let (body, tag) = token.split_once('.').unwrap();
        let forged = Payload {
            s: "tasks:1".into(),
            t: "2000-01-01T00:00:00.000000000Z".into(),
            i: "x".into(),
        };
        let forged_body = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&forged).unwrap());
        assert!(decode(b"k", "tasks:1", &format!("{forged_body}.{tag}")).is_err());
        assert!(decode(b"k", "tasks:1", &format!("{body}.AAAA")).is_err());
        assert!(decode(b"k", "tasks:1", "garbage").is_err());
        assert!(decode(b"k", "tasks:1", "").is_err());
    }
}
