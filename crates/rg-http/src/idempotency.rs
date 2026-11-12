//! `Idempotency-Key` for the create-type requests.
//!
//! A client that retries after a network error or a timeout sends the same
//! `Idempotency-Key` header; the server then answers with the first response
//! instead of creating a second pull request or issue. The key is scoped to the
//! user and, for a personal access token, to the token, so two clients never
//! collide.
//!
//! Rules (design: `harness-native-design-2026-11.md` §4.5):
//!
//! * same key, same request: the stored response is replayed (`Idempotent-Replay:
//!   true`), whatever its status below 500;
//! * same key, a different request (method, path, query, body or `X-Dry-Run`):
//!   `422`, `reason: IDEMPOTENCY_KEY_REUSED`;
//! * same key while the first request is still running: `409`,
//!   `reason: IDEMPOTENCY_IN_PROGRESS`, retryable;
//! * a response with status 500 or more, or too large to keep, releases the key,
//!   so a retry runs again;
//! * keys are kept for 24 hours.
//!
//! Only routes whose response carries no secret are covered
//! ([`idempotency_supported`]): the response is stored in the database, so a
//! route that returns a token or a webhook secret must never be added. Sending
//! the header to any other route is refused (`IDEMPOTENCY_NOT_SUPPORTED`) rather
//! than silently ignored, so a client never retries believing it is protected.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::AppState;

/// The request header carrying the client's key.
pub const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";
/// How long a key is kept.
pub const RETENTION: chrono::Duration = chrono::Duration::hours(24);
/// A request that has been `pending` this long is treated as abandoned (the
/// server died while handling it) and its key is released.
const PENDING_TIMEOUT: chrono::Duration = chrono::Duration::minutes(5);
/// Largest request body that is hashed, and largest response that is stored.
const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;
const MAX_STORED_RESPONSE_BYTES: usize = 32_000;
/// A response read past this is not a document a create route returns.
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// A key is 1–255 characters of `[A-Za-z0-9._:-]`.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 255
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

/// Whether a request path (without the `/api/v1` prefix) may use an idempotency
/// key: creating a repository, or writing under one of a repository's
/// issues / pull requests / contents / commits / releases / labels /
/// milestones / branches / wiki. Everything else, in particular anything that
/// returns a token or a secret, is out.
///
/// Derived from the route declarations (`rg_core::auth::route_meta`, `.idem()`):
/// a route accepts a key only if it is declared replayable, and a path that is
/// not a declared route does not. Declare `.idem()` only for a response that
/// carries no token or secret, because the response is stored.
pub fn idempotency_supported(path: &str) -> bool {
    rg_core::auth::route_meta::idempotency_replayable(path)
}

fn fingerprint(method: &Method, target: &str, dry_run: &str, body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(method.as_str().as_bytes());
    hasher.update(b"\n");
    hasher.update(target.as_bytes());
    hasher.update(b"\n");
    hasher.update(dry_run.as_bytes());
    hasher.update(b"\n");
    hasher.update(body);
    format!("{:x}", hasher.finalize())
}

