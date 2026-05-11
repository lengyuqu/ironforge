//! Shared HTTP middleware for IronForge.

use axum::extract::Request;
use axum::http::HeaderName;
use axum::middleware::Next;
use axum::response::Response;

/// Per-request unique ID, generated from `X-Request-Id` header or a fresh UUID.
#[derive(Clone, Debug)]
pub struct RequestId(pub String);

static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Middleware that generates (or propagates) a per-request unique ID.
///
/// - If the client sends `X-Request-Id`, that value is reused.
/// - Otherwise, a fresh v4 UUID is generated.
/// - The ID is stored in request extensions and added to the response header.
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

    // Store in extensions so AppError can pick it up for error responses
    request
        .extensions_mut()
        .insert(RequestId(request_id.clone()));

    let mut response = next.run(request).await;

    // Add X-Request-Id to response
    if let Ok(val) = request_id.parse() {
        response.headers_mut().insert(X_REQUEST_ID.clone(), val);
    }

    response
}
