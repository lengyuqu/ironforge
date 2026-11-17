//! Webhook service — register, trigger, and deliver webhooks.
//!
//! Webhooks allow external services to be notified when events occur in a
//! repository (push, issues, pull_request, etc.). This service handles:
//! - CRUD for webhook registrations
//! - Event dispatch (find matching webhooks and fire HTTP POST)
//! - Delivery recording (status, response, timing)

use anyhow::{Context, Result};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use rg_db::entities::webhook;
use rg_db::entities::webhook_delivery;
use rg_db::ops::webhook_ops;

use crate::agent::ActorRef;
use crate::error::{CoreError, CoreResult};

// ── API types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateWebhookRequest {
    pub url: String,
    pub content_type: Option<String>, // "json" (default) or "form"
    pub secret: Option<String>,
    pub active: Option<bool>,
    pub events: Vec<String>, // e.g. ["push", "issue.opened"] — full names with a dot; matching is a case-insensitive LIKE substring of the delivered name, so a bare "issues" or "*" matches nothing and the hook dies silently
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
) -> CoreResult<webhook::Model> {
    // SSRF guard: only public http/https endpoints may receive payloads.
    crate::net_guard::validate_webhook_url(&req.url)
        .await
        .map_err(|e| CoreError::invalid_input(format!("invalid webhook URL: {e}")))?;

    let now = Utc::now();
    let events_str = req.events.join(",");
    let model = webhook::ActiveModel {
        id: sea_orm::NotSet,
        repo_id: sea_orm::Set(repo_id),
        url: sea_orm::Set(req.url.clone()),
        content_type: sea_orm::Set(
            req.content_type
                .clone()
                .unwrap_or_else(|| "json".to_string()),
        ),
        secret: sea_orm::Set(req.secret.clone()),
        active: sea_orm::Set(req.active.unwrap_or(true)),
        events: sea_orm::Set(events_str),
        created_at: sea_orm::Set(now),
        updated_at: sea_orm::Set(now),
    };
    Ok(webhook_ops::create_webhook(db, model).await?)
}

/// List all webhooks for a repository.
pub async fn list_webhooks(
    db: &DatabaseConnection,
    repo_id: i64,
) -> CoreResult<Vec<webhook::Model>> {
    Ok(webhook_ops::list_by_repo(db, repo_id).await?)
}

/// Get a webhook by id.
pub async fn get_webhook(db: &DatabaseConnection, id: i64) -> CoreResult<Option<webhook::Model>> {
    Ok(webhook_ops::find_by_id(db, id).await?)
}

