//! O09 repository-scoped, live search. The caller must enforce collaboration read
//! access on every request. No index, total count, or content snippets are returned.
use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use chrono::{DateTime, Utc};
use rg_db::entities::{
    collab_decision, collab_handoff, collab_message, collab_question, collab_task,
    collab_work_package,
};
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub kind: String,
    pub id: String,
    pub task_id: String,
    pub created_at: DateTime<Utc>,
    pub origin: &'static str,
}
#[derive(Debug, Serialize)]
pub struct SearchPage {
    pub items: Vec<SearchHit>,
    pub next_cursor: Option<String>,
    /// Time the live query began; separate pages are not a snapshot.
    pub queried_at: DateTime<Utc>,
    pub consistency: &'static str,
}

/// One object kind per request; `q` is a bounded literal substring. Matching
/// uses the database's LIKE collation. Message bodies and work package bodies
/// are searchable, as are their commit SHAs. Results never include content.
pub async fn search(
    db: &DatabaseConnection,
    repo_id: i64,
    key: &[u8],
    kind: &str,
    q: &str,
    limit: Option<u64>,
    token: Option<&str>,
) -> CollabResult<SearchPage> {
    let q = q.trim();
    if q.is_empty() || q.len() > 256 || q.contains(['%', '_', '\\', '\0']) {
        return Err(CollabError::bad_request(
            "q must be 1..256 bytes and contain no LIKE metacharacters",
        ));
    }
    if !matches!(
        kind,
        "task" | "question" | "decision" | "handoff" | "message" | "work_package"
    ) {
        return Err(CollabError::bad_request(
            "kind must be task, question, decision, handoff, message, or work_package",
        ));
    }
    // Hash the query before putting the scope into the signed (not encrypted) cursor.
    let digest = hex::encode(Sha256::digest(q.as_bytes()));
    let scope = format!("collab-search:v1:{repo_id}:{kind}:{digest}");
    let pos = token.map(|t| cursor::decode(key, &scope, t)).transpose()?;
    let limit = super::clamp_limit(limit);
    let queried_at = Utc::now();
    macro_rules! run {
        ($module:ident, $matches:expr, $task:expr) => {{
            use $module::{Column as C, Entity};
            let mut query = Entity::find()
                .filter(C::RepoId.eq(repo_id))
                .filter($matches);
            if let Some(p) = &pos {
                query = query.filter(
                    Condition::any().add(C::CreatedAt.lt(p.at)).add(
                        Condition::all()
                            .add(C::CreatedAt.eq(p.at))
                            .add(C::Id.lt(p.id.clone())),
                    ),
                );
            }
            let rows = query
                .order_by_desc(C::CreatedAt)
                .order_by_desc(C::Id)
                .limit(limit + 1)
                .all(db)
                .await?;
            rows.into_iter()
                .map(|r| {
                    let task_id: String = $task(&r);
                    SearchHit {
                        kind: kind.to_owned(),
                        id: r.id,
                        task_id,
                        created_at: r.created_at,
                        origin: super::ORIGIN_USER_CONTENT,
                    }
                })
                .collect::<Vec<_>>()
        }};
    }
    let mut items = match kind {
        "task" => run!(
            collab_task,
            Condition::any()
                .add(collab_task::Column::Title.contains(q))
                .add(collab_task::Column::Description.contains(q)),
            |r: &collab_task::Model| r.id.clone()
        ),
        "question" => run!(
            collab_question,
            Condition::any()
                .add(collab_question::Column::Title.contains(q))
                .add(collab_question::Column::Body.contains(q))
                .add(collab_question::Column::ContextJson.contains(q)),
            |r: &collab_question::Model| r.task_id.clone()
        ),
        "decision" => run!(
            collab_decision,
            Condition::any()
                .add(collab_decision::Column::Title.contains(q))
                .add(collab_decision::Column::Rationale.contains(q))
                .add(collab_decision::Column::ContextJson.contains(q)),
            |r: &collab_decision::Model| r.task_id.clone()
        ),
        "handoff" => run!(
            collab_handoff,
            Condition::any()
                .add(collab_handoff::Column::Summary.contains(q))
                .add(collab_handoff::Column::Completed.contains(q))
                .add(collab_handoff::Column::NextSteps.contains(q))
                .add(collab_handoff::Column::Blockers.contains(q))
                .add(collab_handoff::Column::CommitSha.contains(q)),
            |r: &collab_handoff::Model| r.task_id.clone()
        ),
        "message" => run!(
            collab_message,
            Condition::any()
                .add(collab_message::Column::Body.contains(q))
                .add(collab_message::Column::CommitSha.contains(q)),
            |r: &collab_message::Model| r.task_id.clone()
        ),
        "work_package" => run!(
            collab_work_package,
            Condition::any()
                .add(collab_work_package::Column::BodyJson.contains(q))
                .add(collab_work_package::Column::CommitSha.contains(q)),
            |r: &collab_work_package::Model| r.task_id.clone()
        ),
        _ => unreachable!(),
    };
    let next_cursor = if items.len() > limit as usize {
        items.truncate(limit as usize);
        let last = items.last().expect("limit is at least one");
        Some(cursor::encode(
            key,
            &scope,
            &KeysetPosition {
                at: last.created_at,
                id: last.id.clone(),
            },
        ))
    } else {
        None
    };
    Ok(SearchPage {
        items,
        next_cursor,
        queried_at,
        consistency: "live_query_not_snapshot",
    })
}
