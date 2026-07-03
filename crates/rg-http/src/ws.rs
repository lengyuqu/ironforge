//! WebSocket real-time notification push.
//!
//! Clients connect to `ws://host/api/v1/ws/notifications?token=<jwt>`
//! and receive real-time notification events as JSON messages.
//!
//! Security: per-user channels ensure each client only receives their own
//! notifications. Job-log events use a separate global channel (public).

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, Query, State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::AppState;

/// The broadcast channel capacity for real-time notifications.
const NOTIFICATION_CHANNEL_CAPACITY: usize = 256;

/// A notification event sent over WebSocket.
#[derive(Debug, Clone, Serialize)]
pub struct NotificationEvent {
    pub event_type: String,
    pub data: serde_json::Value,
}

/// Internal state for the notification hub.
#[derive(Debug)]
struct NotificationHubInner {
    /// Per-user notification channels. Only the owning user receives
    /// messages pushed via `push_notification`.
    user_channels: RwLock<HashMap<i64, broadcast::Sender<NotificationEvent>>>,
    /// Global channel for public events (e.g. job_log) that all
    /// connected clients should receive.
    global_sender: broadcast::Sender<NotificationEvent>,
}

/// Global notification hub with per-user isolation.
///
/// Wrapped in `Arc` so it can be cheaply cloned into `AppState`.
#[derive(Debug, Clone)]
pub struct NotificationHub {
    inner: Arc<NotificationHubInner>,
}

impl Default for NotificationHub {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationHub {
    /// Create a new notification hub.
    pub fn new() -> Self {
        let (global_sender, _) = broadcast::channel(NOTIFICATION_CHANNEL_CAPACITY);
        Self {
            inner: Arc::new(NotificationHubInner {
                user_channels: RwLock::new(HashMap::new()),
                global_sender,
            }),
        }
    }

    /// Push a notification to a **specific user's** channel.
    /// Creates the channel if it doesn't exist yet.
    pub async fn push_notification(
        &self,
        user_id: i64,
        event_type: &str,
        data: serde_json::Value,
    ) {
        let event = NotificationEvent {
            event_type: event_type.to_string(),
            data: serde_json::json!({
                "user_id": user_id,
                "payload": data,
            }),
        };

        let channels = self.inner.user_channels.read().await;
        if let Some(sender) = channels.get(&user_id) {
            let _ = sender.send(event);
        }
        // If the user has no active channel, the notification is silently
        // dropped. The REST API /notifications endpoint will still serve
        // persisted notifications when the user comes online.
    }

    /// Subscribe to a specific user's notification channel.
    /// Creates the channel if it doesn't exist.
    async fn subscribe_user(&self, user_id: i64) -> broadcast::Receiver<NotificationEvent> {
        let mut channels = self.inner.user_channels.write().await;
        let sender = channels
            .entry(user_id)
            .or_insert_with(|| broadcast::channel(NOTIFICATION_CHANNEL_CAPACITY).0);
        sender.subscribe()
    }

    /// Subscribe to the global event channel (job logs, etc.).
    fn subscribe_global(&self) -> broadcast::Receiver<NotificationEvent> {
        self.inner.global_sender.subscribe()
    }

    /// Broadcast a job log update to **all** WebSocket subscribers.
    ///
    /// Frontend clients can listen for `job_log` events and filter by job_id.
    pub fn push_job_log(&self, job_id: i64, log: &str) {
        let event = NotificationEvent {
            event_type: "job_log".to_string(),
            data: serde_json::json!({
                "job_id": job_id,
                "log": log,
            }),
        };
        let _ = self.inner.global_sender.send(event);
    }
}

/// Query params for WebSocket upgrade.
#[derive(Deserialize)]
pub struct WsQuery {
    /// JWT token for authentication.
    token: Option<String>,
}

/// GET /api/v1/ws/notifications — WebSocket upgrade handler.
pub async fn ws_notifications_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // Authenticate via JWT token in query string
    let user_id = query
        .token
        .as_deref()
        .and_then(|t| rg_core::auth::jwt::validate_token(t, &state.jwt_secret))
        .and_then(|c| c.sub.parse::<i64>().ok());

    ws.on_upgrade(move |socket| {
        handle_ws_connection(socket, state.notification_hub.clone(), user_id)
    })
}

