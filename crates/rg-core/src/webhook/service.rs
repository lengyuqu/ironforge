//! Webhook service — register, trigger, and deliver webhooks.
//!
//! Webhooks allow external services to be notified when events occur in a
//! repository (push, issues, pull_request, etc.). This service handles:
//! - CRUD for webhook registrations
//! - Event dispatch (find matching webhooks and fire HTTP POST)
//! - Delivery recording (status, response, timing)

use anyhow::{Context, Result};
use chrono::Utc;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use rg_db::entities::webhook;
use rg_db::entities::webhook_delivery;
use rg_db::ops::webhook_ops;

// ── API types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateWebhookRequest {
    pub url: String,
    pub content_type: Option<String>,  // "json" (default) or "form"
    pub secret: Option<String>,
    pub active: Option<bool>,
    pub events: Vec<String>,  // e.g. ["push", "issues"]
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateWebhookRequest {
    pub url: Option<String>,
    pub content_type: Option<String>,
    pub secret: Option<String>,
    pub active: Option<bool>,
    pub events: Option<Vec<String>>,
}

// ── CRUD ──────────────────────────────────────────────────────────────────

/// Register a new webhook for a repository.
pub async fn create_webhook(
    db: &DatabaseConnection,
    repo_id: i64,
    req: &CreateWebhookRequest,
) -> Result<webhook::Model> {
    let now = Utc::now();
    let events_str = req.events.join(",");
    let model = webhook::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: sea_orm::Set(repo_id),
        url: sea_orm::Set(req.url.clone()),
        content_type: sea_orm::Set(req.content_type.clone().unwrap_or_else(|| "json".to_string())),
        secret: sea_orm::Set(req.secret.clone()),
        active: sea_orm::Set(req.active.unwrap_or(true)),
        events: sea_orm::Set(events_str),
        created_at: sea_orm::Set(now),
        updated_at: sea_orm::Set(now),
    };
    webhook_ops::create_webhook(db, model).await
}

/// List all webhooks for a repository.
pub async fn list_webhooks(
    db: &DatabaseConnection,
    repo_id: i64,
) -> Result<Vec<webhook::Model>> {
    webhook_ops::list_by_repo(db, repo_id).await
}

/// Get a webhook by id.
pub async fn get_webhook(
    db: &DatabaseConnection,
    id: i64,
) -> Result<Option<webhook::Model>> {
    webhook_ops::find_by_id(db, id).await
}

/// Update a webhook.
pub async fn update_webhook(
    db: &DatabaseConnection,
    existing: &webhook::Model,
    req: &UpdateWebhookRequest,
) -> Result<webhook::Model> {
    let model = webhook::ActiveModel {
        id: sea_orm::Set(existing.id),
        repo_id: sea_orm::Set(existing.repo_id),
        url: sea_orm::Set(req.url.clone().unwrap_or_else(|| existing.url.clone())),
        content_type: sea_orm::Set(req.content_type.clone().unwrap_or_else(|| existing.content_type.clone())),
        secret: sea_orm::Set(req.secret.clone().or_else(|| existing.secret.clone())),
        active: sea_orm::Set(req.active.unwrap_or(existing.active)),
        events: sea_orm::Set(
            req.events
                .as_ref()
                .map(|e| e.join(","))
                .unwrap_or_else(|| existing.events.clone()),
        ),
        created_at: sea_orm::Set(existing.created_at),
        updated_at: sea_orm::Set(Utc::now()),
    };
    webhook_ops::update_webhook(db, model).await
}

/// Delete a webhook.
pub async fn delete_webhook(
    db: &DatabaseConnection,
    id: i64,
) -> Result<()> {
    webhook_ops::delete_webhook_by_id(db, id).await
}

// ── Event dispatch ────────────────────────────────────────────────────────