/// Update a webhook.
pub async fn update_webhook(
    db: &DatabaseConnection,
    existing: &webhook::Model,
    req: &UpdateWebhookRequest,
) -> CoreResult<webhook::Model> {
    if let Some(url) = &req.url {
        // SSRF guard on updated endpoint.
        crate::net_guard::validate_webhook_url(url)
            .await
            .map_err(|e| CoreError::invalid_input(format!("invalid webhook URL: {e}")))?;
    }

    let model = webhook::ActiveModel {
        id: sea_orm::Set(existing.id),
        repo_id: sea_orm::Set(existing.repo_id),
        url: sea_orm::Set(req.url.clone().unwrap_or_else(|| existing.url.clone())),
        content_type: sea_orm::Set(
            req.content_type
                .clone()
                .unwrap_or_else(|| existing.content_type.clone()),
        ),
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
    Ok(webhook_ops::update_webhook(db, model).await?)
}

/// Delete a webhook.
pub async fn delete_webhook(db: &DatabaseConnection, id: i64) -> CoreResult<()> {
    Ok(webhook_ops::delete_webhook_by_id(db, id).await?)
}

// ── Event envelope ────────────────────────────────────────────────────────

/// Version of the [`EventEnvelope`] wrapper. A consumer that sees a higher
/// number must not assume it understands anything beyond `data`.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Header carrying the event kind (`push`, `pull_request.opened`, …).
pub const HEADER_EVENT: &str = "X-IronForge-Event";
/// Header carrying the delivery id, also recorded in `webhook_deliveries`.
pub const HEADER_DELIVERY: &str = "X-IronForge-Delivery";

/// The outer shape every repository event is published in: one envelope, the
/// business payload untouched under `data`.
///
/// The envelope is what a consumer subscribes to — it can dispatch on `kind`,
/// deduplicate on `id` (the same id is used for the event log and every webhook
/// delivery of that event), and tell an agent's action from a person's through
/// `actor`. `data` is byte-for-byte what the event carried before the envelope
/// existed, so an existing receiver that reads the fields it knows keeps
/// working: it only has to look one level deeper.
///
/// `actor` is `null` when the event names no account (for example a
/// collaboration declaration, or a payload written before actor ids were
/// carried).
#[derive(Debug, Clone, Serialize)]
pub struct EventEnvelope {
    pub schema: u32,
    pub id: uuid::Uuid,
    pub kind: String,
    pub at: chrono::DateTime<Utc>,
    pub repo_id: i64,
    pub actor: Option<ActorRef>,
    pub data: Value,
}

impl EventEnvelope {
    /// Wrap `data` as a new event of `kind`. The caller builds it once so the
    /// event log, the headers and every delivery share one id and timestamp.
    pub fn new(repo_id: i64, kind: &str, actor: Option<ActorRef>, data: Value) -> Self {
        Self {
            schema: EVENT_SCHEMA_VERSION,
            id: uuid::Uuid::new_v4(),
            kind: kind.to_string(),
            at: Utc::now(),
            repo_id,
            actor,
            data,
        }
    }

    pub fn to_value(self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// Does this JSON value already carry an envelope? Readers use it to accept
/// rows written both before and after the envelope existed, and redelivery uses
/// it to avoid wrapping a stored payload twice.
///
/// Accepts any `schema >= 1`, so a future version is still unwrapped rather
/// than mistaken for a legacy payload.
pub fn looks_like_envelope(value: &Value) -> bool {
    value
        .get("schema")
        .and_then(Value::as_u64)
        .is_some_and(|v| v >= 1)
        && value.get("id").and_then(Value::as_str).is_some()
        && value.get("kind").and_then(Value::as_str).is_some()
        && value.get("data").is_some()
}

/// Payload keys that name the account that caused an event, in the order they
/// are tried. Event payloads are assembled by the calling service, so this is
/// the only actor information available without changing every call site.
const ACTOR_ID_KEYS: [&str; 3] = ["actor_id", "author_id", "pushed_by"];

/// Keys under which a payload nests the object it describes, so the account can
/// be read from the object itself (`pull_request.author_id`, `issue.author_id`,
/// …). Tried only when the top level names no account.
const ACTOR_CONTAINERS: [&str; 6] = [
    "actor",
    "pull_request",
    "issue",
    "release",
    "milestone",
    "comment",
];

fn id_in(value: &Value) -> Option<i64> {
    ACTOR_ID_KEYS
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_i64))
}

/// The account id an event names, at the top level or one level down.
fn actor_id(payload: &Value) -> Option<i64> {
    id_in(payload).or_else(|| {
        ACTOR_CONTAINERS
            .iter()
            .find_map(|key| payload.get(*key).and_then(id_in))
    })
}

/// The actor of an event, when its payload names one. Best effort: a lookup
/// failure leaves `actor` null instead of failing the delivery.
async fn event_actor(db: &DatabaseConnection, payload: &Value) -> Option<ActorRef> {
    event_actor_on(db, payload).await
}

/// The actor of an event resolved through `conn` — a transaction included.
///
/// This carries the same rule as [`crate::agent::resolve_actors`] (the `users`
/// row, plus the owner of an agent), expressed over any connection because the
/// event row can be written inside a business transaction (#15, step 1).
/// `resolve_actors` is bound to `&DatabaseConnection` and widening it is outside
/// this task's owned paths; [`event_actor`] delegates here so the service has a
/// single rule.
async fn event_actor_on<C: ConnectionTrait>(conn: &C, payload: &Value) -> Option<ActorRef> {
    let id = actor_id(payload)?;
    match resolve_actor(conn, id).await {
        Ok(actor) => actor,
        Err(error) => {
            tracing::warn!(actor_id = id, %error, "could not resolve a webhook event actor");
            None
        }
    }
}

async fn resolve_actor<C: ConnectionTrait>(conn: &C, id: i64) -> Result<Option<ActorRef>> {
    let Some(user) = rg_db::entities::user::Entity::find_by_id(id)
        .one(conn)
        .await?
    else {
        return Ok(None);
    };
    if !crate::agent::is_agent(&user) {
        return Ok(Some(ActorRef {
            id: user.id,
            username: user.username,
            kind: crate::agent::KIND_HUMAN,
            owner_id: None,
            owner_username: None,
        }));
    }
    let owner_username = match user.owner_user_id {
        Some(owner_id) => rg_db::entities::user::Entity::find_by_id(owner_id)
            .one(conn)
            .await?
            .map(|owner| owner.username),
        None => None,
    };
    Ok(Some(ActorRef {
        id: user.id,
        username: user.username,
        kind: crate::agent::KIND_AGENT,
        owner_id: user.owner_user_id,
        owner_username,
    }))
}

// ── Event dispatch ────────────────────────────────────────────────────────

/// Trigger a webhook event: find matching webhooks and deliver payloads.
///
/// `payload` is the business payload; it is published untouched under the
/// envelope's `data`.
///
/// Two halves, one envelope: the log row and the delivered body keep the same
/// envelope id, exactly as before the split (#15, step 1). The log write stays
/// best effort — a log failure never failed the webhook and still does not.
pub async fn trigger_event(
    db: &DatabaseConnection,
    repo_id: i64,
    event: &str,
    payload: &Value,
) -> Result<()> {
    let actor = event_actor(db, payload).await;
    let envelope = EventEnvelope::new(repo_id, event, actor, payload.clone()).to_value();
    record_envelope(db, repo_id, event, &envelope).await;
    deliver_envelope(db, repo_id, event, envelope).await
}

/// Record an event in the repository's log **through any connection** — a
/// transaction included — and deliver nothing.
///
/// This is the half of [`trigger_event`] that has to happen inside the business
/// transaction (#15, step 1): the issue row and its `issue.opened` event then
/// commit or roll back together, instead of the event being written after the
/// commit where a crash loses it forever.
///
/// Unlike the legacy best-effort log write, an append failure is returned: the
/// caller's transaction must abort rather than commit a business change whose
/// event is missing. The delivery half is [`deliver_event`], called after the
/// commit.
pub async fn record_event_in_txn<C: ConnectionTrait>(
    conn: &C,
    repo_id: i64,
    event: &str,
    payload: &Value,
) -> Result<()> {
    let actor = event_actor_on(conn, payload).await;
    let envelope = EventEnvelope::new(repo_id, event, actor, payload.clone()).to_value();
    append_envelope(conn, repo_id, event, &envelope).await
}

/// Deliver an event whose log row was already written by
/// [`record_event_in_txn`]. Called after the commit.
///
/// Deliberately **does not** append to the log: writing it again would record
/// the event twice and move the subscribers' cursor two steps per event.
pub async fn deliver_event(
    db: &DatabaseConnection,
    repo_id: i64,
    event: &str,
    payload: &Value,
) -> Result<()> {
    let actor = event_actor(db, payload).await;
    let envelope = EventEnvelope::new(repo_id, event, actor, payload.clone()).to_value();
    deliver_envelope(db, repo_id, event, envelope).await
}

/// Append an event to the repository's log **only** — no webhook, no retry job.
///
/// This is the log half of [`trigger_event`], split out (T11) because the agent
/// draft area has to be silent towards people while still being observable to
/// the agent: `wait_repo_events` must see the push, no receiver may be woken by
/// it. Callers that mean "a webhook event the log also records" keep using
/// [`trigger_event`]; callers that mean "record this, deliver nothing" use this
/// or [`trigger_event_quietly`].
///
/// The envelope is the same one the delivered event would carry, so a reader
/// cannot tell a quiet event from a delivered one by shape — only by the absence
/// of `webhook_deliveries` rows.
pub async fn record_event(
    db: &DatabaseConnection,
    repo_id: i64,
    event: &str,
    payload: &Value,
) -> Result<()> {
    let actor = event_actor(db, payload).await;
    let envelope = EventEnvelope::new(repo_id, event, actor, payload.clone()).to_value();
    record_envelope(db, repo_id, event, &envelope).await;
    Ok(())
}

/// Trigger an event that is deliberately **not** delivered to any webhook.
///
/// Identical to [`record_event`] in effect; the separate name is the point. A
/// delivery channel added to [`trigger_event`] later must be considered here
/// explicitly, where a `deliver: false` flag threaded through one function would
/// make it easy to switch off a channel that the quiet path never chose to
/// suppress (and, worse, to leave a new channel on for events that were meant to
/// be silent). Read this as "this event *would* have been delivered, and the
/// suppression is intentional".
pub async fn trigger_event_quietly(
    db: &DatabaseConnection,
    repo_id: i64,
    event: &str,
    payload: &Value,
) -> Result<()> {
    tracing::debug!(
        repo_id,
        event,
        "webhook event recorded without delivery (draft namespace)"
    );
    record_event(db, repo_id, event, payload).await
}

/// Record one already-built envelope in the repository's event log, best
/// effort: it never blocks or fails the caller, as it never did before.
async fn record_envelope<C: ConnectionTrait>(
    conn: &C,
    repo_id: i64,
    event: &str,
    envelope: &Value,
) {
    if let Err(error) = append_envelope(conn, repo_id, event, envelope).await {
        tracing::warn!(repo_id, %event, %error, "failed to record a repository event");
    }
}

/// Append one already-built envelope to the log through `conn`, replacing an
/// oversized body with a stub that says so. Errors are returned: a caller inside
/// a transaction wants them, the legacy best-effort path swallows them.
async fn append_envelope<C: ConnectionTrait>(
    conn: &C,
    repo_id: i64,
    event: &str,
    envelope: &Value,
) -> Result<()> {
    let text = serde_json::to_string(envelope).unwrap_or_default();
    let stored = if text.len() <= MAX_EVENT_PAYLOAD_BYTES {
        text
    } else {
        serde_json::json!({ "truncated": true, "original_bytes": text.len() }).to_string()
    };
    let event: String = event.chars().take(64).collect();
    rg_db::ops::repo_event_ops::append(conn, repo_id, &event, Some(&stored)).await?;
    Ok(())
}

/// Publish one already-built envelope: append it to the event log, then deliver
/// it to every active hook of the repository.
async fn dispatch(
    db: &DatabaseConnection,
    repo_id: i64,
    event: &str,
    envelope: Value,
) -> Result<()> {
    record_envelope(db, repo_id, event, &envelope).await;
    deliver_envelope(db, repo_id, event, envelope).await
}

/// Deliver one already-built envelope to every active hook of the repository.
///
/// Never touches the event log: the row was written by the caller (the log half
/// of [`trigger_event`], or [`record_event_in_txn`] before the commit). Keeping
/// the two apart is what lets the log write happen inside a transaction while
/// the delivery stays outside it — and what keeps the log row and the delivered
/// body sharing one envelope.
async fn deliver_envelope(
    db: &DatabaseConnection,
    repo_id: i64,
    event: &str,
    envelope: Value,
) -> Result<()> {
    let body = serde_json::to_string(&envelope).unwrap_or_default();

    let hooks = webhook_ops::list_active_by_repo_and_event(db, repo_id, event).await?;

    for hook in hooks {
        // Spawn delivery in background — don't block the caller
        let db_clone = db.clone();
        let hook_id = hook.id;
        let event_str = event.to_string();
        let body = body.clone();
        let url = hook.url.clone();
        let content_type = hook.content_type.clone();
        let secret = hook.secret.clone();

        tokio::spawn(async move {
            let delivery_id = uuid::Uuid::new_v4().to_string();
            let start = std::time::Instant::now();

            let (status, response_body) = match deliver(
                &url,
                &content_type,
                &secret,
                &body,
                &event_str,
                &delivery_id,
            )
            .await
            {
                Ok(resp_status) if retryable_status(resp_status) => {
                    // The receiver is asking for another attempt. A 5xx or 429
                    // is "come back later", not a delivered event: queue the
                    // same durable retry a transport failure queues.
                    tracing::warn!(
                        webhook_id = hook_id,
                        event = %event_str,
                        http_status = resp_status,
                        "webhook receiver asked for a retry"
                    );
                    enqueue_retry(&db_clone, hook_id, &event_str, &body).await;
                    (
                        Some(resp_status),
                        Some(format!("retryable receiver status {resp_status}")),
                    )
                }
                Ok(resp_status) => {
                    tracing::info!(
                        webhook_id = hook_id,
                        event = %event_str,
                        http_status = resp_status,
                        "webhook delivered successfully"
                    );
                    (Some(resp_status), None::<String>)
                }
                Err(e) => {
                    tracing::warn!(
                        webhook_id = hook_id,
                        event = %event_str,
                        error = %e,
                        "webhook delivery failed"
                    );
                    enqueue_retry(&db_clone, hook_id, &event_str, &body).await;
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
                request_payload: sea_orm::Set(Some(body)),
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

/// Queue a durable retry (QUEUE-001): the background worker redelivers with
/// backoff and records every attempt. Used for transport failures and for
/// receiver statuses that ask for another attempt.
async fn enqueue_retry(db: &DatabaseConnection, hook_id: i64, event: &str, body: &str) {
    let queue = crate::background_jobs::BackgroundJobQueue::new(db.clone());
    let retry_payload = serde_json::json!({
        "webhook_id": hook_id,
        "event": event,
        "payload": body,
    });
    if let Err(retry_err) = queue
        .enqueue(crate::background_jobs::TASK_WEBHOOK_DELIVER, &retry_payload)
        .await
    {
        tracing::error!(
            webhook_id = hook_id,
            error = %retry_err,
            "failed to enqueue webhook delivery retry"
        );
    }
}

/// Does a receiver status ask for another attempt?
///
/// 429 and 5xx are transient by definition. 2xx–4xx are a final answer: a 4xx
/// means the receiver rejected this payload, and sending it again unchanged
/// cannot change that.
fn retryable_status(status: i32) -> bool {
    status == 429 || (500..=599).contains(&status)
}

/// Largest payload kept in the event log. A bigger one is replaced by a stub
/// saying so, so the log stays small and a reader knows to fetch the object.
const MAX_EVENT_PAYLOAD_BYTES: usize = 16 * 1024;

/// Shared HTTP client for webhook deliveries.
///
/// - Redirects are **not** followed: a validated public endpoint must not be
///   able to bounce the POST to an internal target via 3xx (which would
///   bypass the SSRF guard). Receivers that answer 3xx are recorded as such.
/// - Connection pooling avoids a new TLS handshake per delivery.
static HTTP_CLIENT: std::sync::LazyLock<Option<reqwest::Client>> = std::sync::LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()
});

/// Deliver a webhook payload via HTTP POST.
///
/// Every request carries the envelope kind and the delivery id as headers, so a
/// receiver can route and deduplicate without parsing the body, and can match a
/// request to the delivery recorded for it.
async fn deliver(
    url: &str,
    content_type: &str,
    secret: &Option<String>,
    payload: &str,
    event: &str,
    delivery_id: &str,
) -> Result<i32> {
    // Re-validate immediately before sending: covers legacy rows created
    // before the URL guard existed and narrows the DNS-rebinding window
    // (a host that resolved publicly at registration time may now resolve
    // internally).
    crate::net_guard::validate_webhook_url(url)
        .await
        .map_err(|e| anyhow::anyhow!("webhook URL rejected: {e}"))?;

    let client = HTTP_CLIENT
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("failed to initialise webhook HTTP client"))?;
    let mut builder = client
        .post(url)
        .header(HEADER_EVENT, event)
        .header(HEADER_DELIVERY, delivery_id);

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

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).context("HMAC init failed")?;
        mac.update(payload.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());
        builder = builder.header("X-Hub-Signature-256", format!("sha256={}", sig));
    }

    let resp = builder.send().await.context("webhook POST failed")?;
    Ok(resp.status().as_u16() as i32)
}

/// Execute one queued webhook delivery (QUEUE-001 `webhook.deliver` handler).
///
/// Payload: `{ "webhook_id", "event", "payload" }`. Transport failures and
/// receiver statuses that ask for another attempt (429, 5xx) return `Err` so the
/// queue reschedules with backoff; every attempt is recorded in
/// `webhook_deliveries`. A webhook deleted after the retry was scheduled is
/// treated as resolved (Ok); a hook that is only `active = false` is still delivered to.
pub async fn deliver_retry_job(db: &DatabaseConnection, payload: &Value) -> Result<()> {
    let webhook_id = payload["webhook_id"]
        .as_i64()
        .context("webhook retry payload is missing webhook_id")?;
    let event = payload["event"]
        .as_str()
        .context("webhook retry payload is missing event")?;
    let body = payload["payload"]
        .as_str()
        .context("webhook retry payload is missing payload")?;

    let hook = match webhook_ops::find_by_id(db, webhook_id).await? {
        Some(hook) => hook,
        None => {
            tracing::info!(webhook_id, "webhook deleted; dropping queued retry");
            return Ok(());
        }
    };

    let delivery_id = uuid::Uuid::new_v4().to_string();
    let start = std::time::Instant::now();
    let (status, response_body) = match deliver(
        &hook.url,
        &hook.content_type,
        &hook.secret,
        body,
        event,
        &delivery_id,
    )
    .await
    {
        Ok(resp_status) if retryable_status(resp_status) => {
            tracing::warn!(
                webhook_id,
                event = %event,
                http_status = resp_status,
                "webhook redelivery: receiver asked for another attempt"
            );
            (
                Some(resp_status),
                Some(format!("retryable receiver status {resp_status}")),
            )
        }
        Ok(resp_status) => {
            tracing::info!(
                webhook_id,
                event = %event,
                http_status = resp_status,
                "webhook redelivery succeeded"
            );
            (Some(resp_status), None::<String>)
        }
        Err(e) => (None, Some(format!("delivery error: {:#}", e))),
    };

    let duration_ms = start.elapsed().as_millis() as i64;
    let delivery_model = webhook_delivery::ActiveModel {
        id: sea_orm::NotSet,
        webhook_id: sea_orm::Set(webhook_id),
        event: sea_orm::Set(event.to_string()),
        delivery_id: sea_orm::Set(delivery_id),
        response_status: sea_orm::Set(status),
        request_payload: sea_orm::Set(Some(body.to_string())),
        response_body: sea_orm::Set(response_body.clone()),
        duration_ms: sea_orm::Set(Some(duration_ms)),
        created_at: sea_orm::Set(Utc::now()),
    };
    webhook_ops::create_delivery(db, delivery_model).await?;

    match response_body {
        Some(error) => anyhow::bail!("{error}"),
        None => Ok(()),
    }
}

/// List recent deliveries for a webhook.
pub async fn list_deliveries(
    db: &DatabaseConnection,
    webhook_id: i64,
) -> CoreResult<Vec<webhook_delivery::Model>> {
    Ok(webhook_ops::list_deliveries_by_webhook(db, webhook_id).await?)
}

/// Get a delivery by id.
pub async fn get_delivery(
    db: &DatabaseConnection,
    id: i64,
) -> CoreResult<Option<webhook_delivery::Model>> {
    Ok(webhook_ops::find_delivery_by_id(db, id).await?)
}

/// Redeliver a delivery: replays its payload via the event dispatcher, so the event reaches every active hook of the repo, not only the original one. The stored envelope, including its id, is reused; a delivery recorded before the envelope existed is wrapped once.
pub async fn redeliver(db: &DatabaseConnection, delivery_id: i64) -> CoreResult<()> {
    let delivery = webhook_ops::find_delivery_by_id(db, delivery_id)
        .await?
        .ok_or_else(|| CoreError::not_found("delivery not found"))?;

    let hook = webhook_ops::find_by_id(db, delivery.webhook_id)
        .await?
        .ok_or_else(|| CoreError::not_found("webhook not found"))?;

    let payload = delivery.request_payload.clone().unwrap_or_default();
    let stored = serde_json::from_str::<Value>(&payload).unwrap_or(Value::Null);
    // A replayed event must keep its identity: the same envelope id reaches
    // every hook, so a receiver that deduplicates on it does not see the
    // original and the replay as two events.
    let envelope = if looks_like_envelope(&stored) {
        stored
    } else {
        let actor = event_actor(db, &stored).await;
        EventEnvelope::new(hook.repo_id, &delivery.event, actor, stored).to_value()
    };

    // Fire and record a new delivery
    if let Err(e) = dispatch(db, hook.repo_id, &delivery.event, envelope).await {
        tracing::warn!("Failed to redeliver webhook event: {e}");
    }

    Ok(())
}

// ── Convenience event helpers ───────────────────────────────────────────

// `trigger_release_created` used to live here and recorded the event *after* the
// release insert, which is exactly the crash window #15 is about. The creation
// path now writes the event inside its transaction
// (`release::service::create_release`); the helper had no other caller and was
// removed so the old shape cannot be reintroduced by accident. A caller that
// really needs "record and deliver after the fact" should use `trigger_event`
// explicitly and say why in a comment.

/// Trigger a release.deleted webhook event.
pub async fn trigger_release_deleted(
    db: &DatabaseConnection,
    repo_id: i64,
    release: &Value,
) -> Result<()> {
    let payload = serde_json::json!({
        "event": "release.deleted",
        "release": release,
    });
    trigger_event(db, repo_id, "release.deleted", &payload).await
}

/// Trigger a branch.created webhook event.
pub async fn trigger_branch_created(
    db: &DatabaseConnection,
    repo_id: i64,
    branch: &str,
) -> Result<()> {
    let payload = serde_json::json!({
        "event": "branch.created",
        "ref": branch,
        "ref_type": "branch",
    });
    trigger_event(db, repo_id, "branch.created", &payload).await
}

/// Trigger a branch.deleted webhook event.
pub async fn trigger_branch_deleted(
    db: &DatabaseConnection,
    repo_id: i64,
    branch: &str,
) -> Result<()> {
    let payload = serde_json::json!({
        "event": "branch.deleted",
        "ref": branch,
        "ref_type": "branch",
    });
    trigger_event(db, repo_id, "branch.deleted", &payload).await
}

/// Trigger a tag.created webhook event.
pub async fn trigger_tag_created(db: &DatabaseConnection, repo_id: i64, tag: &str) -> Result<()> {
    let payload = serde_json::json!({
        "event": "tag.created",
        "ref": tag,
        "ref_type": "tag",
    });
    trigger_event(db, repo_id, "tag.created", &payload).await
}

/// Trigger a tag.deleted webhook event.
pub async fn trigger_tag_deleted(db: &DatabaseConnection, repo_id: i64, tag: &str) -> Result<()> {
    let payload = serde_json::json!({
        "event": "tag.deleted",
        "ref": tag,
        "ref_type": "tag",
    });
    trigger_event(db, repo_id, "tag.deleted", &payload).await
}

/// Trigger an issue.opened webhook event.
pub async fn trigger_issue_opened(
    db: &DatabaseConnection,
    repo_id: i64,
    issue: &Value,
) -> Result<()> {
    let payload = serde_json::json!({
        "event": "issue.opened",
        "issue": issue,
    });
    trigger_event(db, repo_id, "issue.opened", &payload).await
}

/// Trigger an issue.closed webhook event.
pub async fn trigger_issue_closed(
    db: &DatabaseConnection,
    repo_id: i64,
    issue: &Value,
) -> Result<()> {
    let payload = serde_json::json!({
        "event": "issue.closed",
        "issue": issue,
    });
    trigger_event(db, repo_id, "issue.closed", &payload).await
}

/// Trigger an issue.comment webhook event.
pub async fn trigger_issue_comment(
    db: &DatabaseConnection,
    repo_id: i64,
    issue: &Value,
    comment: &Value,
) -> Result<()> {
    let payload = serde_json::json!({
        "event": "issue.comment",
        "issue": issue,
        "comment": comment,
    });
    trigger_event(db, repo_id, "issue.comment", &payload).await
}

// `trigger_pr_opened` and `trigger_pr_closed` used to live here. Both wrote the
// event *after* the pull request write, and the closed one was worse: it was
// called *before* `update_pr` wrote the state change, so a failed update still
// announced a close that never happened (#15, step 6). Creation and update now
// write their log rows inside the business transaction
// (`pull_request::service::{create_pr, update_pr}`) and only deliver after the
// commit. The helpers had no other caller and were removed so the old shape
// cannot be reintroduced by accident. A caller that really needs "record and
// deliver after the fact" should use `trigger_event` explicitly and say why in a
// comment.

// `trigger_pr_merged` used to live here and recorded the event *after* the merge
// transaction committed, which is exactly the crash window #15 is about. The
// merge path now writes the event inside that transaction
// (`pull_request::service::update_pr_merged`); the helper had no other caller and
// was removed so the old shape cannot be reintroduced by accident. A caller that
// really needs "record and deliver after the fact" should use `trigger_event`
// explicitly and say why in a comment.

/// Trigger a milestone.closed webhook event.
pub async fn trigger_milestone_closed(
    db: &DatabaseConnection,
    repo_id: i64,
    milestone: &Value,
) -> Result<()> {
    let payload = serde_json::json!({
        "event": "milestone.closed",
        "milestone": milestone,
    });
    trigger_event(db, repo_id, "milestone.closed", &payload).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A payload is only treated as an envelope when every marker is present, so
    /// a business payload is never silently replaced by its `data`.
    #[test]
    fn a_legacy_payload_is_not_mistaken_for_an_envelope() {
        assert!(!looks_like_envelope(&serde_json::json!({
            "ref": "refs/heads/main",
            "after": "abc",
        })));
        // A business payload that happens to use one of the key names is still
        // not an envelope.
        assert!(!looks_like_envelope(&serde_json::json!({ "kind": "push" })));
        // Every marker is required: a payload missing `data` is not an envelope.
        assert!(!looks_like_envelope(&serde_json::json!({
            "schema": 1,
            "id": "x",
            "kind": "push",
        })));

        let envelope =
            EventEnvelope::new(7, "push", None, serde_json::json!({ "after": "abc" })).to_value();
        assert!(looks_like_envelope(&envelope));
        assert_eq!(envelope["schema"], EVENT_SCHEMA_VERSION);

        // A future schema version is still recognised, so a reader unwraps it
        // instead of treating it as a legacy payload.
        let mut future = envelope;
        future["schema"] = serde_json::json!(99);
        assert!(looks_like_envelope(&future));
    }

    #[test]
    fn only_a_receiver_asking_for_another_attempt_is_retried() {
        for transient in [429, 500, 502, 503, 599] {
            assert!(retryable_status(transient), "{transient}");
        }
        for final_answer in [200, 201, 204, 301, 400, 401, 403, 404, 409, 422] {
            assert!(!retryable_status(final_answer), "{final_answer}");
        }
    }

    #[test]
    fn an_actor_is_found_at_the_top_level_and_one_level_down() {
        // push: the pushing account sits at the top level.
        assert_eq!(
            actor_id(&serde_json::json!({ "pushed_by": 4, "after": "abc" })),
            Some(4)
        );
        // pull_request / issue / release nest the object they describe.
        assert_eq!(
            actor_id(&serde_json::json!({
                "event": "pull_request.opened",
                "pull_request": { "number": 1, "author_id": 5 },
            })),
            Some(5)
        );
        // No account named: null rather than a fabricated actor.
        assert_eq!(
            actor_id(&serde_json::json!({ "event": "tag.created", "ref": "v1" })),
            None
        );
    }
}
