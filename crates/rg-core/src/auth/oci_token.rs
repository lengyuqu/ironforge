//! OCI Distribution Spec — Bearer Token authentication.
//!
//! Implements [OCI Distribution Spec v1.0 — Token Authentication]
//! (https://docs.docker.com/registry/spec/auth/token/).
//!
//! ## Token Format
//!
//! IronForge issues **signed JWTs** (HS256) with the following claims:
//!
//! ```json
//! {
//!   "iss": "ironforge",
//!   "sub": "<username>",
//!   "aud": "ironforge-registry",
//!   "exp": 1700000000,
//!   "iat": 1699999900,
//!   "scope": "repository:owner/repo:pull,push"
//! }
//! ```
//!
//! ## Supported Scopes
//!
//! | Scope String | Meaning |
//! |-------------|---------|
//! | `repository:<owner>/<repo>:pull` | Read access to repo |
//! | `repository:<owner>/<repo>:push` | Write access to repo |
//!
//! `registry:catalog:*` is **not** issued. The parser still recognises the
//! shape so the token endpoint can report it as unsupported rather than
//! silently misreading it, but no grant is produced: there is no `_catalog`
//! endpoint, and the verifier only matches `repository` scopes.
//!
//! ## Token Endpoint
//!
//! `GET /v2/auth/token?service=...&scope=...`
//!
//! Returns `{ "token": "<jwt>" }`.

use crate::error::{CoreError, CoreResult};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// OCI Bearer token JWT claims.
#[derive(Debug, Serialize, Deserialize)]
pub struct OciTokenClaims {
    /// Issuer — always `"ironforge"`.
    pub iss: String,

    /// Subject — username (or `anonymous`).
    pub sub: String,

    /// Audience — must match `service` query parameter (`"ironforge-registry"`).
    pub aud: String,

    /// Expiration (UNIX seconds).
    pub exp: usize,

    /// Issued-at (UNIX seconds).
    pub iat: usize,

    /// Optional not-before (UNIX seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<usize>,

    /// Optional JWT ID (for revocation if needed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,

    /// Numeric id of the user the token was minted for, if any.
    ///
    /// `sub` carries a username, which is neither stable nor sufficient to
    /// re-check authorization: the verifier needs an id it can pass to
    /// `can_read_repo`/`can_write_repo`. Tokens issued before this field
    /// existed simply lack it (`None`), and then the verifier treats them as
    /// anonymous — which is why they stop working for repos that are private.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<i64>,

    /// Space-separated scope strings.
    /// Example: `"repository:alice/myapp:pull,push"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Parse a scope string into structured permissions.
///
/// Scope format: `<type>:<name>:<actions>`
/// - `repository:owner/repo:pull,push`
/// - `registry:catalog:*` — parsed, but never granted by the token endpoint
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedScope {
    pub scope_type: String,       // "repository" | "registry"
    pub name: String,             // "owner/repo" | "catalog"
    pub actions: HashSet<String>, // {"pull", "push"} | {"*"}
}

impl ParsedScope {
    /// Parse a single scope string.
    /// Returns `None` if the format is invalid.
    pub fn parse(scope_str: &str) -> Option<Self> {
        let parts: Vec<&str> = scope_str.splitn(3, ':').collect();
        if parts.len() != 3 {
            return None;
        }
        let actions: HashSet<String> = parts[2]
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if actions.is_empty() {
            return None;
        }
        Some(Self {
            scope_type: parts[0].to_string(),
            name: parts[1].to_string(),
            actions,
        })
    }

    /// Check if this scope grants a specific action.
    pub fn has_action(&self, action: &str) -> bool {
        self.actions.contains("*") || self.actions.contains(action)
    }

    /// Check if this is a repository scope for a specific repo.
    pub fn matches_repo(&self, owner: &str, repo: &str) -> bool {
        self.scope_type == "repository" && self.name == format!("{}/{}", owner, repo)
    }
}

/// Generate an OCI Bearer token (JWT HS256).
///
/// - `username`: The authenticated username (or `"anonymous"` for public access).
/// - `scope`: Space-separated scope strings (e.g., `"repository:alice/hello:pull,push"`).
/// - `secret`: The JWT signing secret (same as `jwt_secret`).
/// - `ttl_secs`: Token time-to-live in seconds (recommended: 300 = 5 min).
/// - `uid`: Numeric id of the authenticated user, or `None` for an anonymous
///   token. The verifier re-checks repository visibility against this id, so a
///   token minted while a repo was public stops working once it turns private.
///
/// Returns the serialized JWT string.
pub fn generate_oci_token(
    username: &str,
    uid: Option<i64>,
    scope: &str,
    secret: &str,
    ttl_secs: u64,
) -> CoreResult<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize;

    let claims = OciTokenClaims {
        iss: "ironforge".to_string(),
        sub: username.to_string(),
        aud: "ironforge-registry".to_string(),
        iat: now,
        exp: now + ttl_secs as usize,
        nbf: None,
        jti: None,
        uid,
        scope: if scope.is_empty() {
            None
        } else {
            Some(scope.to_string())
        },
    };

