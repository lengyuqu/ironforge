//! Session secret generation and verification.
//!
//! The secret is 256 random bits, returned to the client exactly once. Only a
//! domain-separated digest bound to the session id is stored, so a database
//! read does not yield usable credentials and a digest cannot be replayed for
//! another session.

use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Prefix that makes the secret recognisable to secret scanners and log filters.
pub const SECRET_PREFIX: &str = "ifs_";
const DIGEST_DOMAIN: &[u8] = b"ironforge.collab.session-secret.v1\0";

/// Generate a fresh secret. Panics if the OS entropy source is unavailable
/// (fail closed, like PAT generation: never mint a guessable credential).
pub fn generate() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    format!("{SECRET_PREFIX}{}", hex::encode(bytes))
}

/// Digest stored in `agent_sessions.secret_hash` (lower-case hex SHA-256).
pub fn hash(session_id: &str, secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DIGEST_DOMAIN);
    hasher.update(session_id.as_bytes());
    hasher.update([0u8]);
    hasher.update(secret.as_bytes());
    hex::encode(hasher.finalize())
}

/// Constant-time comparison of the presented secret against the stored digest.
pub fn verify(session_id: &str, secret: &str, stored_hash: &str) -> bool {
    let candidate = hash(session_id, secret);
    let (a, b) = (candidate.as_bytes(), stored_hash.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_unique_and_long() {
        let (a, b) = (generate(), generate());
        assert_ne!(a, b);
        assert!(a.starts_with(SECRET_PREFIX));
        assert_eq!(a.len(), SECRET_PREFIX.len() + 64);
    }

    #[test]
    fn digest_is_bound_to_session_and_secret() {
        let secret = generate();
        let stored = hash("session-a", &secret);
        assert!(verify("session-a", &secret, &stored));
        assert!(
            !verify("session-b", &secret, &stored),
            "digest must not transfer"
        );
        assert!(!verify("session-a", &generate(), &stored));
        assert!(!verify("session-a", &secret, "short"));
        assert!(!stored.contains(&secret));
    }
}
