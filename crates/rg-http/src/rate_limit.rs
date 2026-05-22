//! Simple token-bucket rate limiter middleware for Axum.
//!
//! Limits requests per IP address. Configurable requests-per-minute.
//! Returns 429 Too Many Requests when the limit is exceeded.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::Request;
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

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
    /// std::sync::Mutex is used because critical sections are very short
    /// (single HashMap lookup/update) and never await.
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
    fn allow(&self, key: &str) -> bool {
        let mut clients = match self.clients.lock() {
            Ok(guard) => guard,
            // If the mutex is poisoned, reset the map and continue
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.clear();
                return true;
            }
        };
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

    /// Clean up expired entries. Called periodically by the background task.
    fn cleanup(&self) {
        let mut clients = match self.clients.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.clear();
                return;
            }
        };
        let now = Instant::now();
        clients.retain(|_, state| now < state.reset_at);
    }

    /// Spawn a background task that periodically cleans up expired entries.
    pub fn spawn_cleanup_task(&self) {
        let limiter = self.clone();
        // Cleanup interval: half the window duration, min 60s, max 600s
        let interval_secs = (self.window_secs / 2).clamp(60, 600);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                limiter.cleanup();
            }
        });
    }
}

/// Extract client IP from headers (X-Forwarded-For, X-Real-IP).
/// Returns `None` if no identifying header is present.
fn extract_client_key(headers: &HeaderMap) -> Option<String> {
    // Try X-Forwarded-For first (first IP in the list)
    if let Some(xff) = headers.get("x-forwarded-for") {
        if let Ok(val) = xff.to_str() {
            if let Some(ip) = val.split(',').next() {
                let ip = ip.trim();
                if !ip.is_empty() {
                    return Some(ip.to_string());
                }
            }
        }
    }

    // Try X-Real-IP
    if let Some(xri) = headers.get("x-real-ip") {
        if let Ok(val) = xri.to_str() {
            let val = val.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }

    None
}

/// Axum middleware for rate limiting.
pub async fn rate_limit_middleware(
    axum::extract::State(limiter): axum::extract::State<RateLimiter>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let key = match extract_client_key(&headers) {
        Some(k) => k,
        // Skip rate limiting if we cannot identify the client.
        // This avoids having all unidentified clients share a single "unknown" bucket.
        None => return next.run(request).await,
    };

    if limiter.allow(&key) {
        next.run(request).await
    } else {
        crate::error::AppError::rate_limited("Too many requests. Please try again later.")
            .into_response()
    }
}
