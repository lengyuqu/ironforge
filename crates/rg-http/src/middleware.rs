//! Shared HTTP middleware for IronForge.

use axum::body::{to_bytes, Body};
use axum::extract::Request;
use axum::http::{header, HeaderName, HeaderValue};
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

    // Inject request_id into JSON error response bodies
    if response.status().is_client_error() || response.status().is_server_error() {
        if is_json_response(&response) {
            return inject_request_id(response, &request_id).await;
        }
    }

    response
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
            obj.insert("request_id".to_string(), serde_json::Value::String(request_id.to_string()));
        }
    }

    let modified = serde_json::to_vec(&json).unwrap_or_else(|_| bytes.to_vec());
    parts.headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&modified.len().to_string()).unwrap_or_else(|_| HeaderValue::from(0)),
    );
    Response::from_parts(parts, Body::from(modified))
}
