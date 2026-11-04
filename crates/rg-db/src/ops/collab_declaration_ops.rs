//! Database operations for `collab_session_declarations`.
//!
//! One current row per session, replaced under an optimistic `version`. Each
//! write is a single statement (an `INSERT`, or a conditional `UPDATE` that is
//! the arbiter), so no transaction is needed.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::*;

use crate::entities::collab_session_declaration::{self, ActiveModel, Entity, Model as Declaration};

#[derive(Debug, Clone, PartialEq)]
pub enum PutOutcome {
    Saved(Declaration),
    /// The row is already what this request would have produced (its own retry).
    Replayed(Declaration),
    /// `version` is not the expected one; `current_version` is 0 when there is
    /// no declaration yet.
    Conflict { current_version: i64 },
}

pub async fn find<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    session_id: &str,
) -> Result<Option<Declaration>> {
    Entity::find_by_id(session_id.to_string())
        .filter(collab_session_declaration::Column::RepoId.eq(repo_id))
        .one(db)
        .await
        .context("db: find session declaration")
}

/// Replace the session's declaration iff it is still at `expected_version`
/// (0 = none yet). `model` carries the new content with `version` =
/// `expected_version + 1`.
pub async fn put(
    db: &DatabaseConnection,
    repo_id: i64,
    expected_version: i64,
    model: ActiveModel,
) -> Result<PutOutcome> {
    let session_id = model.session_id.clone().unwrap();
    let hash = model.payload_hash.clone().unwrap();

    if expected_version == 0 {
        return match model.insert(db).await {
            Ok(row) => Ok(PutOutcome::Saved(row)),
            Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
                resolve(db, repo_id, &session_id, expected_version, &hash).await
            }
            Err(err) => Err(err).context("db: insert session declaration"),
        };
    }

    let res = Entity::update_many()
        .col_expr(collab_session_declaration::Column::Version, Expr::value(expected_version + 1))
        .col_expr(collab_session_declaration::Column::ToolsJson, Expr::value(model.tools_json.clone().unwrap()))
        .col_expr(collab_session_declaration::Column::CanRunTests, Expr::value(model.can_run_tests.clone().unwrap()))
        .col_expr(collab_session_declaration::Column::Network, Expr::value(model.network.clone().unwrap()))
        .col_expr(collab_session_declaration::Column::NetworkNotes, Expr::value(model.network_notes.clone().unwrap()))
        .col_expr(collab_session_declaration::Column::Notes, Expr::value(model.notes.clone().unwrap()))
        .col_expr(collab_session_declaration::Column::PayloadHash, Expr::value(hash.clone()))
        .col_expr(collab_session_declaration::Column::DeclaredAt, Expr::value(model.declared_at.clone().unwrap()))
        .filter(collab_session_declaration::Column::SessionId.eq(session_id.clone()))
        .filter(collab_session_declaration::Column::RepoId.eq(repo_id))
        .filter(collab_session_declaration::Column::Version.eq(expected_version))
        .exec(db)
        .await
        .context("db: replace session declaration")?;
    if res.rows_affected == 1 {
        let row = find(db, repo_id, &session_id)
            .await?
            .context("db: declaration vanished after update")?;
        return Ok(PutOutcome::Saved(row));
    }
    resolve(db, repo_id, &session_id, expected_version, &hash).await
}

/// The write did not apply: it is either the retry of a request that did, or a
/// genuine version conflict.
async fn resolve(
    db: &DatabaseConnection,
    repo_id: i64,
    session_id: &str,
    expected_version: i64,
    hash: &str,
) -> Result<PutOutcome> {
    match find(db, repo_id, session_id).await? {
        Some(row) if row.version == expected_version + 1 && row.payload_hash == hash => {
            Ok(PutOutcome::Replayed(row))
        }
        Some(row) => Ok(PutOutcome::Conflict { current_version: row.version }),
        None => Ok(PutOutcome::Conflict { current_version: 0 }),
    }
}

/// Keyset page of the repository's declarations ordered by
/// `(declared_at, session_id)` descending.
pub async fn list_page<C: ConnectionTrait>(
    db: &C,
    repo_id: i64,
    after: Option<(DateTime<Utc>, String)>,
    limit: u64,
) -> Result<Vec<Declaration>> {
    let mut query = Entity::find().filter(collab_session_declaration::Column::RepoId.eq(repo_id));
    if let Some((at, id)) = after {
        query = query.filter(
            Condition::any()
                .add(collab_session_declaration::Column::DeclaredAt.lt(at))
                .add(
                    Condition::all()
                        .add(collab_session_declaration::Column::DeclaredAt.eq(at))
                        .add(collab_session_declaration::Column::SessionId.lt(id)),
                ),
        );
    }
    query
        .order_by_desc(collab_session_declaration::Column::DeclaredAt)
        .order_by_desc(collab_session_declaration::Column::SessionId)
        .limit(limit)
        .all(db)
        .await
        .context("db: list session declarations")
}
