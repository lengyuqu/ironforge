//! Work packages: immutable, numbered hand-over summaries of a task.
//!
//! Everything a package says (goal, completed items, "tests pass") is the
//! author's claim and is returned as such. Only `evidence_refs` that point at a
//! pipeline of this repository are resolved by the platform, and that result is
//! reported separately (`verified_reference` / `unavailable`) at read time.

use chrono::{DateTime, Utc};
use rg_db::entities::collab_work_package;
use rg_db::ops::collab_work_package_ops::{self, CreateOutcome};
use sea_orm::{DatabaseConnection, Set};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::ci_evidence::{self, PipelineEvidence};
use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::message::normalize_commit_sha;
use super::{task, validation, Actor, Page, ORIGIN_USER_CONTENT};

pub const MAX_ITEMS: usize = 50;
pub const MAX_ITEM_BYTES: usize = 2048;
pub const MAX_GOAL_BYTES: usize = 4096;
/// Serialized body must fit a portable `TEXT` column (MySQL: 64 KiB).
const MAX_BODY_JSON_BYTES: usize = 48 * 1024;
pub const EVIDENCE_KINDS: [&str; 4] = ["pipeline", "commit", "url", "note"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceRef {
    /// `pipeline` (id of a pipeline of this repository), `commit`, `url` or `note`.
    pub kind: String,
    pub reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Body {
    pub goal: String,
    pub scope: String,
    pub artifacts: Vec<String>,
    pub completed: Vec<String>,
    pub open_items: Vec<String>,
    pub next_steps: Vec<String>,
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone)]
pub struct NewPackage {
    pub client_package_id: String,
    /// When set, must equal the task's current version.
    pub expected_task_version: Option<i64>,
    pub commit_sha: Option<String>,
    pub body: Body,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PackageView {
    pub id: String,
    pub task_id: String,
    pub revision: i64,
    pub author_kind: String,
    pub author_id: String,
    pub task_version: i64,
    pub commit_sha: Option<String>,
    #[serde(flatten)]
    pub body: Body,
    /// What the author left out; saving incomplete drafts is allowed.
    pub missing_fields: Vec<&'static str>,
    pub client_package_id: String,
    pub created_at: DateTime<Utc>,
    /// Platform lookups of `pipeline` evidence; only on single reads. Claims in
    /// the body stay claims whatever this says.
    pub evidence_resolution: Option<Vec<PipelineEvidence>>,
    pub origin: &'static str,
}

fn missing_fields(body: &Body, commit_sha: &Option<String>) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if body.goal.trim().is_empty() {
        missing.push("goal");
    }
    if commit_sha.is_none() {
        missing.push("commit_sha");
    }
    if body.next_steps.is_empty() {
        missing.push("next_steps");
    }
    if body.evidence_refs.is_empty() {
        missing.push("evidence_refs");
    }
    missing
}

fn view(row: collab_work_package::Model) -> PackageView {
    let body: Body = serde_json::from_str(&row.body_json).unwrap_or_default();
    PackageView {
        missing_fields: missing_fields(&body, &row.commit_sha),
        body,
        id: row.id,
        task_id: row.task_id,
        revision: row.revision,
        author_kind: row.author_kind,
        author_id: row.author_id,
        task_version: row.task_version,
        commit_sha: row.commit_sha,
        client_package_id: row.client_package_id,
        created_at: row.created_at,
        evidence_resolution: None,
        origin: ORIGIN_USER_CONTENT,
    }
}

fn check_items(field: &str, items: &[String]) -> CollabResult<()> {
    if items.len() > MAX_ITEMS {
        return Err(CollabError::bad_request(format!(
            "{field} may have at most {MAX_ITEMS} entries"
        )));
    }
    for item in items {
        if item.trim().is_empty() {
            return Err(CollabError::bad_request(format!("{field} entries must not be empty")));
        }
        validation::text(field, item, 1, MAX_ITEM_BYTES)?;
    }
    Ok(())
}

fn validate(input: &NewPackage) -> CollabResult<()> {
    validation::client_key("client_package_id", &input.client_package_id)?;
    let b = &input.body;
    validation::text("goal", &b.goal, 0, MAX_GOAL_BYTES)?;
    validation::text("scope", &b.scope, 0, MAX_GOAL_BYTES)?;
    check_items("artifacts", &b.artifacts)?;
    check_items("completed", &b.completed)?;
    check_items("open_items", &b.open_items)?;
    check_items("next_steps", &b.next_steps)?;
    if b.evidence_refs.len() > MAX_ITEMS {
        return Err(CollabError::bad_request(format!(
            "evidence_refs may have at most {MAX_ITEMS} entries"
        )));
    }
    for e in &b.evidence_refs {
        if !EVIDENCE_KINDS.contains(&e.kind.as_str()) {
            return Err(CollabError::bad_request(format!(
                "evidence kind must be one of {}",
                EVIDENCE_KINDS.join(", ")
            )));
        }
        validation::text("evidence reference", &e.reference, 1, 512)?;
        if e.kind == "pipeline" && e.reference.parse::<i64>().map_or(true, |n| n <= 0) {
            return Err(CollabError::bad_request(
                "a pipeline evidence reference must be a pipeline id",
            ));
        }
        if e.kind == "commit" {
            normalize_commit_sha(&e.reference)?;
        }
    }
    Ok(())
}

fn payload_hash(task_id: &str, input: &NewPackage) -> String {
    let canonical = serde_json::json!([
        task_id,
        input.expected_task_version,
        input.commit_sha,
        input.body
    ]);
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

pub struct Created {
    pub package: PackageView,
    pub replayed: bool,
}

pub async fn create(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    actor: &Actor,
    mut input: NewPackage,
    now: DateTime<Utc>,
) -> CollabResult<Created> {
    validate(&input)?;
    if let Some(sha) = &input.commit_sha {
        input.commit_sha = Some(normalize_commit_sha(sha)?);
    }
    for e in &mut input.body.evidence_refs {
        if e.kind == "commit" {
            e.reference = normalize_commit_sha(&e.reference)?;
        }
    }
    let body_json =
        serde_json::to_string(&input.body).map_err(|e| CollabError::Internal(e.into()))?;
    if body_json.len() > MAX_BODY_JSON_BYTES {
        return Err(CollabError::bad_request("work package is too large"));
    }

    let task = task::find(db, repo_id, task_id).await?;
    if let Some(version) = input.expected_task_version {
        if version != task.version {
            return Err(CollabError::VersionConflict {
                current_version: task.version,
            });
        }
    }
    let hash = payload_hash(&task.id, &input);
    let outcome = collab_work_package_ops::create_next_revision(
        db,
        &task.id,
        actor.kind,
        &actor.id,
        &input.client_package_id,
        |revision| collab_work_package::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            repo_id: Set(repo_id),
            task_id: Set(task.id.clone()),
            revision: Set(revision),
            author_kind: Set(actor.kind.to_string()),
            author_id: Set(actor.id.clone()),
            task_version: Set(task.version),
            commit_sha: Set(input.commit_sha.clone()),
            body_json: Set(body_json.clone()),
            client_package_id: Set(input.client_package_id.clone()),
            payload_hash: Set(hash.clone()),
            created_at: Set(now),
        },
    )
    .await?;
    match outcome {
        CreateOutcome::Created(row) => Ok(Created { package: view(row), replayed: false }),
        CreateOutcome::Existing(row) if row.payload_hash == hash => {
            Ok(Created { package: view(row), replayed: true })
        }
        CreateOutcome::Existing(_) => Err(CollabError::IdempotencyKeyReused),
    }
}

/// One package with the platform's lookup of its pipeline evidence.
pub async fn get(
    db: &DatabaseConnection,
    repo_id: i64,
    id: &str,
    now: DateTime<Utc>,
) -> CollabResult<PackageView> {
    if !validation::is_uuid(id) {
        return Err(CollabError::not_found("work package not found"));
    }
    let row = collab_work_package_ops::find_in_repo(db, repo_id, id)
        .await?
        .ok_or_else(|| CollabError::not_found("work package not found"))?;
    let mut out = view(row);
    let mut resolved = Vec::new();
    for e in out.body.evidence_refs.iter().filter(|e| e.kind == "pipeline") {
        if let Ok(pipeline_id) = e.reference.parse::<i64>() {
            resolved.push(ci_evidence::resolve_pipeline(db, repo_id, pipeline_id, now).await?);
        }
    }
    out.evidence_resolution = Some(resolved);
    Ok(out)
}

fn list_scope(repo_id: i64, task_id: &str) -> String {
    format!("work-packages:v1:repo={repo_id}:task={task_id}:order=created_desc,id_desc")
}

/// Packages of a task, newest first.
pub async fn list(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    repo_id: i64,
    task_id: &str,
    limit: u64,
    cursor_token: Option<&str>,
) -> CollabResult<Page<PackageView>> {
    let task = task::find(db, repo_id, task_id).await?;
    let scope = list_scope(repo_id, &task.id);
    let after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| (p.at, p.id));
    let mut rows = collab_work_package_ops::list_page(db, repo_id, &task.id, after, limit + 1).await?;
    let has_more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let next_cursor = has_more.then(|| rows.last()).flatten().map(|last| {
        cursor::encode(
            cursor_key,
            &scope,
            &KeysetPosition { at: last.created_at, id: last.id.clone() },
        )
    });
    Ok(Page {
        items: rows.into_iter().map(view).collect(),
        next_cursor,
    })
}
