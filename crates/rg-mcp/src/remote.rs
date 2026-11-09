//! MCP over **streamable HTTP** (`ironforge-mcp --http`).
//!
//! For agents that cannot start a local `ironforge-mcp` subprocess and are not
//! on the machine that runs the IronForge server. One endpoint, `/mcp`:
//!
//! | request                              | meaning                                        |
//! |--------------------------------------|------------------------------------------------|
//! | `POST /mcp` `initialize`             | authenticate, open an MCP session, return its id in `Mcp-Session-Id` |
//! | `POST /mcp` anything else            | needs `Mcp-Session-Id` and the same token      |
//! | `DELETE /mcp`                        | end the session                                |
//! | `GET /mcp`                           | 405: the server never starts messages of its own |
//! | `GET /healthz`                       | liveness for a load balancer (no information)  |
//!
//! # Where authority lives
//!
//! This process holds **no** credentials and makes **no** decisions about what a
//! token may do. Every tool call is forwarded to the IronForge REST API with the
//! caller's own `Authorization: Bearer <token>`, so the scope ladder, the
//! repository and branch allow-lists, the idempotency keys and the audit trail
//! of the server apply exactly as for a direct REST call; there is no second
//! check here that could disagree with it. The only things decided here are:
//!
//! * a request without a bearer token is refused (HTTP 401) before any work;
//! * `initialize` probes the token once against the REST API (`GET /users/me`:
//!   401 = unknown or expired token, refused; 200 or 403 = a real token, 403
//!   only meaning it has no `user` scope, which agent tokens do not) so garbage
//!   tokens cannot fill the session table;
//! * a session is bound to the SHA-256 of the token that opened it. Any other
//!   token presenting that session id gets the same 404 as an unknown id;
//! * limits: sessions in total and per token, idle expiry, calls in flight in
//!   total and per session, request size, an `Origin` allow-list.
//!
//! The upstream address is operator configuration (`IRONFORGE_URL`), never taken
//! from a request, so this is not an open proxy.
//!
//! # Not covered
//!
//! No server-initiated messages (no `GET` stream), no resumable streams, no
//! legacy HTTP+SSE transport (`GET /sse` + `/messages`), no TLS (terminate it in
//! the reverse proxy), state is in this process only (restart = sessions gone,
//! clients get 404 and re-initialize, as the spec says).

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::collab::SessionStore;
use crate::protocol::{make_error, JsonRpcRequest};
use crate::AppState;

const SESSION_HEADER: HeaderName = HeaderName::from_static("mcp-session-id");
const PROTOCOL_HEADER: &str = "mcp-protocol-version";

/// Protocol versions this endpoint speaks, newest first (the first is offered
/// when the client asks for something else).
pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// Default for [`RemoteConfig::heartbeat`]: how often an SSE response sends a
/// comment line while a tool call is running, so a proxy or load balancer with
/// an idle timeout keeps the connection.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

/// Operator configuration of the HTTP transport.
#[derive(Clone, Debug)]
pub struct RemoteConfig {
    /// Base URL of the IronForge server, e.g. `http://127.0.0.1:8080`.
    pub api_base: String,
    /// Browser origins allowed to call the endpoint (`*` = any). Requests with
    /// no `Origin` header (every non-browser agent) are always fine.
    pub allowed_origins: Vec<String>,
    pub max_sessions: usize,
    pub max_sessions_per_token: usize,
    pub idle_ttl: Duration,
    pub max_inflight: usize,
    pub max_inflight_per_session: usize,
    /// Concurrent token probes at `initialize`.
    pub max_probes: usize,
    pub max_body_bytes: usize,
    /// How often an SSE response sends a comment line while a tool call runs.
    pub heartbeat: Duration,
}

impl RemoteConfig {
    pub fn new(api_base: impl Into<String>) -> Self {
        Self {
            api_base: api_base.into(),
            allowed_origins: Vec::new(),
            max_sessions: 1000,
            max_sessions_per_token: 16,
            idle_ttl: Duration::from_secs(30 * 60),
            max_inflight: 64,
            max_inflight_per_session: 4,
            max_probes: 16,
            max_body_bytes: 4 * 1024 * 1024,
            heartbeat: HEARTBEAT_INTERVAL,
        }
    }

