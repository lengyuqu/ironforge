//! WebSocket real-time notification push.
//!
//! Clients connect to `ws://host/api/v1/ws/notifications?token=<jwt>`
//! and receive real-time notification events as JSON messages.

use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::AppState;

/// The broadcast channel capacity for real-time notifications.
const NOTIFICATION_CHANNEL_CAPACITY: usize = 256;

/// A notification event sent over WebSocket.
#[derive(Debug, Clone, Serialize)]
pub struct NotificationEvent {
    pub event_type: String,
    pub data: serde_json::Value,
}

/// Global notification hub shared across all connections.
#[derive(Debug, Clone)]
pub struct NotificationHub {
    sender: broadcast::Sender<NotificationEvent>,
}

impl NotificationHub {
    /// Create a new notification hub.
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(NOTIFICATION_CHANNEL_CAPACITY);
        Self { sender }
    }

    /// Broadcast a notification event to all connected clients.
    pub fn broadcast(&self, event: NotificationEvent) {
        // It's OK if there are no receivers — the channel just drops the message.
        let _ = self.sender.send(event);
    }

    /// Subscribe to notification events.
    pub fn subscribe(&self) -> broadcast::Receiver<NotificationEvent> {
        self.sender.subscribe()
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
        .map(|c| c.sub.parse::<i64>().ok())
        .flatten();

    ws.on_upgrade(move |socket| handle_ws_connection(socket, state.notification_hub.clone(), user_id))
}

/// Handle an individual WebSocket connection.
async fn handle_ws_connection(
    socket: WebSocket,
    hub: NotificationHub,
    user_id: Option<i64>,
) {
    let (mut sender, mut receiver) = socket.split();

    if user_id.is_none() {
        let _ = sender
            .send(Message::Text(
                serde_json::json!({"error": "authentication required"}).to_string().into(),
            ))
            .await;
        let _ = sender.close().await;
        return;
    }

    let uid = user_id.unwrap();
    tracing::info!(user_id = uid, "WebSocket client connected for notifications");

    // Subscribe to the notification hub
    let mut rx = hub.subscribe();

    // Send initial connection confirmation
    let welcome = serde_json::json!({
        "type": "connected",
        "user_id": uid,
    });
    if sender.send(Message::Text(welcome.to_string().into())).await.is_err() {
        return;
    }

    // Split into two tasks:
    // 1. Read from broadcast channel and forward to WebSocket
    // 2. Read from WebSocket for client commands (ping/pong, etc.)

    let send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            // Filter: only send notifications meant for this user
            // The event data should contain a `user_id` field
            if let Some(event_user_id) = event.data.get("user_id").and_then(|v| v.as_i64()) {
                if event_user_id != uid {
                    continue;
                }
            }

            let msg = match serde_json::to_string(&event) {
                Ok(s) => s,
                Err(_) => continue,
            };

            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Read incoming messages (mainly for keepalive / client commands)
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    // Client can send ping as text
                    if text == "ping" {
                        // No-op: keepalive handled
                    }
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
pub fn push_notification(
    hub: &NotificationHub,
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
    hub.broadcast(event);
}
