//! Simple token-bucket rate limiter middleware for Axum.
//!
//! Limits requests per IP address. Configurable requests-per-minute.
//! Returns 429 Too Many Requests when the limit is exceeded.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tokio::sync::Mutex;

/// Per-client rate limit state.
#[derive(Debug)]
struct ClientState {
    /// Number of requests remaining in the current window.
    tokens: u32,
    /// When the current window resets.
    reset_at: Instant,
}

/// Shared rate limiter state.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    /// Maximum requests per window.
    max_requests: u32,
    /// Window duration in seconds.
    window_secs: u64,
    /// Client IP → state mapping.
    clients: Arc<Mutex<HashMap<String, ClientState>>>,
}

impl RateLimiter {
    /// Create a new rate limiter.
    ///
    /// - `max_requests`: maximum number of requests allowed per window.
    /// - `window_secs`: duration of the rate limit window in seconds.
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            max_requests,
            window_secs,
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if a request is allowed. Returns true if the request should proceed.
    async fn allow(&self, key: &str) -> bool {
        let mut clients = self.clients.lock().await;
        let now = Instant::now();

        let entry = clients.entry(key.to_string()).or_insert_with(|| ClientState {
            tokens: self.max_requests,
            reset_at: now + std::time::Duration::from_secs(self.window_secs),
        });

        // Reset window if expired
        if now >= entry.reset_at {
            entry.tokens = self.max_requests;
            entry.reset_at = now + std::time::Duration::from_secs(self.window_secs);
        }

        if entry.tokens > 0 {
            entry.tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Clean up expired entries (call periodically).
    pub async fn cleanup(&self) {
        let mut clients = self.clients.lock().await;
        let now = Instant::now();
        clients.retain(|_, state| now < state.reset_at);
    }
}

/// Extract client IP from headers (X-Forwarded-For, X-Real-IP) or connection info.
fn extract_client_key(headers: &HeaderMap) -> String {
    // Try X-Forwarded-For first (first IP in the list)
    if let Some(xff) = headers.get("x-forwarded-for") {
        if let Ok(val) = xff.to_str() {
            if let Some(ip) = val.split(',').next() {
                return ip.trim().to_string();
            }
        }
    }

    // Try X-Real-IP
    if let Some(xri) = headers.get("x-real-ip") {
        if let Ok(val) = xri.to_str() {
            return val.trim().to_string();
        }
    }

    // Fallback: use a default key
    "unknown".to_string()
}

/// Axum middleware for rate limiting.
pub async fn rate_limit_middleware(
    axum::extract::Extension(limiter): axum::extract::Extension<RateLimiter>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let key = extract_client_key(&headers);

    if limiter.allow(&key).await {
        next.run(request).await
    } else {
        (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(serde_json::json!({
                "error": "rate limit exceeded",
                "message": "Too many requests. Please try again later.",
            })),
        )
            .into_response()
    }
}