/// Trigger a webhook event: find matching webhooks and deliver payloads.
pub async fn trigger_event(
    db: &DatabaseConnection,
    repo_id: i64,
    event: &str,
    payload: &Value,
) -> Result<()> {
    let hooks = webhook_ops::list_active_by_repo_and_event(db, repo_id, event).await?;

    for hook in hooks {
        // Spawn delivery in background — don't block the caller
        let db_clone = db.clone();
        let hook_id = hook.id;
        let event_str = event.to_string();
        let payload_str = serde_json::to_string(payload).unwrap_or_default();
        let url = hook.url.clone();
        let content_type = hook.content_type.clone();
        let secret = hook.secret.clone();

        tokio::spawn(async move {
            let delivery_id = uuid::Uuid::new_v4().to_string();
            let start = std::time::Instant::now();

            let (status, response_body) = match deliver(&url, &content_type, &secret, &payload_str).await {
                Ok(resp_status) => (Some(resp_status), None::<String>),
                Err(e) => {
                    tracing::warn!(webhook_id = hook_id, error = %e, "webhook delivery failed");
                    (None, Some(format!("delivery error: {:#}", e)))
                }
            };

            let duration_ms = start.elapsed().as_millis() as i64;

            let delivery_model = webhook_delivery::ActiveModel {
                id: sea_orm::NotSet,
                webhook_id: sea_orm::Set(hook_id),
                event: sea_orm::Set(event_str),
                delivery_id: sea_orm::Set(delivery_id),
                response_status: sea_orm::Set(status),
                request_payload: sea_orm::Set(Some(payload_str)),
                response_body: sea_orm::Set(response_body),
                duration_ms: sea_orm::Set(Some(duration_ms)),
                created_at: sea_orm::Set(Utc::now()),
            };

            if let Err(e) = webhook_ops::create_delivery(&db_clone, delivery_model).await {
                tracing::error!(error = %e, "failed to record webhook delivery");
            }
        });
    }

    Ok(())
}

/// Deliver a webhook payload via HTTP POST.
async fn deliver(
    url: &str,
    content_type: &str,
    secret: &Option<String>,
    payload: &str,
) -> Result<i32> {
    let client = reqwest::Client::new();
    let mut builder = client.post(url);

    if content_type == "form" {
        builder = builder
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(payload.to_string());
    } else {
        builder = builder
            .header("Content-Type", "application/json")
            .body(payload.to_string());
    }

    // Sign with HMAC-SHA256 if secret is configured
    if let Some(secret) = secret {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .context("HMAC init failed")?;
        mac.update(payload.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());
        builder = builder.header("X-Hub-Signature-256", format!("sha256={}", sig));
    }

    let resp = builder.send().await.context("webhook POST failed")?;
    Ok(resp.status().as_u16() as i32)
}

/// List recent deliveries for a webhook.
pub async fn list_deliveries(
    db: &DatabaseConnection,
    webhook_id: i64,
) -> Result<Vec<webhook_delivery::Model>> {
    webhook_ops::list_deliveries_by_webhook(db, webhook_id).await
}

/// Get a delivery by id.
pub async fn get_delivery(
    db: &DatabaseConnection,
    id: i64,
) -> Result<Option<webhook_delivery::Model>> {
    webhook_ops::find_delivery_by_id(db, id).await
}

/// Redeliver a webhook (re-post the original payload).
pub async fn redeliver(
    db: &DatabaseConnection,
    delivery_id: i64,
) -> Result<()> {
    let delivery = webhook_ops::find_delivery_by_id(db, delivery_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("delivery {} not found", delivery_id))?;

    let hook = webhook_ops::find_by_id(db, delivery.webhook_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("webhook {} not found", delivery.webhook_id))?;

    let payload = delivery.request_payload.clone().unwrap_or_default();

    // Fire and record a new delivery
    let _ = trigger_event(
        db,
        hook.repo_id,
        &delivery.event,
        &serde_json::from_str(&payload).unwrap_or(Value::Null),
    )
    .await;

    Ok(())
}