fn is_write(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

/// Axum middleware. Runs **inside** the PAT middleware (which has already turned
/// a token into a session and stamped the token id) so it sees the real actor.
pub async fn idempotency_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(raw_key) = req
        .headers()
        .get(IDEMPOTENCY_KEY_HEADER)
        .map(|v| v.to_str().unwrap_or("").to_string())
    else {
        return next.run(req).await;
    };
    if !is_write(req.method()) {
        // A read has nothing to deduplicate; the header is harmless there.
        return next.run(req).await;
    }
    let key = raw_key.trim().to_string();
    if !valid_key(&key) {
        return AppError::bad_request(
            "Idempotency-Key must be 1-255 characters of [A-Za-z0-9._:-]",
        )
        .into_response();
    }
    let path = req
        .uri()
        .path()
        .strip_prefix("/api/v1")
        .unwrap_or(req.uri().path())
        .to_string();
    if !idempotency_supported(&path) {
        return AppError::coded(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "this route does not support Idempotency-Key",
        )
        .with_reason(
            "IDEMPOTENCY_NOT_SUPPORTED",
            false,
            "send the request without the header, or check the state before retrying",
        )
        .into_response();
    }
    // Without a known actor the handler will answer 401; nothing to store.
    let Some(user_id) = crate::api::auth::extract_user_id(req.headers(), &state.jwt_secret) else {
        return next.run(req).await;
    };
    let token_id = req
        .headers()
        .get(crate::api::audit::ACCESS_TOKEN_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);

    let (parts, body) = req.into_parts();
    let body_bytes = match axum::body::to_bytes(body, MAX_REQUEST_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return AppError::coded(
                StatusCode::PAYLOAD_TOO_LARGE,
                "PAYLOAD_TOO_LARGE",
                "the request is too large to use with Idempotency-Key",
            )
            .into_response()
        }
    };
    let target = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("")
        .to_string();
    let dry_run = parts
        .headers
        .get("x-dry-run")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let print = fingerprint(&parts.method, &target, &dry_run, &body_bytes);

    let claimed = match claim_with_recovery(&state, user_id, token_id, &key, &print).await {
        Ok(Claimed::Own(id)) => id,
        Ok(Claimed::Answer(response)) => return response,
        Err(error) => return error.into_response(),
    };

    let response = next
        .run(Request::from_parts(parts, Body::from(body_bytes)))
        .await;
    store_or_release(&state, claimed, response).await
}

enum Claimed {
    /// This request owns the key (row id) and must run.
    Own(i64),
    /// The key was already used: this is the answer to send.
    Answer(Response),
}

async fn claim_with_recovery(
    state: &AppState,
    user_id: i64,
    token_id: i64,
    key: &str,
    print: &str,
) -> Result<Claimed, AppError> {
    use rg_db::ops::idempotency_ops::{self, Claim};
    for _ in 0..2 {
        let expires_before = chrono::Utc::now() - RETENTION;
        let claim =
            match idempotency_ops::claim(&state.db, user_id, token_id, key, print, expires_before)
                .await
            {
                Ok(claim) => claim,
                // The winner's row was deleted between the conflict and the read.
                Err(rg_db::sea_orm::DbErr::RecordNotFound(_)) => continue,
                Err(error) => return Err(AppError::from(error)),
            };
        let existing = match claim {
            Claim::Claimed(id) => return Ok(Claimed::Own(id)),
            Claim::Existing(row) => row,
        };
        if existing.fingerprint != print {
            return Ok(Claimed::Answer(
                AppError::coded(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "UNPROCESSABLE",
                    "this Idempotency-Key was already used with a different request",
                )
                .with_reason(
                    "IDEMPOTENCY_KEY_REUSED",
                    false,
                    "use a new key for a different request, or resend the original request unchanged",
                )
                .into_response(),
            ));
        }
        if existing.state == idempotency_ops::STATE_DONE {
            return Ok(Claimed::Answer(replay(&existing)));
        }
        // Still running. If it has been running implausibly long, the server died
        // while handling it: release the key and claim it again.
        if existing.created_at < chrono::Utc::now() - PENDING_TIMEOUT {
            let _ = idempotency_ops::release(&state.db, existing.id).await;
            continue;
        }
        return Ok(Claimed::Answer(
            AppError::conflict_because(
                "IDEMPOTENCY_IN_PROGRESS",
                true,
                "a request with this Idempotency-Key is still being processed",
                serde_json::json!({}),
                "wait a moment and send the same request again to get its result",
            )
            .into_response(),
        ));
    }
    Err(AppError::internal("could not claim the idempotency key"))
}

