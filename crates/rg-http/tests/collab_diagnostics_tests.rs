//! O10 diagnostics over the real HTTP router and an isolated test database.
mod collab_common;
mod common;

use chrono::{Duration, Utc};
use collab_common::Env;
use reqwest::Method;
use rg_db::entities::{agent_session, collab_handoff, collab_notice, collab_task};
use rg_db::sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::{json, Value};

fn finding<'a>(body: &'a Value, code: &str) -> Option<&'a Value> {
    body["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["code"] == code)
}

#[tokio::test]
async fn diagnostics_scope_parameters_and_offline_are_explicit() {
    let env = Env::new().await;
    let session = env.session(&env.owner_token, "owner").await;
    let (status, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&session),
            Some(json!({"title":"diagnose", "owner_session_id": session.id})),
        )
        .await;
    assert_eq!(status, 201, "{task}");
    let task_id = task["id"].as_str().unwrap();
    for suffix in [
        "/diagnostics".to_string(),
        format!("/diagnostics?task_id={task_id}&session_id={}", session.id),
    ] {
        assert_eq!(
            env.json(Method::GET, &env.url(&suffix), &env.owner_token, None, None)
                .await
                .0,
            400
        );
    }
    for suffix in [
        "/diagnostics?task_id=invalid".to_string(),
        "/diagnostics?session_id=invalid".to_string(),
    ] {
        assert_eq!(
            env.json(Method::GET, &env.url(&suffix), &env.owner_token, None, None)
                .await
                .0,
            404
        );
    }
    let (outsider, _) = env.user("outsider").await;
    let url = env.url(&format!("/diagnostics?task_id={task_id}"));
    assert_eq!(
        env.json(Method::GET, &url, &outsider, None, None).await.0,
        403
    );
    let response = env.send(Method::GET, &url, None, None, None).await;
    assert_eq!(response.status(), 401);
    common::create_repo(&env.base, &env.owner_token, "other-repo").await;
    for query in [
        format!("task_id={task_id}"),
        format!("session_id={}", session.id),
    ] {
        let url = env.url_in("other-repo", &format!("/diagnostics?{query}"));
        assert_eq!(
            env.json(Method::GET, &url, &env.owner_token, None, None)
                .await
                .0,
            404
        );
    }
    let row = agent_session::Entity::find_by_id(&session.id)
        .one(&env.db)
        .await
        .unwrap()
        .unwrap();
    let mut active: agent_session::ActiveModel = row.into();
    active.last_seen_at = Set(Utc::now() - Duration::minutes(10));
    active.update(&env.db).await.unwrap();
    let url = env.url(&format!("/diagnostics?session_id={}", session.id));
    let (status, body) = env
        .json(Method::GET, &url, &env.owner_token, None, None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["authority"], "derived_from_records");
    assert_eq!(
        finding(&body, "SESSION_OFFLINE").unwrap()["confidence"],
        "established"
    );
    assert_eq!(
        finding(&body, "SESSION_OFFLINE_CAUSE").unwrap()["confidence"],
        "unknown"
    );
    assert!(body["not_covered"].as_array().unwrap().len() >= 4);
    for f in body["findings"].as_array().unwrap() {
        assert!(f["source"]["record"].is_string(), "{f}");
    }
    let (_, task_diag) = env
        .json(
            Method::GET,
            &env.url(&format!("/diagnostics?task_id={task_id}")),
            &env.owner_token,
            None,
            None,
        )
        .await;
    assert!(finding(&task_diag, "OWNER_MAY_BE_ACTIVE").is_some());
}

#[tokio::test]
async fn diagnostics_reports_unconfirmed_then_receipt_without_changing_it() {
    let env = Env::new().await;
    let author = env.session(&env.owner_token, "author").await;
    let target = env.session(&env.owner_token, "target").await;
    let (_, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&author),
            Some(json!({"title":"delivery"})),
        )
        .await;
    let task_id = task["id"].as_str().unwrap();
    let (status, msg) = env.json(Method::POST, &env.url(&format!("/tasks/{task_id}/messages")), &env.owner_token, Some(&author), Some(json!({"client_message_id":"diagnostic-message","kind":"note","body":"hello","target_session_id":target.id}))).await;
    assert_eq!(status, 201, "{msg}");
    let message_id = msg["id"].as_str().unwrap();
    let (reader, reader_id) = env.user("reader").await;
    env.add_collaborator(reader_id, "read").await;
    let url = env.url(&format!("/diagnostics?task_id={task_id}"));
    let (_, before) = env.json(Method::GET, &url, &reader, None, None).await;
    assert!(finding(&before, "MESSAGES_UNCONFIRMED").is_none());
    let delivery_url = env.url(&format!("/tasks/{task_id}/messages/{message_id}/delivery"));
    let (_, delivery) = env
        .json(
            Method::GET,
            &delivery_url,
            &env.owner_token,
            Some(&author),
            None,
        )
        .await;
    assert_eq!(delivery["status"], "unknown");
    assert_eq!(
        env.json(
            Method::GET,
            &delivery_url,
            &env.owner_token,
            Some(&target),
            None
        )
        .await
        .0,
        403
    );
    let receipt_url = env.url(&format!(
        "/tasks/{task_id}/messages/{message_id}/read-receipt"
    ));
    assert_eq!(
        env.json(
            Method::POST,
            &receipt_url,
            &env.owner_token,
            Some(&target),
            None
        )
        .await
        .0,
        201
    );
    let (_, after) = env.json(Method::GET, &url, &reader, None, None).await;
    assert_eq!(
        before["findings"], after["findings"],
        "receipt changes must not be observable by a repository reader"
    );
    let (_, delivery) = env
        .json(
            Method::GET,
            &delivery_url,
            &env.owner_token,
            Some(&author),
            None,
        )
        .await;
    assert_eq!(delivery["status"], "read");
}

