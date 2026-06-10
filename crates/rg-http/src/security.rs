//! Security headers middleware for IronForge.
//!
//! Phase 22-D: Adds defense-in-depth HTTP security headers to all responses.
//! These headers protect against common web vulnerabilities (XSS, clickjacking,
//! MIME sniffing, etc.) without affecting functionality.

use axum::extract::Request;
use axum::http::{header, HeaderValue, Uri};
use axum::middleware::Next;
use axum::response::Response;

/// Middleware that adds security headers to all responses.
///
/// Headers added (Phase 22-D):
/// - `X-Content-Type-Options: nosniff` — prevent MIME sniffing
/// - `X-Frame-Options: DENY` — prevent clickjacking
/// - `X-XSS-Protection: 0` — disable legacy XSS filter (modern browsers deprecated it)
/// - `Referrer-Policy: strict-origin-when-cross-origin` — limit referrer info
/// - `Strict-Transport-Security` — HSTS (only added if request is HTTPS)
/// - `Content-Security-Policy` — restrict resource loading
/// - `Permissions-Policy` — disable unused browser features
/// - `Cross-Origin-Opener-Policy: same-origin`
/// - `Cross-Origin-Resource-Policy: same-origin`
pub async fn security_headers_middleware(request: Request, next: Next) -> Response {
    let is_https = is_https_uri(request.uri())
        || request
            .headers()
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "https")
            .unwrap_or(false);

    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    // Prevent MIME type sniffing
    headers.insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );

    // Prevent clickjacking
    headers.insert(
        header::HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );

    // Disable legacy XSS filter (modern browsers handle this via CSP)
    headers.insert(
        header::HeaderName::from_static("x-xss-protection"),
        HeaderValue::from_static("0"),
    );

    // Limit referrer information leakage
    headers.insert(
        header::HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );

    // HSTS — only over HTTPS
    if is_https {
        headers.insert(
            header::HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_static("max-age=31536000; includeSubDomains; preload"),
        );
    }

    // Content Security Policy — restrictive default
    // Allow self for scripts/styles/images; allow data: for images; inline styles OK
    // This is a baseline — adjust as needed for specific features
    headers.insert(
        header::HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'self'; \
             script-src 'self' 'unsafe-inline'; \
             style-src 'self' 'unsafe-inline'; \
             img-src 'self' data: https:; \
             font-src 'self' data:; \
             connect-src 'self'; \
             frame-ancestors 'none'; \
             base-uri 'self'; \
             form-action 'self'",
        ),
    );

    // Permissions Policy — disable unused browser features
    headers.insert(
        header::HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static(
            "geolocation=(), microphone=(), camera=(), payment=(), usb=(), magnetometer=(), gyroscope=(), accelerometer=()",
        ),
    );

    // Cross-Origin policies for isolation
    headers.insert(
        header::HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        header::HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("same-origin"),
    );

    response
}

/// Check if the request URI scheme is HTTPS.
fn is_https_uri(uri: &Uri) -> bool {
    uri.scheme().map(|s| s == "https").unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::middleware::from_fn;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    async fn dummy_handler() -> &'static str {
        "ok"
    }

    #[tokio::test]
    async fn test_security_headers_added() {
        let app = Router::new()
            .route("/", get(dummy_handler))
            .layer(from_fn(security_headers_middleware));

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let headers = response.headers();
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(headers.get("x-frame-options").unwrap(), "DENY");
        assert_eq!(headers.get("x-xss-protection").unwrap(), "0");
        assert_eq!(
            headers.get("referrer-policy").unwrap(),
            "strict-origin-when-cross-origin"
        );
        assert!(headers.get("content-security-policy").is_some());
        assert!(headers.get("permissions-policy").is_some());
        assert!(headers.get("cross-origin-opener-policy").is_some());
    }

    #[tokio::test]
    async fn test_hsts_only_on_https() {
        let app = Router::new()
            .route("/", get(dummy_handler))
            .layer(from_fn(security_headers_middleware));

        // HTTP request — no HSTS
        let response = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(response.headers().get("strict-transport-security").is_none());

        // HTTPS request — has HSTS
        let response = app
            .oneshot(
                Request::builder()
                    .uri("https://example.com/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.headers().get("strict-transport-security").is_some());
    }
}
