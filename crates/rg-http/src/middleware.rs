//! Shared HTTP middleware for IronForge.

use axum::body::{to_bytes, Body};
use axum::extract::{MatchedPath, Request};
use axum::http::Method;
use axum::http::{header, HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use std::time::Instant;

/// Per-request unique ID, generated from `X-Request-Id` header or a fresh UUID.
#[derive(Clone, Debug)]
pub struct RequestId(pub String);

static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// HTTP metrics middleware - records request count, duration, and in-flight count.
///
/// Uses `MatchedPath` to normalize route templates like `/users/{id}` instead of
/// concrete paths like `/users/123` to avoid label cardinality explosion.
pub async fn http_metrics_middleware(request: Request, next: Next) -> Response {
    use crate::metrics::http_requests::{IN_FLIGHT, REQUEST_COUNT, REQUEST_DURATION};

    let start = Instant::now();

    // Increment in-flight counter
    if let Some(g) = IN_FLIGHT.get() {
        g.inc();
    }

    let method = request.method().clone();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let response = next.run(request).await;

    let elapsed = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    // Decrement in-flight counter
    if let Some(g) = IN_FLIGHT.get() {
        g.dec();
    }

    // Record metrics
    if let Some(c) = REQUEST_COUNT.get() {
        c.with_label_values(&[method.as_str(), &route, &status])
            .inc();
    }
    if let Some(h) = REQUEST_DURATION.get() {
        h.observe(elapsed);
    }

    response
}

/// Middleware that generates (or propagates) a per-request unique ID.
///
/// - If the client sends `X-Request-Id`, that value is reused.
/// - Otherwise, a fresh v4 UUID is generated.
/// - The ID is stored in request extensions and added to the response header.
/// - For 4xx/5xx JSON error responses, the request_id is also injected into the body.
pub async fn request_id_middleware(mut request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get(&X_REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let request_id = if request_id.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        request_id.to_string()
    };

    // Store in extensions so handlers can access it if needed
    request
        .extensions_mut()
        .insert(RequestId(request_id.clone()));

    let mut response = next.run(request).await;

    // Add X-Request-Id to response header
    if let Ok(val) = request_id.parse() {
        response.headers_mut().insert(X_REQUEST_ID.clone(), val);
    }

    // Scrub server paths from every error body, then (for JSON) add the
    // request id. Done here, once, because error text is produced in ~100
    // handlers plus OCI/git responses that bypass `AppError`.
    if response.status().is_client_error() || response.status().is_server_error() {
        return scrub_error_response(response, &request_id).await;
    }

    response
}

/// Largest error body that is buffered for scrubbing. Real error bodies are a
/// line or two; anything bigger (or of unknown length) is left untouched rather
/// than buffered.
const MAX_SCRUBBED_BODY: u64 = 1024 * 1024;

fn is_textual_response(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.starts_with("text/") || ct.starts_with("application/json"))
        .unwrap_or(false)
}

/// Redact server filesystem paths from a 4xx/5xx text or JSON body, and add the
/// request id to JSON error bodies.
async fn scrub_error_response(response: Response, request_id: &str) -> Response {
    use axum::body::HttpBody;

    if !is_textual_response(&response) {
        return response;
    }
    let bounded = response
        .body()
        .size_hint()
        .upper()
        .is_some_and(|upper| upper <= MAX_SCRUBBED_BODY);
    if !bounded {
        return if is_json_response(&response) {
            inject_request_id(response, request_id).await
        } else {
            response
        };
    }

    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, MAX_SCRUBBED_BODY as usize).await {
        Ok(bytes) => bytes,
        Err(_) => return Response::from_parts(parts, Body::empty()),
    };
    let scrubbed: Vec<u8> = match std::str::from_utf8(&bytes) {
        Ok(text) => match crate::redact::redact_server_paths(text) {
            std::borrow::Cow::Borrowed(_) => bytes.to_vec(),
            std::borrow::Cow::Owned(redacted) => redacted.into_bytes(),
        },
        Err(_) => bytes.to_vec(),
    };
    parts.headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&scrubbed.len().to_string()).unwrap_or_else(|_| HeaderValue::from(0)),
    );
    let response = Response::from_parts(parts, Body::from(scrubbed));
    if is_json_response(&response) {
        inject_request_id(response, request_id).await
    } else {
        response
    }
}

fn is_json_response(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.starts_with("application/json"))
        .unwrap_or(false)
}

/// Inject request_id into a JSON response body. Returns the (possibly modified) response.
/// If body reading or JSON parsing fails, returns the response with the original body preserved
/// when possible, or empty body on stream errors.
async fn inject_request_id(response: Response, request_id: &str) -> Response {
    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, usize::MAX).await {
        Ok(b) => b,
        Err(_) => return Response::from_parts(parts, Body::empty()),
    };

    let mut json: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            // Not valid JSON, restore original body
            let body = Body::from(bytes);
            return Response::from_parts(parts, body);
        }
    };

    // Inject request_id into error body if present
    if let Some(error_obj) = json.get_mut("error") {
        if let Some(obj) = error_obj.as_object_mut() {
            obj.insert(
                "request_id".to_string(),
                serde_json::Value::String(request_id.to_string()),
            );
        }
    }

    let modified = serde_json::to_vec(&json).unwrap_or_else(|_| bytes.to_vec());
    parts.headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&modified.len().to_string()).unwrap_or_else(|_| HeaderValue::from(0)),
    );
    Response::from_parts(parts, Body::from(modified))
}

/// Maintenance mode middleware — rejects mutating requests when the instance
/// is in read-only maintenance mode. Safe methods (GET, HEAD, OPTIONS) and
/// the admin panel are always allowed.
///
/// Two things this must get right:
///
/// 1. **Status code.** Maintenance is a temporary, server-imposed condition, so
///    the response is `503 Service Unavailable` with `Retry-After` — not 200.
///    Returning 200 made clients treat the rejection as success.
/// 2. **Git fetch must keep working.** `POST /git/<owner>/<repo>/git-upload-pack`
///    is a read-only operation that happens to use POST, and git clients always
///    use it. Blocking it would break every clone/fetch during maintenance,
///    while the matching `git-receive-pack` (the actual write) stays blocked.
pub async fn maintenance_middleware(request: Request, next: Next) -> Response {
    let settings = crate::instance::get_settings();
    if settings.maintenance_mode {
        let method = request.method();
        let path = request.uri().path();

        let is_safe_method =
            method == Method::GET || method == Method::HEAD || method == Method::OPTIONS;
        let is_admin = path.starts_with("/api/v1/admin/");
        // Read-only git transport: upload-pack negotiates and sends objects, it
        // never writes. `info/refs` is a GET and already allowed above.
        let is_git_read = path.ends_with("/git-upload-pack");

        if !is_safe_method && !is_admin && !is_git_read {
            let mut response = Json(serde_json::json!({
                "error": "Instance is in maintenance mode. Read-only access only.",
                "code": "MAINTENANCE_MODE"
            }))
            .into_response();
            *response.status_mut() = axum::http::StatusCode::SERVICE_UNAVAILABLE;
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_static("300"),
            );
            return response;
        }
    }
    next.run(request).await
}