    let key = EncodingKey::from_secret(secret.as_bytes())
        .map_err(|e| CoreError::internal(format!("OCI token generation failed: {}", e)))?;
    let token = encode(&Header::default(), &claims, &key)
        .map_err(|e| CoreError::internal(format!("OCI token generation failed: {}", e)))?;
    Ok(token)
}

/// Validate an OCI Bearer token (JWT HS256).
///
/// Returns the claims if valid, `None` otherwise.
pub fn validate_oci_token(token: &str, secret: &str) -> Option<OciTokenClaims> {
    let key = DecodingKey::from_secret(secret.as_bytes()).ok()?;
    let validation = Validation {
        iss: Some(std::collections::HashSet::from(["ironforge".to_string()])),
        aud: Some(std::collections::HashSet::from([
            "ironforge-registry".to_string()
        ])),
        ..Default::default()
    };

    match decode::<OciTokenClaims>(token, &key, &validation) {
        Ok(data) => Some(data.claims),
        Err(e) => {
            tracing::debug!("OCI token validation failed: {}", e);
            None
        }
    }
}

/// Extract and validate the OCI Bearer token from HTTP headers.
///
/// Looks for `Authorization: Bearer <token>` and validates it.
/// Returns `Some(claims)` if valid, `None` otherwise.
pub fn extract_oci_claims(headers: &http::HeaderMap, secret: &str) -> Option<OciTokenClaims> {
    let auth = headers.get(http::header::AUTHORIZATION)?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    validate_oci_token(token, secret)
}

/// Check if the request has permission for a specific repository action.
///
/// - `headers`: HTTP headers (for `Authorization: Bearer`)
/// - `secret`: JWT secret
/// - `owner`: Repository owner
/// - `repo`: Repository name
/// - `required_action`: `"pull"` or `"push"`
///
/// Returns `true` if authorized, `false` otherwise.
///
/// ## Logic
///
/// 1. If no token: public pull is allowed (for public repos)
/// 2. If token has `repository:<owner>/<repo>:pull` → allow pull
/// 3. If token has `repository:<owner>/<repo>:push` → allow push
/// 4. If token has `repository:<owner>/<repo>:*` → allow all
pub fn check_repo_access(
    headers: &http::HeaderMap,
    secret: &str,
    owner: &str,
    repo: &str,
    required_action: &str,
) -> bool {
    let claims = match extract_oci_claims(headers, secret) {
        Some(c) => c,
        None => {
            // No token: allow public pull only
            return required_action == "pull";
        }
    };

    let scope_str = match claims.scope {
        Some(ref s) => s.clone(),
        None => return false,
    };

    // Parse all scopes (space-separated)
    for single_scope in scope_str.split_whitespace() {
        if let Some(parsed) = ParsedScope::parse(single_scope) {
            if parsed.matches_repo(owner, repo) && parsed.has_action(required_action) {
                return true;
            }
        }
    }

    false
}

