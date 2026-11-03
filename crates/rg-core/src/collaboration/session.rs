//! Agent session lifecycle: register, authenticate, heartbeat, end, list.
//!
//! A session is a *traceable label* under one account, not a security
//! principal: the account's own credentials and repository permissions are
//! always checked by the caller first. The session secret only stops accidental
//! impersonation by someone who merely knows a session id.

use chrono::{DateTime, Duration, Utc};
use rg_db::entities::agent_session;
use rg_db::ops::agent_session_ops;
use sea_orm::{DatabaseConnection, Set};
use serde::Serialize;

use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::{secret, validation, Page, ORIGIN_USER_CONTENT};

/// Hard lifetime of a session. Heartbeats never extend it.
pub const SESSION_TTL: Duration = Duration::hours(8);
/// A session with a heartbeat this recent is shown as `online`.
pub const ONLINE_WINDOW: Duration = Duration::seconds(90);

pub const MAX_DISPLAY_NAME_CHARS: usize = 80;
pub const MAX_CLIENT_KIND_CHARS: usize = 64;

/// Public representation of a session. Never contains the secret or its digest.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionView {
    pub id: String,
    pub display_name: String,
    pub client_kind: String,
    /// `online` | `offline` | `expired` | `ended`. `offline` is not terminal.
    pub state: &'static str,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Name and kind are self-reported by the client.
    pub origin: &'static str,
}

/// Response of session registration: the only place the secret ever appears.
#[derive(Debug, Clone, Serialize)]
pub struct CreatedSession {
    pub session: SessionView,
    pub session_secret: String,
}

pub fn state_of(session: &agent_session::Model, now: DateTime<Utc>) -> &'static str {
    if session.ended_at.is_some() {
        "ended"
    } else if session.expires_at <= now {
        "expired"
    } else if now - session.last_seen_at <= ONLINE_WINDOW {
        "online"
    } else {
        "offline"
    }
}

pub fn view(session: &agent_session::Model, now: DateTime<Utc>) -> SessionView {
    SessionView {
        id: session.id.clone(),
        display_name: session.display_name.clone(),
        client_kind: session.client_kind.clone(),
        state: state_of(session, now),
        created_at: session.created_at,
        last_seen_at: session.last_seen_at,
        expires_at: session.expires_at,
        ended_at: session.ended_at,
        origin: ORIGIN_USER_CONTENT,
    }
}

/// Register a new session for `user_id` in `repo_id`. The caller has already
/// established write access.
pub async fn create(
    db: &DatabaseConnection,
    repo_id: i64,
    user_id: i64,
    display_name: &str,
    client_kind: &str,
    now: DateTime<Utc>,
) -> CollabResult<CreatedSession> {
    let display_name = validation::label("display_name", display_name, MAX_DISPLAY_NAME_CHARS)?;
    let client_kind = validation::label("client_kind", client_kind, MAX_CLIENT_KIND_CHARS)?;

    let id = uuid::Uuid::new_v4().to_string();
    let session_secret = secret::generate();
    let row = agent_session_ops::create(
        db,
        agent_session::ActiveModel {
            id: Set(id.clone()),
            repo_id: Set(repo_id),
            user_id: Set(user_id),
            secret_hash: Set(secret::hash(&id, &session_secret)),
            display_name: Set(display_name),
            client_kind: Set(client_kind),
            created_at: Set(now),
            last_seen_at: Set(now),
            expires_at: Set(now + SESSION_TTL),
            ended_at: Set(None),
        },
    )
    .await?;
    Ok(CreatedSession {
        session: view(&row, now),
        session_secret,
    })
}

/// Check the id/secret pair without looking at the session's lifecycle.
///
/// Unknown id, other repository, other account and wrong secret are all the same
/// [`CollabError::SessionCredentials`], so the response cannot be used to probe
/// which session ids exist.
pub async fn verify_credentials(
    db: &DatabaseConnection,
    repo_id: i64,
    user_id: i64,
    session_id: &str,
    session_secret: &str,
) -> CollabResult<agent_session::Model> {
    if !validation::is_uuid(session_id) {
        return Err(CollabError::SessionCredentials);
    }
    let session = agent_session_ops::find_in_repo(db, repo_id, session_id)
        .await?
        .ok_or(CollabError::SessionCredentials)?;
    // Evaluate both so timing does not reveal which half failed.
    let secret_ok = secret::verify(&session.id, session_secret, &session.secret_hash);
    let owner_ok = session.user_id == user_id;
    if secret_ok && owner_ok {
        Ok(session)
    } else {
        Err(CollabError::SessionCredentials)
    }
}

/// Verify credentials and require a live session (not ended, not expired).
pub async fn authenticate(
    db: &DatabaseConnection,
    repo_id: i64,
    user_id: i64,
    session_id: &str,
    session_secret: &str,
    now: DateTime<Utc>,
) -> CollabResult<agent_session::Model> {
    let session = verify_credentials(db, repo_id, user_id, session_id, session_secret).await?;
    if session.ended_at.is_some() || session.expires_at <= now {
        return Err(CollabError::SessionExpired);
    }
    Ok(session)
}

/// Record a heartbeat for the caller's own session.
pub async fn heartbeat(
    db: &DatabaseConnection,
    repo_id: i64,
    user_id: i64,
    session_id: &str,
    session_secret: &str,
    now: DateTime<Utc>,
) -> CollabResult<SessionView> {
    authenticate(db, repo_id, user_id, session_id, session_secret, now).await?;
    if agent_session_ops::touch(db, repo_id, session_id, now).await? == 0 {
        // Ended or expired between the check and the update.
        return Err(CollabError::SessionExpired);
    }
    let session = agent_session_ops::find_in_repo(db, repo_id, session_id)
        .await?
        .ok_or(CollabError::SessionCredentials)?;
    Ok(view(&session, now))
}

/// End the caller's own session. Idempotent: ending an ended or expired session
/// with valid credentials succeeds.
pub async fn end(
    db: &DatabaseConnection,
    repo_id: i64,
    user_id: i64,
    session_id: &str,
    session_secret: &str,
    now: DateTime<Utc>,
) -> CollabResult<()> {
    verify_credentials(db, repo_id, user_id, session_id, session_secret).await?;
    agent_session_ops::end(db, repo_id, session_id, now).await?;
    Ok(())
}

fn list_scope(repo_id: i64) -> String {
    format!("sessions:v1:repo={repo_id}:order=last_seen_desc,id_desc")
}

/// One page of the repository's sessions, most recently seen first.
pub async fn list(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    repo_id: i64,
    limit: u64,
    cursor_token: Option<&str>,
    now: DateTime<Utc>,
) -> CollabResult<Page<SessionView>> {
    let scope = list_scope(repo_id);
    let after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| (p.at, p.id));

    let mut rows = agent_session_ops::list_page(db, repo_id, after, limit + 1).await?;
    let has_more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let next_cursor = if has_more {
        rows.last().map(|last| {
            cursor::encode(
                cursor_key,
                &scope,
                &KeysetPosition {
                    at: last.last_seen_at,
                    id: last.id.clone(),
                },
            )
        })
    } else {
        None
    };
    Ok(Page {
        items: rows.iter().map(|row| view(row, now)).collect(),
        next_cursor,
    })
}