    /// `IRONFORGE_URL` (default `http://localhost:8080`) and the optional
    /// `IRONFORGE_MCP_ALLOWED_ORIGINS` (comma separated),
    /// `IRONFORGE_MCP_MAX_SESSIONS`, `IRONFORGE_MCP_MAX_SESSIONS_PER_TOKEN`,
    /// `IRONFORGE_MCP_IDLE_SECS`, `IRONFORGE_MCP_MAX_INFLIGHT`.
    pub fn from_env() -> Self {
        let mut cfg = Self::new(
            std::env::var("IRONFORGE_URL").unwrap_or_else(|_| "http://localhost:8080".into()),
        );
        if let Ok(list) = std::env::var("IRONFORGE_MCP_ALLOWED_ORIGINS") {
            cfg.allowed_origins = list
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        fn number(name: &str) -> Option<usize> {
            std::env::var(name)
                .ok()?
                .trim()
                .parse()
                .ok()
                .filter(|n| *n > 0)
        }
        if let Some(n) = number("IRONFORGE_MCP_MAX_SESSIONS") {
            cfg.max_sessions = n;
        }
        if let Some(n) = number("IRONFORGE_MCP_MAX_SESSIONS_PER_TOKEN") {
            cfg.max_sessions_per_token = n;
        }
        if let Some(n) = number("IRONFORGE_MCP_IDLE_SECS") {
            cfg.idle_ttl = Duration::from_secs(n as u64);
        }
        if let Some(n) = number("IRONFORGE_MCP_MAX_INFLIGHT") {
            cfg.max_inflight = n;
        }
        cfg
    }
}

// ── session table ────────────────────────────────────────────────────

struct Session {
    digest: [u8; 32],
    state: AppState,
    last_used: Instant,
    inflight: Arc<Semaphore>,
}

fn digest_of(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// Compares without stopping at the first difference.
fn same_digest(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

struct Shared {
    cfg: RemoteConfig,
    sessions: Mutex<HashMap<String, Session>>,
    global: Arc<Semaphore>,
    probes: Semaphore,
    http: reqwest::Client,
}

impl Shared {
    fn sessions(&self) -> std::sync::MutexGuard<'_, HashMap<String, Session>> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The session, if `digest` is the token that opened it. An unknown id and
    /// someone else's id look the same.
    fn lookup(&self, id: &str, digest: &[u8; 32]) -> Option<(AppState, Arc<Semaphore>)> {
        let mut map = self.sessions();
        let session = map.get_mut(id)?;
        if !same_digest(&session.digest, digest) {
            return None;
        }
        session.last_used = Instant::now();
        Some((session.state.clone(), session.inflight.clone()))
    }

    fn touch(&self, id: &str) {
        if let Some(s) = self.sessions().get_mut(id) {
            s.last_used = Instant::now();
        }
    }

    fn remove(&self, id: &str, digest: &[u8; 32]) -> bool {
        let mut map = self.sessions();
        match map.get(id) {
            Some(s) if same_digest(&s.digest, digest) => {
                map.remove(id);
                true
            }
            _ => false,
        }
    }

    /// Drop sessions idle longer than the TTL. A session with a call still
    /// running is not idle.
    fn purge(&self) -> usize {
        let ttl = self.cfg.idle_ttl;
        let per_session = self.cfg.max_inflight_per_session;
        let mut map = self.sessions();
        let before = map.len();
        map.retain(|_, s| {
            s.inflight.available_permits() < per_session || s.last_used.elapsed() < ttl
        });
        before - map.len()
    }

    /// Probe the token against the REST API. See the module documentation.
    async fn probe(&self, token: &str) -> Probe {
        let url = format!(
            "{}/api/v1/users/me",
            self.cfg.api_base.trim_end_matches('/')
        );
        match self.http.get(url).bearer_auth(token).send().await {
            Ok(resp) => match resp.status().as_u16() {
                200 | 403 => Probe::Valid,
                401 => Probe::Invalid,
                _ => Probe::Unavailable,
            },
            Err(_) => Probe::Unavailable,
        }
    }

    async fn initialize(&self, token: String, digest: [u8; 32], message: &Value) -> Response {
        let id = match message.get("id") {
            Some(id) if !id.is_null() => id.clone(),
            _ => return rpc_error(StatusCode::BAD_REQUEST, -32600, "initialize needs an id"),
        };
        let version = message
            .pointer("/params/protocolVersion")
            .and_then(Value::as_str)
            .filter(|v| SUPPORTED_PROTOCOL_VERSIONS.contains(v))
            .unwrap_or(SUPPORTED_PROTOCOL_VERSIONS[0]);

        let Ok(_probe_slot) = self.probes.try_acquire() else {
            return busy("too many sign-ins in progress");
        };
        match self.probe(&token).await {
            Probe::Valid => {}
            Probe::Invalid => return unauthorized("the token is not valid"),
            Probe::Unavailable => {
                return rpc_error(
                    StatusCode::BAD_GATEWAY,
                    -32603,
                    "the IronForge server could not be reached to check the token",
                )
            }
        }

        let session_id = uuid::Uuid::new_v4().to_string();
        {
            let mut map = self.sessions();
            let ttl = self.cfg.idle_ttl;
            let per_session = self.cfg.max_inflight_per_session;
            map.retain(|_, s| {
                s.inflight.available_permits() < per_session || s.last_used.elapsed() < ttl
            });
            if map.len() >= self.cfg.max_sessions {
                return busy("the server has reached its session limit");
            }
            let mine = map
                .values()
                .filter(|s| same_digest(&s.digest, &digest))
                .count();
            if mine >= self.cfg.max_sessions_per_token {
                return busy("this token has too many open sessions; end one with DELETE /mcp");
            }
            map.insert(
                session_id.clone(),
                Session {
                    digest,
                    state: AppState {
                        api_base: self.cfg.api_base.clone(),
                        pat: token,
                        // Memory only and private to this session: never the
                        // process-wide `IRONFORGE_COLLAB_SESSION_FILE`, which
                        // would let one token resume another's collaboration
                        // credentials.
                        collab: Arc::new(SessionStore::in_memory()),
                    },
                    last_used: Instant::now(),
                    inflight: Arc::new(Semaphore::new(self.cfg.max_inflight_per_session)),
                },
            );
        }
        tracing::info!(
            "mcp session opened ({})",
            &session_id[..8.min(session_id.len())]
        );

        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id,
            method: "initialize".into(),
            params: None,
        };
        let body = crate::server::handle_initialize(&request, version);
        let mut response = Json(body).into_response();
        if let Ok(value) = HeaderValue::from_str(&session_id) {
            response.headers_mut().insert(SESSION_HEADER, value);
        }
        response
    }
}

enum Probe {
    Valid,
    Invalid,
    Unavailable,
}

// ── responses ────────────────────────────────────────────────────────

fn rpc_error(status: StatusCode, code: i32, message: &str) -> Response {
    (status, Json(make_error(Value::Null, code, message))).into_response()
}

fn unauthorized(message: &str) -> Response {
    let mut response = rpc_error(StatusCode::UNAUTHORIZED, -32001, message);
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer realm=\"ironforge-mcp\""),
    );
    response
}