/// The stored response of the first request.
fn replay(row: &rg_db::entities::idempotency_key::Model) -> Response {
    let status = row
        .status_code
        .and_then(|s| u16::try_from(s).ok())
        .and_then(|s| StatusCode::from_u16(s).ok())
        .unwrap_or(StatusCode::OK);
    let mut response = Response::new(Body::from(row.response_body.clone().unwrap_or_default()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
        .headers_mut()
        .insert("idempotent-replay", HeaderValue::from_static("true"));
    response
}

/// Keep the response for replay, or release the key when it cannot or should not
/// be replayed (a server error, a body too large to keep, a body that is not
/// text). The response is always returned to the client unchanged.
async fn store_or_release(state: &AppState, id: i64, response: Response) -> Response {
    let status = response.status();
    if status.is_server_error() {
        let _ = rg_db::ops::idempotency_ops::release(&state.db, id).await;
        return response;
    }
    let (parts, body) = response.into_parts();
    // Routes covered by an idempotency key return small JSON documents, so the
    // whole response is read; a body past the hard cap is an error, not a stream.
    let bytes = match axum::body::to_bytes(body, MAX_RESPONSE_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            let _ = rg_db::ops::idempotency_ops::release(&state.db, id).await;
            return AppError::internal("failed to read the response").into_response();
        }
    };
    let storable = if bytes.len() <= MAX_STORED_RESPONSE_BYTES {
        std::str::from_utf8(&bytes).ok()
    } else {
        None
    };
    match storable {
        Some(text) => {
            if let Err(error) = rg_db::ops::idempotency_ops::complete(
                &state.db,
                id,
                i32::from(status.as_u16()),
                text,
            )
            .await
            {
                tracing::warn!(%error, "could not store an idempotent response");
                let _ = rg_db::ops::idempotency_ops::release(&state.db, id).await;
            }
        }
        // Too large, or not text: it cannot be replayed, so a retry runs again.
        None => {
            let _ = rg_db::ops::idempotency_ops::release(&state.db, id).await;
        }
    }
    Response::from_parts(parts, Body::from(bytes))
}

/// Delete keys older than the retention period, forever.
pub async fn run_cleanup_loop(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
    loop {
        interval.tick().await;
        let before = chrono::Utc::now() - RETENTION;
        match rg_db::ops::idempotency_ops::delete_older_than(&state.db, before).await {
            Ok(0) => {}
            Ok(count) => tracing::info!(count, "deleted expired idempotency keys"),
            Err(error) => tracing::warn!(%error, "idempotency key cleanup failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_short_and_plain() {
        assert!(valid_key("a"));
        assert!(valid_key("agent-1:create_pr.2026-11-06"));
        assert!(valid_key(&"k".repeat(255)));
        assert!(!valid_key(""));
        assert!(!valid_key(&"k".repeat(256)));
        assert!(!valid_key("has space"));
        assert!(!valid_key("a/b"));
        assert!(!valid_key("新"));
        assert!(!valid_key("a\nb"));
    }

    #[test]
    fn only_routes_whose_response_carries_no_secret_are_covered() {
        for path in [
            "/repos",
            "/repos/a/b/issues",
            "/repos/a/b/issues/3/comments",
            "/repos/a/b/pulls",
            "/repos/a/b/pulls/3/merge",
            "/repos/a/b/contents/src/main.rs",
            "/repos/a/b/releases",
            "/repos/a/b/branches",
        ] {
            assert!(idempotency_supported(path), "{path}");
        }
        for path in [
            // Responses carry a secret.
            "/users/tokens",
            "/users/login",
            "/users/register",
            "/repos/a/b/hooks",
            "/repos/a/b/collaboration/sessions",
            "/repos/a/b/deploy-keys",
            // Not covered.
            "/repos/a/b",
            "/repos/a/b/issues/../hooks",
            "/repos//b/issues",
            "/admin/users",
            "",
        ] {
            assert!(!idempotency_supported(path), "{path}");
        }
    }

    #[test]
    fn the_fingerprint_covers_method_target_dry_run_and_body() {
        let base = fingerprint(&Method::POST, "/repos/a/b/pulls", "", b"{}");
        assert_eq!(
            base,
            fingerprint(&Method::POST, "/repos/a/b/pulls", "", b"{}")
        );
        assert_ne!(
            base,
            fingerprint(&Method::PUT, "/repos/a/b/pulls", "", b"{}")
        );
        assert_ne!(
            base,
            fingerprint(&Method::POST, "/repos/a/b/pulls?x=1", "", b"{}")
        );
        assert_ne!(
            base,
            fingerprint(&Method::POST, "/repos/a/b/pulls", "1", b"{}")
        );
        assert_ne!(
            base,
            fingerprint(&Method::POST, "/repos/a/b/pulls", "", b"{ }")
        );
    }
}