/// Handle an individual WebSocket connection.
async fn handle_ws_connection(socket: WebSocket, hub: NotificationHub, user_id: Option<i64>) {
    let (mut sender, mut receiver) = socket.split();

    if user_id.is_none() {
        let _ = sender
            .send(Message::Text(
                serde_json::json!({"error": "authentication required"})
                    .to_string()
                    .into(),
            ))
            .await;
        let _ = sender.close().await;
        return;
    }

    // user_id is guaranteed Some at this point due to the guard above.
    let Some(uid) = user_id else {
        tracing::error!("user_id became None after guard check — this is a logic bug");
        return;
    };
    tracing::info!(
        user_id = uid,
        "WebSocket client connected for notifications"
    );

    // Subscribe to per-user and global channels
    let mut user_rx = hub.subscribe_user(uid).await;
    let mut global_rx = hub.subscribe_global();

    // Send initial connection confirmation
    let welcome = serde_json::json!({
        "type": "connected",
        "user_id": uid,
    });
    if sender
        .send(Message::Text(welcome.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    // Forward events from both channels to the WebSocket.
    // - user_rx: only this user's notifications (per-user isolation)
    // - global_rx: public events like job_log
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                Ok(event) = user_rx.recv() => {
                    let msg = match serde_json::to_string(&event) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    if sender.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
                Ok(event) = global_rx.recv() => {
                    // Only forward job_log events from the global channel
                    if event.event_type != "job_log" {
                        continue;
                    }
                    let msg = match serde_json::to_string(&event) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    if sender.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
                else => { break; }
            }
        }
    });

    // Read incoming messages (mainly for keepalive / client commands)
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text)
                    // Client can send ping as text
                    if text == "ping" => {
                        // No-op: keepalive handled
                    }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    tracing::info!(user_id = uid, "WebSocket client disconnected");
}

/// Push a notification to the WebSocket hub for real-time delivery.
///
/// Spawns an async task to send to the user's channel without blocking
/// the caller. The notification is also persisted in the database, so
/// offline users will see it via the REST API on next fetch.
pub fn push_notification(
    hub: &NotificationHub,
    user_id: i64,
    event_type: &str,
    data: serde_json::Value,
) {
    let hub = hub.clone();
    let event_type = event_type.to_string();
    tokio::spawn(async move {
        hub.push_notification(user_id, &event_type, data).await;
    });
}

/// Broadcast a job log update to all WebSocket subscribers.
///
/// Frontend clients can listen for `job_log` events and filter by job_id.
pub fn push_job_log(hub: &NotificationHub, job_id: i64, log: &str) {
    hub.push_job_log(job_id, log);
}

/// GET /api/v1/ws/job/:job_id — WebSocket for real-time job log streaming.
///
/// Requires JWT authentication via query parameter: `?token=<jwt>`.
/// Frontend subscribes to receive `job_log` events filtered by the specified job_id.
pub async fn ws_job_log_handler(
    ws: WebSocketUpgrade,
    Path(job_id): Path<i64>,
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // Authenticate via JWT token in query string
    let user_id = query
        .token
        .as_deref()
        .and_then(|t| rg_core::auth::jwt::validate_token(t, &state.jwt_secret))
        .and_then(|c| c.sub.parse::<i64>().ok());

    ws.on_upgrade(move |socket| {
        handle_job_log_connection(socket, state.notification_hub.clone(), job_id, user_id)
    })
}

/// Handle a job log WebSocket connection.
async fn handle_job_log_connection(
    socket: WebSocket,
    hub: NotificationHub,
    job_id: i64,
    user_id: Option<i64>,
) {
    let (mut sender, mut receiver) = socket.split();

    // Reject unauthenticated connections
    if user_id.is_none() {
        let _ = sender
            .send(Message::Text(
                serde_json::json!({"error": "authentication required"})
                    .to_string()
                    .into(),
            ))
            .await;
        let _ = sender.close().await;
        return;
    }

    // Job log connections only need the global channel
    let mut rx = hub.subscribe_global();

    // Send confirmation
    let welcome = serde_json::json!({
        "type": "connected",
        "job_id": job_id,
    });
    if sender
        .send(Message::Text(welcome.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    let send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            // Only forward job_log events for this specific job_id
            if event.event_type == "job_log" {
                if let Some(eid) = event.data.get("job_id").and_then(|v| v.as_i64()) {
                    if eid == job_id {
                        let msg = match serde_json::to_string(&event) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        if sender.send(Message::Text(msg.into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Close(_) = msg {
                break;
            }
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}