fn busy(message: &str) -> Response {
    let mut response = rpc_error(StatusCode::TOO_MANY_REQUESTS, -32002, message);
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    response
}

fn session_not_found() -> Response {
    rpc_error(
        StatusCode::NOT_FOUND,
        -32003,
        "unknown or expired session: send initialize again",
    )
}

// ── request handling ─────────────────────────────────────────────────

/// `Authorization: Bearer <token>`: 1-512 visible ASCII characters.
fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    let valid = scheme.eq_ignore_ascii_case("bearer")
        && !token.is_empty()
        && token.len() <= 512
        && token.bytes().all(|b| (0x21..=0x7e).contains(&b));
    valid.then(|| token.to_string())
}

fn origin_allowed(cfg: &RemoteConfig, headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    cfg.allowed_origins
        .iter()
        .any(|o| o == "*" || o.eq_ignore_ascii_case(origin))
}

fn accepts_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::ACCEPT)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|v| v.contains("text/event-stream"))
}

/// One message of a POST body, classified.
enum Item {
    /// A request: has a method and an id, wants an answer.
    Request(JsonRpcRequest),
    /// Malformed: answered with this error.
    Invalid(Value),
    /// A notification, or the client's answer to a request: nothing to send back.
    Silent,
}

fn classify(message: Value) -> Item {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str);
    match (method, id) {
        (Some("initialize"), _) => Item::Invalid(
            serde_json::to_value(make_error(
                message.get("id").cloned().unwrap_or(Value::Null),
                -32600,
                "initialize must be sent alone, once, to open a session",
            ))
            .unwrap_or(Value::Null),
        ),
        (Some(method), Some(id)) if !id.is_null() => Item::Request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id,
            method: method.to_string(),
            params: message.get("params").cloned(),
        }),
        // A notification: a method and no id.
        (Some(_), None) => Item::Silent,
        // The client answering a request: has result or error and no method.
        (None, _) if message.get("result").is_some() || message.get("error").is_some() => {
            Item::Silent
        }
        _ => Item::Invalid(
            serde_json::to_value(make_error(
                message.get("id").cloned().unwrap_or(Value::Null),
                -32600,
                "not a JSON-RPC request or notification",
            ))
            .unwrap_or(Value::Null),
        ),
    }
}

