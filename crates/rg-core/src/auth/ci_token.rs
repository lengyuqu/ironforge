//! Least-privilege CI/CD job tokens.
//!
//! CI job tokens (`CI_JOB_TOKEN`) are short-lived JWTs scoped to a specific
//! repository with limited permissions. They are injected into CI job
//! environments and can be used to call the IronForge API during job execution.
//!
//! ## Token claims
//!
//! ```text
//! {
//!   sub: "ci:job:<id>",    // CI job identifier
//!   repo_id: <id>,          // scoped repository
//!   scope: "repo:read packages:read",  // space-separated permissions
//!   iss: "ironforge-ci",    // issuer identifier
//!   iat, exp                // standard JWT timestamps
//! }
//! ```

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};

/// CI job token claims.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CiJobClaims {
    /// Subject: "ci:job:<job_id>"
    pub sub: String,
    /// Repository this token is scoped to.
    pub repo_id: i64,
    /// Space-separated scope list.
    pub scope: String,
    /// Issuer: "ironforge-ci"
    pub iss: String,
    /// Issued-at (Unix timestamp seconds).
    pub iat: i64,
    /// Expiry (Unix timestamp seconds).
    pub exp: i64,
}

impl CiJobClaims {
    /// Check if this token has the required scope.
    ///
    /// Scopes are hierarchical: `repo:write` implies `repo:read`.
    pub fn has_scope(&self, required: &str) -> bool {
        let granted: Vec<&str> = self.scope.split_whitespace().collect();
        if granted.contains(&required) {
            return true;
        }
        // Hierarchical: "repo:write" implies "repo:read"
        if let Some((prefix, level)) = required.rsplit_once(':') {
            let write_scope = format!("{prefix}:write");
            if level == "read" && granted.contains(&write_scope.as_str()) {
                return true;
            }
        }
        false
    }

    /// Check if this token is authorized for the given repository.
    pub fn has_repo_access(&self, target_repo_id: i64) -> bool {
        self.repo_id == target_repo_id
    }
}

/// Generate a least-privilege CI job token.
///
/// The token is scoped to a specific repository with limited permissions.
/// Default TTL is 1 hour (CI jobs should be short-lived).
pub fn generate_ci_job_token(
    repo_id: i64,
    _pipeline_id: i64,
    job_id: i64,
    scopes: &str,
    secret: &str,
) -> Result<String> {
    let now = Utc::now();
    let exp = now + Duration::hours(1);
    let claims = CiJobClaims {
        sub: format!("ci:job:{}", job_id),
        repo_id,
        scope: scopes.to_string(),
        iss: "ironforge-ci".to_string(),
        iat: now.timestamp(),
        exp: exp.timestamp(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .context("ci job token encode failed")
}

/// Validate and decode a CI job token with scope and repo checking.
///
/// Returns the claims only if:
/// - Token signature is valid and not expired
/// - Token has the CI issuer ("ironforge-ci")
/// - Token has the required scope
/// - Token is authorized for the target repository
pub fn validate_ci_token(
    token: &str,
    secret: &str,
    target_repo_id: i64,
    required_scope: &str,
) -> Option<CiJobClaims> {
    use jsonwebtoken::{decode, DecodingKey, Validation};
    let claims = decode::<CiJobClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .ok()?
    .claims;

    // Must have the CI issuer
    if claims.iss != "ironforge-ci" {
        return None;
    }

    // Check repo + scope
    if !claims.has_repo_access(target_repo_id) || !claims.has_scope(required_scope) {
        return None;
    }

    Some(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_validate() {
        let secret = "ci_secret";
        let token = generate_ci_job_token(100, 1, 42, "repo:read packages:read", secret).unwrap();
        let claims = validate_ci_token(&token, secret, 100, "repo:read").unwrap();
        assert_eq!(claims.sub, "ci:job:42");
        assert_eq!(claims.repo_id, 100);
    }

    #[test]
    fn test_wrong_repo_rejected() {
        let secret = "ci_secret";
        let token = generate_ci_job_token(100, 1, 42, "repo:read", secret).unwrap();
        assert!(validate_ci_token(&token, secret, 200, "repo:read").is_none());
    }

    #[test]
    fn test_scope_hierarchy() {
        let secret = "ci_secret";
        let token = generate_ci_job_token(100, 1, 42, "repo:write", secret).unwrap();
        // write implies read
        assert!(validate_ci_token(&token, secret, 100, "repo:read").is_some());
        assert!(validate_ci_token(&token, secret, 100, "packages:read").is_none());
    }

    #[test]
    fn test_expired_token() {
        // Create a token with negative hours (already expired via custom encode)
        let secret = "ci_secret";
        let now = Utc::now();
        let exp = now - Duration::hours(1);
        let claims = CiJobClaims {
            sub: "ci:job:1".into(),
            repo_id: 1,
            scope: "repo:read".into(),
            iss: "ironforge-ci".into(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();
        assert!(validate_ci_token(&token, secret, 1, "repo:read").is_none());
    }
}