#[tokio::test]
async fn pending_handoffs_are_filtered_before_limit_and_notice_source_is_traceable() {
    let env = Env::new().await;
    let a = env.session(&env.owner_token, "a").await;
    let b = env.session(&env.owner_token, "b").await;
    let (_, task) = env
        .json(
            Method::POST,
            &env.url("/tasks"),
            &env.owner_token,
            Some(&a),
            Some(json!({"title":"handoffs"})),
        )
        .await;
    let tid = task["id"].as_str().unwrap();
    let (status, h) = env.json(Method::POST, &env.url(&format!("/tasks/{tid}/handoffs")), &env.owner_token, Some(&a), Some(json!({"client_handoff_id":"actual", "to_session_id":b.id, "summary":"s", "completed":"c", "next_steps":"n", "blockers":"b", "evidence":[]}))).await;
    assert_eq!(status, 201, "{h}");
    let hid = h["id"].as_str().unwrap();
    let original = collab_handoff::Entity::find_by_id(hid)
        .one(&env.db)
        .await
        .unwrap()
        .unwrap();
    for i in 0..200 {
        let mut row: collab_handoff::ActiveModel = original.clone().into();
        row.id = Set(uuid::Uuid::new_v4().to_string());
        row.client_handoff_id = Set(format!("old-{i}"));
        row.status = Set("cancelled".into());
        row.created_at = Set(original.created_at - Duration::minutes(1));
        row.insert(&env.db).await.unwrap();
    }
    let url = env.url(&format!("/diagnostics?task_id={tid}"));
    let (_, body) = env
        .json(Method::GET, &url, &env.owner_token, None, None)
        .await;
    assert_eq!(
        finding(&body, "HANDOFF_PENDING").unwrap()["source"]["id"],
        hid
    );
    let notice = collab_notice::Entity::find()
        .filter(collab_notice::Column::SourceId.eq(hid))
        .one(&env.db)
        .await
        .unwrap()
        .unwrap();
    let n = finding(&body, "HANDOFF_NOTICE").unwrap();
    assert_eq!(n["source"]["id"], notice.id);
    assert_eq!(n["details"]["source_id"], hid);
    let (_, session_diag) = env
        .json(
            Method::GET,
            &env.url(&format!("/diagnostics?session_id={}", b.id)),
            &env.owner_token,
            None,
            None,
        )
        .await;
    let summary = finding(&session_diag, "NOTICES_NOT_ACKNOWLEDGED").unwrap();
    assert!(summary["source"]["id"].is_null());
    assert_eq!(summary["details"]["recipient_session_id"], b.id);
    assert!(finding(&body, "HANDOFFS_TRUNCATED").is_none());
    for i in 0..20 {
        let mut row: collab_handoff::ActiveModel = original.clone().into();
        row.id = Set(uuid::Uuid::new_v4().to_string());
        row.client_handoff_id = Set(format!("pending-{i}"));
        row.insert(&env.db).await.unwrap();
    }
    let (_, body) = env
        .json(Method::GET, &url, &env.owner_token, None, None)
        .await;
    assert_eq!(
        finding(&body, "HANDOFFS_TRUNCATED").unwrap()["confidence"],
        "unknown"
    );
    assert_eq!(
        body["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["code"] == "HANDOFF_PENDING")
            .count(),
        20
    );
    let missing = body["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["code"] == "HANDOFF_NOTICE" && f["details"]["notice_state"] == "none_recorded")
        .unwrap();
    assert!(missing["source"]["id"].is_null());
    assert!(missing["details"]["source_id"].is_string());
}

#[tokio::test]
async fn done_tasks_never_suggest_immediate_claim() {
    let env = Env::new().await;
    let owner = env.session(&env.owner_token, "owner").await;
    for owned in [false, true] {
        let (_, task) = env.json(Method::POST, &env.url("/tasks"), &env.owner_token, Some(&owner), Some(json!({"title":"done", "owner_session_id": if owned {Some(&owner.id)} else {None}}))).await;
        let id = task["id"].as_str().unwrap();
        let row = collab_task::Entity::find_by_id(id)
            .one(&env.db)
            .await
            .unwrap()
            .unwrap();
        let mut row: collab_task::ActiveModel = row.into();
        row.status = Set("done".into());
        row.update(&env.db).await.unwrap();
        if owned {
            let row = agent_session::Entity::find_by_id(&owner.id)
                .one(&env.db)
                .await
                .unwrap()
                .unwrap();
            let mut row: agent_session::ActiveModel = row.into();
            row.expires_at = Set(Utc::now() - Duration::minutes(1));
            row.update(&env.db).await.unwrap();
        }
        let (_, body) = env
            .json(
                Method::GET,
                &env.url(&format!("/diagnostics?task_id={id}")),
                &env.owner_token,
                None,
                None,
            )
            .await;
        assert!(finding(&body, "TASK_DONE").unwrap()["statement"]
            .as_str()
            .unwrap()
            .contains("reopened"));
        assert!(finding(&body, "OWNER_CAN_BE_REPLACED").is_none());
        if !owned {
            assert!(finding(&body, "TASK_UNOWNED").unwrap()["statement"]
                .as_str()
                .unwrap()
                .contains("reopened"));
        }
    }
}