/// Run the requests of one POST on a blocking thread (the tools call the REST
/// API with `Handle::block_on`, as under stdio) and return the answers in order.
fn run_items(state: &AppState, items: Vec<Item>) -> Vec<Value> {
    items
        .into_iter()
        .filter_map(|item| match item {
            Item::Request(request) => {
                serde_json::to_value(crate::server::dispatch(state, &request)).ok()
            }
            Item::Invalid(error) => Some(error),
            Item::Silent => None,
        })
        .collect()
}

async fn post_mcp(State(shared): State<Arc<Shared>>, headers: HeaderMap, body: Bytes) -> Response {
    if !origin_allowed(&shared.cfg, &headers) {
        return rpc_error(StatusCode::FORBIDDEN, -32600, "origin not allowed");
    }
    let Some(token) = bearer(&headers) else {
        return unauthorized("an Authorization: Bearer <token> header is required");
    };
    let digest = digest_of(&token);

    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return rpc_error(
                StatusCode::BAD_REQUEST,
                -32700,
                &format!("parse error: {e}"),
            );
        }
    };

    if parsed.get("method").and_then(Value::as_str) == Some("initialize") {
        return shared.initialize(token, digest, &parsed).await;
    }

    let Some(session_id) = headers
        .get(&SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
    else {
        return rpc_error(
            StatusCode::BAD_REQUEST,
            -32600,
            "the Mcp-Session-Id header is required: send initialize first",
        );
    };
    if let Some(version) = headers.get(PROTOCOL_HEADER) {
        if !version
            .to_str()
            .is_ok_and(|v| SUPPORTED_PROTOCOL_VERSIONS.contains(&v))
        {
            return rpc_error(
                StatusCode::BAD_REQUEST,
                -32600,
                "unsupported MCP-Protocol-Version",
            );
        }
    }
    let Some((state, per_session)) = shared.lookup(&session_id, &digest) else {
        return session_not_found();
    };

    let (messages, batched) = match parsed {
        Value::Array(list) if list.is_empty() => {
            return rpc_error(StatusCode::BAD_REQUEST, -32600, "empty batch");
        }
        Value::Array(list) => (list, true),
        single => (vec![single], false),
    };
    let items: Vec<Item> = messages.into_iter().map(classify).collect();
    if items.iter().all(|i| matches!(i, Item::Silent)) {
        return StatusCode::ACCEPTED.into_response();
    }

    // Both permits are held until the blocking call has really ended, also when
    // the client has already gone, so the caps count work that is running.
    let (Ok(global_permit), Ok(session_permit)) = (
        shared.global.clone().try_acquire_owned(),
        per_session.try_acquire_owned(),
    ) else {
        return busy("too many calls in progress");
    };

    let ids: Vec<Value> = items
        .iter()
        .filter_map(|i| match i {
            Item::Request(r) => Some(r.id.clone()),
            _ => None,
        })
        .collect();
    let stream_it = !batched
        && accepts_event_stream(&headers)
        && matches!(&items[0], Item::Request(r) if r.method == "tools/call");

    let work = tokio::task::spawn_blocking(move || {
        let _permits: (OwnedSemaphorePermit, OwnedSemaphorePermit) =
            (global_permit, session_permit);
        run_items(&state, items)
    });
    let weak: Weak<Shared> = Arc::downgrade(&shared);
    let finish = async move {
        let answers = match work.await {
            Ok(answers) => answers,
            Err(_) => ids
                .iter()
                .filter_map(|id| {
                    serde_json::to_value(make_error(id.clone(), -32603, "internal error")).ok()
                })
                .collect(),
        };
        if let Some(shared) = weak.upgrade() {
            shared.touch(&session_id);
        }
        answers
    };

    if stream_it {
        let heartbeat = shared.cfg.heartbeat;
        // The answer comes as one SSE event; while the tool waits (up to 55 s for
        // the wait_* tools) comment lines keep idle-timeout proxies from closing it.
        let stream = futures::stream::once(async move {
            let answers = finish.await;
            let data = answers
                .into_iter()
                .next()
                .unwrap_or(Value::Null)
                .to_string();
            Ok::<Event, Infallible>(Event::default().event("message").data(data))
        });
        let mut response = Sse::new(stream)
            .keep_alive(KeepAlive::new().interval(heartbeat).text("heartbeat"))
            .into_response();
        // Tell nginx-style proxies not to buffer this response.
        response
            .headers_mut()
            .insert("x-accel-buffering", HeaderValue::from_static("no"));
        return response;
    }

    let mut answers = finish.await;
    if batched {
        Json(Value::Array(answers)).into_response()
    } else {
        match answers.pop() {
            Some(answer) => Json(answer).into_response(),
            None => StatusCode::ACCEPTED.into_response(),
        }
    }
}