/// Build the `WWW-Authenticate` header value for OCI Distribution auth.
///
/// `scope` is optional in the OCI distribution spec, and the initial `/v2/`
/// probe does not need one. Pass `""` to omit it — advertising a scope the
/// server cannot serve only invites clients to request it.
///
/// Example:
/// ```text
/// Bearer realm="https://registry.example.com/v2/auth/token",service="registry",scope="repository:alice/hello:pull,push"
/// ```
pub fn build_www_authenticate(realm: &str, service: &str, scope: &str) -> String {
    if scope.is_empty() {
        return format!(r#"Bearer realm="{}",service="{}""#, realm, service);
    }
    // URL-encode the parameters
    let scope_encoded = urlencoding::encode(scope);
    format!(
        r#"Bearer realm="{}",service="{}",scope="{}""#,
        realm, service, scope_encoded
    )
}

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-oci-secret-key-1234567890";

    #[test]
    fn test_generate_and_validate_token() {
        let token = generate_oci_token(
            "alice",
            Some(7),
            "repository:alice/hello:pull,push",
            TEST_SECRET,
            300,
        )
        .unwrap();
        assert!(!token.is_empty());

        let claims = validate_oci_token(&token, TEST_SECRET).unwrap();
        assert_eq!(claims.sub, "alice");
        assert_eq!(claims.aud, "ironforge-registry");
        assert!(claims.scope.as_ref().unwrap().contains("pull,push"));
    }

    #[test]
    fn test_validate_invalid_token() {
        let result = validate_oci_token("invalid.jwt.token", TEST_SECRET);
        assert!(result.is_none());
    }

    #[test]
    fn test_validate_wrong_secret() {
        let token = generate_oci_token(
            "alice",
            Some(7),
            "repository:alice/hello:pull",
            TEST_SECRET,
            300,
        )
        .unwrap();
        let wrong_secret = "wrong-secret";
        let result = validate_oci_token(&token, wrong_secret);
        assert!(result.is_none());
    }

    #[test]
    fn test_parsed_scope() {
        let scope = ParsedScope::parse("repository:alice/hello:pull,push").unwrap();
        assert_eq!(scope.scope_type, "repository");
        assert_eq!(scope.name, "alice/hello");
        assert!(scope.has_action("pull"));
        assert!(scope.has_action("push"));
        assert!(!scope.has_action("delete"));
        assert!(scope.matches_repo("alice", "hello"));
        assert!(!scope.matches_repo("alice", "world"));
    }

    #[test]
    fn test_parsed_scope_wildcard() {
        let scope = ParsedScope::parse("repository:alice/hello:*").unwrap();
        assert!(scope.has_action("pull"));
        assert!(scope.has_action("push"));
        assert!(scope.has_action("delete"));
    }

    #[test]
    fn test_parsed_scope_invalid() {
        assert!(ParsedScope::parse("invalid-scope").is_none());
        assert!(ParsedScope::parse("type:name").is_none()); // missing actions
    }

    #[test]
    fn test_check_repo_access_no_token() {
        let headers = http::HeaderMap::new();
        // No token: public pull allowed
        assert!(check_repo_access(
            &headers,
            TEST_SECRET,
            "alice",
            "hello",
            "pull"
        ));
        // No token: push NOT allowed
        assert!(!check_repo_access(
            &headers,
            TEST_SECRET,
            "alice",
            "hello",
            "push"
        ));
    }

    #[test]
    fn test_check_repo_access_with_token() {
        let token = generate_oci_token(
            "alice",
            Some(7),
            "repository:alice/hello:pull,push",
            TEST_SECRET,
            300,
        )
        .unwrap();
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        );

        assert!(check_repo_access(
            &headers,
            TEST_SECRET,
            "alice",
            "hello",
            "pull"
        ));
        assert!(check_repo_access(
            &headers,
            TEST_SECRET,
            "alice",
            "hello",
            "push"
        ));
        assert!(!check_repo_access(
            &headers,
            TEST_SECRET,
            "alice",
            "hello",
            "delete"
        ));
        // Wrong repo
        assert!(!check_repo_access(
            &headers,
            TEST_SECRET,
            "bob",
            "hello",
            "pull"
        ));
    }

    #[test]
    fn test_build_www_authenticate() {
        let www = build_www_authenticate(
            "https://example.com/v2/auth/token",
            "registry",
            "repository:alice/hello:pull,push",
        );
        assert!(www.contains(r#"realm="https://example.com/v2/auth/token""#));
        assert!(www.contains(r#"service="registry""#));
        assert!(www.contains("scope="));
    }

    /// An empty scope must be omitted entirely, not rendered as `scope=""`.
    /// The `/v2/` probe passes `""` because the registry has no `_catalog`
    /// endpoint to offer.
    #[test]
    fn test_build_www_authenticate_without_scope() {
        let www = build_www_authenticate("https://example.com/v2/auth/token", "registry", "");
        assert_eq!(
            www,
            r#"Bearer realm="https://example.com/v2/auth/token",service="registry""#
        );
        assert!(!www.contains("scope"));
    }

    /// The token must carry the numeric user id, because the verifier re-checks
    /// repository visibility and `can_read_repo` cannot take a username.
    #[test]
    fn test_token_carries_numeric_uid() {
        let token = generate_oci_token(
            "alice",
            Some(7),
            "repository:alice/hello:pull",
            TEST_SECRET,
            300,
        )
        .unwrap();
        let claims = validate_oci_token(&token, TEST_SECRET).unwrap();
        assert_eq!(claims.sub, "alice");
        assert_eq!(claims.uid, Some(7));
    }

    #[test]
    fn test_anonymous_token_has_no_uid() {
        let token = generate_oci_token(
            "anonymous",
            None,
            "repository:alice/hello:pull",
            TEST_SECRET,
            60,
        )
        .unwrap();
        let claims = validate_oci_token(&token, TEST_SECRET).unwrap();
        assert_eq!(claims.sub, "anonymous");
        assert_eq!(claims.uid, None);
    }

    /// Tokens minted before `uid` existed must still decode, with `uid: None`.
    /// They then get evaluated as anonymous, which is the intended downgrade —
    /// otherwise every in-flight token would start failing validation outright.
    #[test]
    fn test_token_without_uid_claim_still_validates() {
        use jsonwebtoken::{encode, EncodingKey, Header};

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize;
        // Hand-built payload with no `uid` key at all.
        let legacy = serde_json::json!({
            "iss": "ironforge",
            "sub": "alice",
            "aud": "ironforge-registry",
            "iat": now,
            "exp": now + 300,
            "scope": "repository:alice/hello:pull",
        });
        let key = EncodingKey::from_secret(TEST_SECRET.as_bytes()).unwrap();
        let token = encode(&Header::default(), &legacy, &key).unwrap();

        let claims = validate_oci_token(&token, TEST_SECRET).unwrap();
        assert_eq!(claims.sub, "alice");
        assert_eq!(
            claims.uid, None,
            "a token without `uid` must decode as anonymous, not fail"
        );
    }
}
