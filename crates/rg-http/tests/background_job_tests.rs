//! QUEUE-001 integration tests: durable background job queue wired into
//! webhook delivery retries.

mod common;

use chrono::Utc;
use common::setup_test_db;
use rg_core::background_jobs::{BackgroundJobQueue, TASK_WEBHOOK_DELIVER};
use rg_core::webhook::service::{
    create_webhook, deliver_retry_job, trigger_event, CreateWebhookRequest,
};
use rg_db::entities::background_job::status;
use rg_db::ops::{background_job_ops, user_ops, webhook_ops};
use sea_orm::{ActiveValue::Set, NotSet};

async fn seed_repo(db: &rg_db::DatabaseConnection) -> i64 {
    let user = user_ops::create_user(db, "job-owner", "job-owner@example.com", "", "Job Owner")
        .await
        .unwrap();
    let now = Utc::now();
    let repo = rg_db::ops::repo_ops::create(
        db,
        rg_db::entities::repository::ActiveModel {
            id: NotSet,
            owner_id: Set(user.id),
            name: Set("jobs-repo".to_string()),
            description: Set(None),
            is_private: Set(false),
            default_branch: Set("main".to_string()),
            fork_id: Set(None),
            stars_count: Set(0),
            forks_count: Set(0),
            org_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
            origin_repo_id: Set(None),
        },
    )
    .await
    .unwrap();
    repo.id
}

/// Bind a port and drop the listener: the next connection to it is refused,
/// giving a reliably unreachable HTTP endpoint.
async fn unreachable_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// Minimal HTTP server that answers every request with 200 "ok".
async fn spawn_ok_server() -> (u16, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                    .await;
            });
        }
    });
    (port, handle)
}

#[tokio::test]
async fn failed_webhook_delivery_enqueues_durable_retry_and_redelivery_succeeds() {
    let (db, _dir) = setup_test_db().await;
    let repo_id = seed_repo(&db).await;

    // Webhook pointing at a dead port: the initial (fire-and-forget) delivery
    // fails and must enqueue a retry job.
    let dead_port = unreachable_port().await;
    let hook = create_webhook(
        &db,
        repo_id,
        &CreateWebhookRequest {
            url: format!("http://127.0.0.1:{dead_port}/hook"),
            content_type: Some("json".to_string()),
            secret: None,
            active: Some(true),
            events: vec!["push".to_string()],
        },
    )
    .await
    .unwrap();

    trigger_event(&db, repo_id, "push", &serde_json::json!({"after": "abc"}))
        .await
        .unwrap();

    // Wait (bounded) for the spawned delivery to fail and enqueue the retry.
    let mut retry = None;
    for _ in 0..100 {
        let pending = background_job_ops::list_by_status(&db, status::PENDING, 10)
            .await
            .unwrap();
        if let Some(job) = pending
            .iter()
            .find(|job| job.task_type == TASK_WEBHOOK_DELIVER)
        {
            retry = Some(job.clone());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let retry = retry.expect("webhook.deliver retry job was not enqueued");
    let payload: serde_json::Value = serde_json::from_str(&retry.payload).unwrap();
    assert_eq!(payload["webhook_id"].as_i64(), Some(hook.id));
    assert_eq!(payload["event"].as_str(), Some("push"));

    // Point the webhook at a live endpoint and run the retry handler.
    let (port, _server) = spawn_ok_server().await;
    let mut model: rg_db::entities::webhook::ActiveModel = hook.clone().into();
    model.url = Set(format!("http://127.0.0.1:{port}/hook"));
    webhook_ops::update_webhook(&db, model).await.unwrap();

    deliver_retry_job(&db, &payload).await.unwrap();

    // Each attempt records a delivery row; the retry one has status 200.
    let deliveries = webhook_ops::list_deliveries_by_webhook(&db, hook.id)
        .await
        .unwrap();
    assert!(deliveries.len() >= 2, "expected initial + retry deliveries");
    assert_eq!(
        deliveries[0].response_status,
        Some(200),
        "latest delivery should be the successful retry"
    );
}

#[tokio::test]
async fn deliver_retry_job_fails_and_records_delivery_for_unreachable_url() {
    let (db, _dir) = setup_test_db().await;
    let repo_id = seed_repo(&db).await;

    let dead_port = unreachable_port().await;
    let hook = create_webhook(
        &db,
        repo_id,
        &CreateWebhookRequest {
            url: format!("http://127.0.0.1:{dead_port}/hook"),
            content_type: Some("json".to_string()),
            secret: None,
            active: Some(true),
            events: vec!["push".to_string()],
        },
    )
    .await
    .unwrap();

    let payload = serde_json::json!({
        "webhook_id": hook.id,
        "event": "push",
        "payload": "{}",
    });
    let result = deliver_retry_job(&db, &payload).await;
    assert!(result.is_err(), "unreachable webhook must fail the job");

    let deliveries = webhook_ops::list_deliveries_by_webhook(&db, hook.id)
        .await
        .unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].response_status, None);
    assert!(deliveries[0]
        .response_body
        .as_deref()
        .unwrap()
        .contains("delivery error"));
}

#[tokio::test]
async fn deliver_retry_job_resolves_when_webhook_was_deleted() {
    let (db, _dir) = setup_test_db().await;
    let repo_id = seed_repo(&db).await;

    let hook = create_webhook(
        &db,
        repo_id,
        &CreateWebhookRequest {
            url: "http://127.0.0.1:1/hook".to_string(),
            content_type: Some("json".to_string()),
            secret: None,
            active: Some(true),
            events: vec!["push".to_string()],
        },
    )
    .await
    .unwrap();
    webhook_ops::delete_webhook_by_id(&db, hook.id)
        .await
        .unwrap();

    // A retry scheduled before deletion completes without redelivery.
    let payload = serde_json::json!({
        "webhook_id": hook.id,
        "event": "push",
        "payload": "{}",
    });
    deliver_retry_job(&db, &payload).await.unwrap();

    let deliveries = webhook_ops::list_deliveries_by_webhook(&db, hook.id)
        .await
        .unwrap();
    assert!(deliveries.is_empty(), "deleted webhook must not be redelivered");

    // Worker round trip: the completed job is marked succeeded.
    let queue = BackgroundJobQueue::new(db.clone());
    let job = queue.enqueue(TASK_WEBHOOK_DELIVER, &payload).await.unwrap();
    let claimed = queue.claim_next("test-worker").await.unwrap().unwrap();
    assert_eq!(claimed.id, job.id);
    queue.complete(job.id).await.unwrap();
}
