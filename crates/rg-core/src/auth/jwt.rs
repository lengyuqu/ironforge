//! JWT token generation and validation.
//!
//! Tokens are HS256-signed JWTs with a configurable expiry (default 7 days).

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

/// JWT claims payload.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// Subject — user id as string.
    pub sub: String,
    /// Username (convenience field, not authoritative).
    pub username: String,
    /// Issued-at (Unix timestamp seconds).
    pub iat: i64,
    /// Expiry (Unix timestamp seconds).
    pub exp: i64,
}

/// Generate a signed JWT for a user.
pub fn generate_token(user_id: i64, username: &str, secret: &str, ttl_days: i64) -> Result<String> {
    let now = Utc::now();
    let exp = now + Duration::days(ttl_days);
    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        iat: now.timestamp(),
        exp: exp.timestamp(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .context("jwt encode failed")
}

/// Validate and decode a JWT. Returns `None` if invalid/expired.
pub fn validate_token(token: &str, secret: &str) -> Option<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|d| d.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_validate() {
        let secret = "test_secret_key";
        let token = generate_token(42, "alice", secret, 1).unwrap();
        let claims = validate_token(&token, secret).unwrap();
        assert_eq!(claims.sub, "42");
        assert_eq!(claims.username, "alice");
    }

    #[test]
    fn test_invalid_token() {
        assert!(validate_token("not.a.token", "secret").is_none());
    }
}
