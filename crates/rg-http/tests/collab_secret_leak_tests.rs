//! T09 (REST part): the session secret appears exactly once, in the
//! registration response, and nowhere else — not in later responses, error
//! bodies, server logs (captured at TRACE level) or the database.

mod common;
mod collab_common;

use std::sync::{Arc, Mutex};

use collab_common::{Env, Sess};
use reqwest::Method;
use rg_db::sea_orm::{ConnectionTrait, Statement};
use serde_json::Value;

#[derive(Clone, Default)]
struct LogBuf(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuf {
    type Writer = LogBuf;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t09_session_secret_never_leaves_the_registration_response() {
    let logs = LogBuf::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("install capture subscriber");

    let env = Env::new().await;
    let (a, created) = env.register_session(&env.owner_token, "leak-check").await;
    assert_eq!(created["session_secret"], a.secret.as_str());
    let secret = a.secret.clone();
    let bogus = "ifs_".to_string() + &"f".repeat(64);

    // Everything a client can do with the session, successes and failures.
    let mut responses: Vec<(String, Value)> = Vec::new();
    let sid = a.id.clone();
    let calls: Vec<(Method, String, Option<Sess>)> = vec![
        (Method::GET, env.url("/sessions"), None),
        (Method::POST, env.url(&format!("/sessions/{sid}/heartbeat")), Some(a.clone())),
        (
            Method::POST,
            env.url(&format!("/sessions/{sid}/heartbeat")),
            Some(Sess { id: sid.clone(), secret: bogus.clone() }),
        ),
        (Method::POST, env.url(&format!("/sessions/{sid}/heartbeat")), None),
        (Method::POST, env.url(&format!("/sessions/{sid}/end")), Some(a.clone())),
        (Method::POST, env.url(&format!("/sessions/{sid}/heartbeat")), Some(a.clone())),
    ];
    for (method, url, session) in calls {
        let (status, body) = env
            .json(method.clone(), &url, &env.owner_token, session.as_ref(), None)
            .await;
        responses.push((format!("{method} {url} -> {status}"), body));
    }
    // A second registration and its listing, too.
    env.register_session(&env.owner_token, "second").await;
    let (_, list) = env.json(Method::GET, &env.url("/sessions"), &env.owner_token, None, None).await;
    responses.push(("list".into(), list));

    for (what, body) in &responses {
        let text = body.to_string();
        assert!(!text.contains(&secret), "{what} leaked the secret: {text}");
        assert!(!text.contains(&bogus), "{what} echoed the presented secret: {text}");
        assert!(!text.contains("secret_hash"), "{what} exposes the digest field");
    }

    // The registration response of the *second* session carries only its own.
    // Server logs, captured for the whole flow.
    let captured = String::from_utf8_lossy(&logs.0.lock().unwrap()).to_string();
    assert!(captured.len() > 100, "log capture is not working: {captured:?}");
    assert!(!captured.contains(&secret), "server log contains the session secret");
    assert!(!captured.contains(&bogus), "server log contains a presented secret");

    // Database: neither the secret nor its raw value is stored anywhere.
    for table in ["agent_sessions", "audit_log", "collab_tasks", "collab_messages"] {
        let rows = env
            .db
            .query_all(Statement::from_string(
                env.db.get_database_backend(),
                format!("SELECT * FROM {table}"),
            ))
            .await
            .unwrap();
        for row in rows {
            let dump = format!("{row:?}");
            assert!(!dump.contains(&secret), "{table} row contains the secret: {dump}");
        }
    }
}