async fn delete_mcp(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    if !origin_allowed(&shared.cfg, &headers) {
        return rpc_error(StatusCode::FORBIDDEN, -32600, "origin not allowed");
    }
    let Some(token) = bearer(&headers) else {
        return unauthorized("an Authorization: Bearer <token> header is required");
    };
    let Some(session_id) = headers.get(&SESSION_HEADER).and_then(|v| v.to_str().ok()) else {
        return rpc_error(
            StatusCode::BAD_REQUEST,
            -32600,
            "the Mcp-Session-Id header is required",
        );
    };
    if shared.remove(session_id, &digest_of(&token)) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        session_not_found()
    }
}

async fn get_mcp() -> Response {
    let mut response = StatusCode::METHOD_NOT_ALLOWED.into_response();
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("POST, DELETE"));
    response
}

async fn healthz() -> &'static str {
    "ok"
}

// ── assembly ─────────────────────────────────────────────────────────

/// The router of the HTTP transport. Must be called inside a Tokio runtime (it
/// starts the task that expires idle sessions).
pub fn router(cfg: RemoteConfig) -> Router {
    let body_limit = cfg.max_body_bytes;
    let shared = Arc::new(Shared {
        global: Arc::new(Semaphore::new(cfg.max_inflight)),
        probes: Semaphore::new(cfg.max_probes),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest::Client::build() failed: no native TLS backend available"),
        sessions: Mutex::new(HashMap::new()),
        cfg,
    });

    let weak = Arc::downgrade(&shared);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            let Some(shared) = weak.upgrade() else { return };
            let dropped = shared.purge();
            if dropped > 0 {
                tracing::info!("mcp: expired {dropped} idle session(s)");
            }
        }
    });

    Router::new()
        .route("/mcp", post(post_mcp).delete(delete_mcp).get(get_mcp))
        .route("/healthz", get(healthz))
        .layer(DefaultBodyLimit::max(body_limit))
        .with_state(shared)
}

/// Serve the transport on `listener` until the process ends.
pub async fn serve(listener: tokio::net::TcpListener, cfg: RemoteConfig) -> std::io::Result<()> {
    axum::serve(listener, router(cfg)).await
}
