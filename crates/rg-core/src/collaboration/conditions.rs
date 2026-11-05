//! User-defined completion conditions (O04), shown next to the task's claim.
//!
//! A *display* feature: nothing here blocks, changes or approves anything. An
//! agent's `done` stays a declaration; each condition says what platform
//! records can show for it right now — `satisfied`, `unsatisfied`, `stale`
//! (the evidence belongs to an outdated version) or `unknown` (nothing the
//! platform can check) — and who defined it.
//!
//! Definitions are immutable versions: every change inserts the task's next
//! `version`, guarded by the version the writer last saw, so history stays
//! traceable and a concurrent edit is a conflict instead of a silent merge.
//! A live evaluation is not history; [`record_snapshot`] stores one as it was.
//!
//! Condition types:
//! * `question_answered {question_id}` — the question is answered and its
//!   context is still fresh; an answer given for an outdated context is
//!   `stale`, an unjudgeable one `unknown`;
//! * `work_package_present {work_package_id}` — that package exists and is the
//!   task's newest revision (a package proves only that a package exists);
//! * `ci_passed_for_commit {commit_sha}` — the latest platform-recorded
//!   pipeline for exactly that commit succeeded (the agent's own statements and
//!   commit statuses are never evidence);
//! * `manual_confirmation {}` — an authorised account explicitly confirmed it
//!   (see [`confirm`]). That is a collaboration record, not a merge approval,
//!   and `actor_kind` never proves a human was behind it.

use chrono::{DateTime, Utc};
use rg_db::entities::{
    collab_completion_condition_set as set_entity, collab_completion_confirmation as conf_entity,
    collab_completion_snapshot as snap_entity,
};
use rg_db::ops::collab_completion_ops::{self as ops, InsertOutcome};
use rg_db::ops::{collab_work_package_ops, pipeline_ops};
use sea_orm::{DatabaseConnection, Set};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::ci_evidence::PipelineEvidence;
use super::completion::{ci_condition, SATISFIED, STALE, UNKNOWN, UNSATISFIED};
use super::error::{CollabError, CollabResult};
use super::message::normalize_commit_sha;
use super::{completion, question, task, validation, Actor, ORIGIN_USER_CONTENT};

pub const MAX_CONDITIONS: usize = 20;
pub const MAX_LABEL_CHARS: usize = 200;
pub const MAX_NOTE_BYTES: usize = 1000;
pub const MAX_LISTED: u64 = 100;
/// Snapshots are immutable and never deleted, so one task may keep only this
/// many. The check is count-then-insert (not atomic): concurrent callers can
/// overshoot by the number of racing requests, which is acceptable for a
/// storage guard. At the cap the caller gets `SNAPSHOT_LIMIT_REACHED`.
pub const MAX_SNAPSHOTS_PER_TASK: u64 = 200;
/// Condition-set versions are immutable and never deleted either: a task keeps
/// at most this many (the next write after version N fails with
/// `CONDITION_VERSION_LIMIT_REACHED`; the version check is exact, not a race).
pub const MAX_SET_VERSIONS_PER_TASK: i64 = 200;
/// Manual confirmations per task (count-then-insert, like the snapshot cap).
pub const MAX_CONFIRMATIONS_PER_TASK: u64 = 200;

pub const QUESTION_ANSWERED: &str = "question_answered";
pub const WORK_PACKAGE_PRESENT: &str = "work_package_present";
pub const CI_PASSED_FOR_COMMIT: &str = "ci_passed_for_commit";
pub const MANUAL_CONFIRMATION: &str = "manual_confirmation";
const TYPES: [&str; 4] = [
    QUESTION_ANSWERED,
    WORK_PACKAGE_PRESENT,
    CI_PASSED_FOR_COMMIT,
    MANUAL_CONFIRMATION,
];

/// One condition as a client submits it. An `id` of an existing condition keeps
/// its identity (and its place in the history); no `id` creates a new one.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConditionInput {
    pub id: Option<String>,
    pub r#type: String,
    pub label: Option<String>,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone)]
pub struct NewConditionSet {
    pub client_request_id: String,
    /// The version the writer last read (0 = none yet).
    pub expected_version: i64,
    pub conditions: Vec<ConditionInput>,
}

/// A condition as stored.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Condition {
    pub id: String,
    pub r#type: String,
    pub label: String,
    pub params: Value,
    /// The set version in which this condition first appeared.
    pub introduced_in_version: i64,
    pub created_by_kind: String,
    pub created_by_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConditionSetView {
    pub task_id: String,
    /// 0 = the task has no defined conditions.
    pub version: i64,
    pub conditions: Vec<Condition>,
    /// Who wrote this version: a claim, not proof of who or what they are.
    pub created_by_kind: Option<String>,
    pub created_by_id: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetCreated {
    pub set: ConditionSetView,
    pub replayed: bool,
}

fn view(task_id: &str, row: Option<set_entity::Model>) -> CollabResult<ConditionSetView> {
    Ok(match row {
        None => ConditionSetView {
            task_id: task_id.to_string(),
            version: 0,
            conditions: Vec::new(),
            created_by_kind: None,
            created_by_id: None,
            created_at: None,
            origin: ORIGIN_USER_CONTENT,
        },
        Some(row) => ConditionSetView {
            task_id: row.task_id,
            version: row.version,
            conditions: serde_json::from_str(&row.conditions_json)
                .map_err(|e| CollabError::Internal(e.into()))?,
            created_by_kind: Some(row.actor_kind),
            created_by_id: Some(row.actor_id),
            created_at: Some(row.created_at),
            origin: ORIGIN_USER_CONTENT,
        },
    })
}

fn hash(parts: &Value) -> String {
    hex::encode(Sha256::digest(parts.to_string().as_bytes()))
}

fn only_keys(params: &Value, keys: &[&str], kind: &str) -> CollabResult<()> {
    let object = params
        .as_object()
        .ok_or_else(|| CollabError::bad_request(format!("{kind}: params must be an object")))?;
    if let Some(extra) = object.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(CollabError::bad_request(format!(
            "{kind}: unknown param `{extra}`"
        )));
    }
    if let Some(missing) = keys.iter().find(|k| !object.contains_key(**k)) {
        return Err(CollabError::bad_request(format!(
            "{kind}: param `{missing}` is required"
        )));
    }
    Ok(())
}

fn str_param<'a>(params: &'a Value, key: &str) -> CollabResult<&'a str> {
    params[key]
        .as_str()
        .ok_or_else(|| CollabError::bad_request(format!("param `{key}` must be a string")))
}

/// Check one condition's parameters and the objects they point at. Returns the
/// params in their canonical form (commit ids lower-cased).
async fn check_params(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    kind: &str,
    params: &Value,
) -> CollabResult<Value> {
    let params = if params.is_null() {
        json!({})
    } else {
        params.clone()
    };
    match kind {
        QUESTION_ANSWERED => {
            only_keys(&params, &["question_id"], kind)?;
            let id = str_param(&params, "question_id")?;
            let q = question::find(db, repo_id, id)
                .await
                .map_err(|err| match err {
                    CollabError::NotFound(_) => {
                        CollabError::bad_request("question_id must be a question of this task")
                    }
                    other => other,
                })?;
            if q.task_id != task_id {
                return Err(CollabError::bad_request(
                    "question_id must be a question of this task",
                ));
            }
            Ok(params)
        }
        WORK_PACKAGE_PRESENT => {
            only_keys(&params, &["work_package_id"], kind)?;
            let id = str_param(&params, "work_package_id")?;
            let found = if validation::is_uuid(id) {
                collab_work_package_ops::find_in_repo(db, repo_id, id).await?
            } else {
                None
            };
            if !found.is_some_and(|p| p.task_id == task_id) {
                return Err(CollabError::bad_request(
                    "work_package_id must be a work package of this task",
                ));
            }
            Ok(params)
        }
        CI_PASSED_FOR_COMMIT => {
            only_keys(&params, &["commit_sha"], kind)?;
            let sha = normalize_commit_sha(str_param(&params, "commit_sha")?)?;
            Ok(json!({ "commit_sha": sha }))
        }
        MANUAL_CONFIRMATION => {
            only_keys(&params, &[], kind)?;
            Ok(json!({}))
        }
        other => Err(CollabError::bad_request(format!(
            "unknown condition type `{other}`; use one of {}",
            TYPES.join(", ")
        ))),
    }
}

/// The task's current definition (version 0 and no conditions when none).
pub async fn get_current(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
) -> CollabResult<ConditionSetView> {
    let task = task::find(db, repo_id, task_id).await?;
    view(&task.id, ops::current_set(db, &task.id).await?)
}

/// Older versions of the definition, newest first.
pub async fn list_versions(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    before_version: Option<i64>,
    limit: u64,
) -> CollabResult<Vec<ConditionSetView>> {
    let task = task::find(db, repo_id, task_id).await?;
    ops::list_sets(db, &task.id, before_version, limit.clamp(1, MAX_LISTED))
        .await?
        .into_iter()
        .map(|row| view(&task.id, Some(row)))
        .collect()
}

pub async fn get_version(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    version: i64,
) -> CollabResult<ConditionSetView> {
    let task = task::find(db, repo_id, task_id).await?;
    let row = ops::get_set(db, &task.id, version)
        .await?
        .ok_or_else(|| CollabError::not_found("condition set version not found"))?;
    view(&task.id, Some(row))
}

/// Replace the definition with a new version. `expected_version` must be the
/// current one; a stale writer gets `VERSION_CONFLICT` with the current version
/// and changes nothing. Retrying with the same `client_request_id` and body
/// returns the version it created.
pub async fn set_conditions(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    actor: &Actor,
    input: NewConditionSet,
    now: DateTime<Utc>,
) -> CollabResult<SetCreated> {
    validation::client_key("client_request_id", &input.client_request_id)?;
    if input.expected_version < 0 {
        return Err(CollabError::bad_request(
            "expected_version must not be negative",
        ));
    }
    if input.conditions.len() > MAX_CONDITIONS {
        return Err(CollabError::bad_request(format!(
            "at most {MAX_CONDITIONS} conditions are allowed"
        )));
    }
    let task = task::find(db, repo_id, task_id).await?;
    let request_hash = hash(&json!([
        task.id,
        input.expected_version,
        serde_json::to_value(&input.conditions).map_err(|e| CollabError::Internal(e.into()))?,
    ]));
    // A retry that already landed is a replay whatever the task looks like now.
    if let Some(existing) = ops::find_set_by_request(db, &task.id, &input.client_request_id).await?
    {
        return if existing.payload_hash == request_hash {
            Ok(SetCreated {
                set: view(&task.id, Some(existing))?,
                replayed: true,
            })
        } else {
            Err(CollabError::IdempotencyKeyReused)
        };
    }

    let current = ops::current_set(db, &task.id).await?;
    let current_version = current.as_ref().map_or(0, |c| c.version);
    if input.expected_version != current_version {
        return Err(CollabError::VersionConflict { current_version });
    }
    if current_version >= MAX_SET_VERSIONS_PER_TASK {
        return Err(CollabError::CodedConflict {
            code: "CONDITION_VERSION_LIMIT_REACHED",
            message: format!(
                "this task already has {current_version} condition-set versions (limit {MAX_SET_VERSIONS_PER_TASK})"
            ),
            details: Some(json!({ "limit": MAX_SET_VERSIONS_PER_TASK, "version": current_version })),
        });
    }
    let previous: Vec<Condition> = match &current {
        Some(row) => serde_json::from_str(&row.conditions_json)
            .map_err(|e| CollabError::Internal(e.into()))?,
        None => Vec::new(),
    };
    let next_version = current_version + 1;

    let mut seen = std::collections::BTreeSet::new();
    let mut conditions = Vec::with_capacity(input.conditions.len());
    for item in input.conditions {
        let label = match item.label.as_deref() {
            Some(l) => validation::label("label", l, MAX_LABEL_CHARS)?,
            None => String::new(),
        };
        let params = check_params(db, repo_id, &task.id, &item.r#type, &item.params).await?;
        let condition = match item.id {
            Some(id) => {
                if !seen.insert(id.clone()) {
                    return Err(CollabError::bad_request(format!(
                        "condition id `{id}` appears twice"
                    )));
                }
                let old = previous.iter().find(|c| c.id == id).ok_or_else(|| {
                    CollabError::bad_request(format!(
                        "condition id `{id}` is not in version {current_version}"
                    ))
                })?;
                if old.r#type != item.r#type {
                    return Err(CollabError::bad_request(
                        "a condition keeps its type; add a new condition instead",
                    ));
                }
                Condition {
                    label,
                    params,
                    ..old.clone()
                }
            }
            None => Condition {
                id: uuid::Uuid::new_v4().to_string(),
                r#type: item.r#type,
                label,
                params,
                introduced_in_version: next_version,
                created_by_kind: actor.kind.to_string(),
                created_by_id: actor.id.clone(),
            },
        };
        conditions.push(condition);
    }

    let model = set_entity::ActiveModel {
        id: Set(uuid::Uuid::new_v4().to_string()),
        repo_id: Set(repo_id),
        task_id: Set(task.id.clone()),
        version: Set(next_version),
        conditions_json: Set(
            serde_json::to_string(&conditions).map_err(|e| CollabError::Internal(e.into()))?
        ),
        actor_kind: Set(actor.kind.to_string()),
        actor_id: Set(actor.id.clone()),
        client_request_id: Set(input.client_request_id.clone()),
        payload_hash: Set(request_hash.clone()),
        created_at: Set(super::creation_timestamp(db, now)),
    };
    match ops::insert_set(db, model, &task.id, &input.client_request_id).await? {
        InsertOutcome::Inserted(row) => Ok(SetCreated {
            set: view(&task.id, Some(row))?,
            replayed: false,
        }),
        InsertOutcome::Replay(row) if row.payload_hash == request_hash => Ok(SetCreated {
            set: view(&task.id, Some(row))?,
            replayed: true,
        }),
        InsertOutcome::Replay(_) => Err(CollabError::IdempotencyKeyReused),
        InsertOutcome::VersionTaken => Err(CollabError::VersionConflict {
            current_version: ops::current_set(db, &task.id)
                .await?
                .map_or(0, |c| c.version),
        }),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewConfirmation {
    pub client_request_id: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConfirmationView {
    pub id: String,
    pub task_id: String,
    pub condition_id: String,
    pub condition_set_version: i64,
    /// The task version and branch head the confirmer was looking at.
    pub task_version: i64,
    pub head_sha: Option<String>,
    pub note: String,
    /// The confirming account. Not proof that a person acted: any client under
    /// the account can call this.
    pub confirmed_by_user_id: i64,
    pub created_at: DateTime<Utc>,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfirmationCreated {
    pub confirmation: ConfirmationView,
    pub replayed: bool,
}

fn confirmation_view(row: conf_entity::Model) -> ConfirmationView {
    ConfirmationView {
        id: row.id,
        task_id: row.task_id,
        condition_id: row.condition_id,
        condition_set_version: row.condition_set_version,
        task_version: row.task_version,
        head_sha: row.head_sha,
        note: row.note,
        confirmed_by_user_id: row.actor_user_id,
        created_at: row.created_at,
        origin: ORIGIN_USER_CONTENT,
    }
}

/// Record an explicit confirmation of a `manual_confirmation` condition of the
/// current definition. Idempotent per `client_request_id`. The confirmation is
/// tied to the branch head observed now and stops counting when the branch
/// moves.
#[allow(clippy::too_many_arguments)]
pub async fn confirm(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    condition_id: &str,
    user_id: i64,
    input: NewConfirmation,
    branch_head: Option<String>,
    now: DateTime<Utc>,
) -> CollabResult<ConfirmationCreated> {
    validation::client_key("client_request_id", &input.client_request_id)?;
    let note = input.note.unwrap_or_default();
    validation::text("note", &note, 0, MAX_NOTE_BYTES)?;
    let task = task::find(db, repo_id, task_id).await?;
    let request_hash = hash(&json!([task.id, condition_id, user_id, note]));
    if let Some(existing) =
        ops::find_confirmation_by_request(db, &task.id, &input.client_request_id).await?
    {
        return if existing.payload_hash == request_hash {
            Ok(ConfirmationCreated {
                confirmation: confirmation_view(existing),
                replayed: true,
            })
        } else {
            Err(CollabError::IdempotencyKeyReused)
        };
    }
    let current = ops::current_set(db, &task.id).await?;
    let defined = current.as_ref().and_then(|row| {
        serde_json::from_str::<Vec<Condition>>(&row.conditions_json)
            .ok()?
            .into_iter()
            .find(|c| c.id == condition_id && c.r#type == MANUAL_CONFIRMATION)
    });
    let (Some(_), Some(set)) = (defined, current) else {
        return Err(CollabError::not_found(
            "no manual_confirmation condition with this id in the current definition",
        ));
    };
    let stored = ops::count_confirmations(db, &task.id).await?;
    if stored >= MAX_CONFIRMATIONS_PER_TASK {
        return Err(CollabError::CodedConflict {
            code: "CONFIRMATION_LIMIT_REACHED",
            message: format!(
                "this task already has {stored} confirmations (limit {MAX_CONFIRMATIONS_PER_TASK})"
            ),
            details: Some(json!({ "limit": MAX_CONFIRMATIONS_PER_TASK, "stored": stored })),
        });
    }
    let model = conf_entity::ActiveModel {
        id: Set(uuid::Uuid::new_v4().to_string()),
        repo_id: Set(repo_id),
        task_id: Set(task.id.clone()),
        condition_id: Set(condition_id.to_string()),
        condition_set_version: Set(set.version),
        task_version: Set(task.version),
        head_sha: Set(branch_head),
        note: Set(note),
        actor_user_id: Set(user_id),
        client_request_id: Set(input.client_request_id.clone()),
        payload_hash: Set(request_hash.clone()),
        created_at: Set(super::creation_timestamp(db, now)),
    };
    match ops::insert_confirmation(db, model, &task.id, &input.client_request_id).await? {
        InsertOutcome::Inserted(row) => Ok(ConfirmationCreated {
            confirmation: confirmation_view(row),
            replayed: false,
        }),
        InsertOutcome::Replay(row) if row.payload_hash == request_hash => Ok(ConfirmationCreated {
            confirmation: confirmation_view(row),
            replayed: true,
        }),
        InsertOutcome::Replay(_) => Err(CollabError::IdempotencyKeyReused),
        InsertOutcome::VersionTaken => Err(CollabError::Internal(anyhow::anyhow!(
            "confirmation insert reported a version conflict"
        ))),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DefinedCondition {
    #[serde(flatten)]
    pub condition: Condition,
    /// `satisfied` | `unsatisfied` | `stale` | `unknown`.
    pub status: &'static str,
    pub detail: String,
    pub evidence: Vec<PipelineEvidence>,
    /// For `manual_confirmation`: the confirmation that was weighed.
    pub confirmation: Option<ConfirmationView>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DefinedStatus {
    pub condition_set_version: i64,
    pub conditions: Vec<DefinedCondition>,
    /// `all_satisfied` | `unsatisfied` | `incomplete`.
    pub overall: &'static str,
}

fn overall_of(conditions: &[DefinedCondition]) -> &'static str {
    if conditions.iter().any(|c| c.status == UNSATISFIED) {
        "unsatisfied"
    } else if conditions.iter().all(|c| c.status == SATISFIED) {
        "all_satisfied"
    } else {
        "incomplete"
    }
}

fn judged(
    status: &'static str,
    detail: impl Into<String>,
) -> (
    &'static str,
    String,
    Vec<PipelineEvidence>,
    Option<ConfirmationView>,
) {
    (status, detail.into(), Vec::new(), None)
}

async fn evaluate_one(
    db: &DatabaseConnection,
    repo_id: i64,
    task: &rg_db::entities::collab_task::Model,
    condition: &Condition,
    branch_head: Option<&str>,
    now: DateTime<Utc>,
) -> CollabResult<(
    &'static str,
    String,
    Vec<PipelineEvidence>,
    Option<ConfirmationView>,
)> {
    Ok(match condition.r#type.as_str() {
        QUESTION_ANSWERED => {
            let id = condition.params["question_id"].as_str().unwrap_or_default();
            let head = branch_head.map(str::to_string);
            match question::get(db, repo_id, id, move |_| head).await {
                Err(_) => judged(UNKNOWN, "the referenced question can no longer be read"),
                Ok(q) => match (q.status.as_str(), q.context_freshness) {
                    ("answered", Some("fresh")) => judged(
                        SATISFIED,
                        "the question is answered and the context it was asked in still holds",
                    ),
                    ("answered", Some("stale")) => judged(
                        STALE,
                        "the question was answered, but for an outdated task version or commit",
                    ),
                    ("answered", _) => judged(
                        UNKNOWN,
                        "the question was answered, but its context cannot be judged now",
                    ),
                    ("open", _) => judged(UNSATISFIED, "the question is still open"),
                    (other, _) => judged(UNSATISFIED, format!("the question is {other}")),
                },
            }
        }
        WORK_PACKAGE_PRESENT => {
            let id = condition.params["work_package_id"]
                .as_str()
                .unwrap_or_default();
            match collab_work_package_ops::find_in_repo(db, repo_id, id).await? {
                None => judged(UNKNOWN, "the referenced work package can no longer be read"),
                Some(p) => {
                    let newest = collab_work_package_ops::list_page(db, repo_id, &task.id, None, 1)
                        .await?
                        .into_iter()
                        .next();
                    match newest {
                        Some(n) if n.revision > p.revision => judged(
                            STALE,
                            format!(
                                "revision {} was replaced by revision {} (a package's contents are the author's claims)",
                                p.revision, n.revision
                            ),
                        ),
                        _ => judged(
                            SATISFIED,
                            format!(
                                "revision {} exists and is the newest (its contents are the author's claims)",
                                p.revision
                            ),
                        ),
                    }
                }
            }
        }
        CI_PASSED_FOR_COMMIT => {
            let sha = condition.params["commit_sha"].as_str().unwrap_or_default();
            let pipelines = pipeline_ops::list_by_repo_and_commit(db, repo_id, sha, 20).await?;
            let ci = ci_condition(Some(sha), &pipelines, branch_head, now);
            (ci.status, ci.detail, ci.evidence, None)
        }
        MANUAL_CONFIRMATION => match ops::latest_confirmation(db, &task.id, &condition.id).await? {
            None => judged(UNSATISFIED, "nobody has confirmed this yet"),
            Some(c) => {
                let (status, detail) = match (&c.head_sha, branch_head) {
                        (Some(at), Some(now_head)) if at != now_head => (
                            STALE,
                            "confirmed against a commit the branch has since moved past",
                        ),
                        (Some(_), None) => (
                            UNKNOWN,
                            "confirmed against a commit, but the branch head cannot be read now",
                        ),
                        _ => (
                            SATISFIED,
                            "an authorised account confirmed it (a collaboration record, not a merge approval)",
                        ),
                    };
                (
                    status,
                    detail.to_string(),
                    Vec::new(),
                    Some(confirmation_view(c)),
                )
            }
        },
        other => judged(UNKNOWN, format!("unknown condition type `{other}`")),
    })
}

/// Evaluate the task's defined conditions now; `None` when it has none.
pub async fn evaluate_defined(
    db: &DatabaseConnection,
    repo_id: i64,
    task: &rg_db::entities::collab_task::Model,
    branch_head: Option<&str>,
    now: DateTime<Utc>,
) -> CollabResult<Option<DefinedStatus>> {
    let Some(row) = ops::current_set(db, &task.id).await? else {
        return Ok(None);
    };
    let defined: Vec<Condition> =
        serde_json::from_str(&row.conditions_json).map_err(|e| CollabError::Internal(e.into()))?;
    if defined.is_empty() {
        return Ok(None);
    }
    let mut conditions = Vec::with_capacity(defined.len());
    for condition in defined {
        let (status, detail, evidence, confirmation) =
            evaluate_one(db, repo_id, task, &condition, branch_head, now).await?;
        conditions.push(DefinedCondition {
            condition,
            status,
            detail,
            evidence,
            confirmation,
        });
    }
    Ok(Some(DefinedStatus {
        condition_set_version: row.version,
        overall: overall_of(&conditions),
        conditions,
    }))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SnapshotSummary {
    pub id: String,
    pub task_id: String,
    /// 0 = no defined conditions.
    pub condition_set_version: i64,
    pub task_version: i64,
    pub overall: String,
    pub recorded_by_kind: String,
    pub recorded_by_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SnapshotView {
    #[serde(flatten)]
    pub summary: SnapshotSummary,
    /// The evaluation exactly as recorded then; it is never recomputed.
    pub result: Value,
    pub immutable: bool,
    /// True when `client_request_id` had already created this snapshot and
    /// nothing new was stored.
    pub replayed: bool,
}

fn summary(row: &snap_entity::Model) -> SnapshotSummary {
    SnapshotSummary {
        id: row.id.clone(),
        task_id: row.task_id.clone(),
        condition_set_version: row.condition_set_version,
        task_version: row.task_version,
        overall: row.overall.clone(),
        recorded_by_kind: row.actor_kind.clone(),
        recorded_by_id: row.actor_id.clone(),
        created_at: row.created_at,
    }
}

/// Evaluate now and store the result unchanged, as history.
pub async fn record_snapshot(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    actor: &Actor,
    client_request_id: Option<&str>,
    branch_head: Option<String>,
    now: DateTime<Utc>,
) -> CollabResult<SnapshotView> {
    if let Some(key) = client_request_id {
        validation::client_key("client_request_id", key)?;
        // A retry returns the snapshot the key already made, before anything is
        // evaluated or counted against the cap.
        let task = task::find(db, repo_id, task_id).await?;
        if let Some(row) = ops::find_snapshot_by_request(db, &task.id, key).await? {
            return snapshot_view(&row, true);
        }
    }
    let status = completion::evaluate(db, repo_id, task_id, branch_head, now).await?;
    let stored = ops::count_snapshots(db, &status.task_id).await?;
    if stored >= MAX_SNAPSHOTS_PER_TASK {
        return Err(CollabError::CodedConflict {
            code: "SNAPSHOT_LIMIT_REACHED",
            message: format!(
                "this task already has {stored} evaluation snapshots (limit {MAX_SNAPSHOTS_PER_TASK}); read an existing one instead"
            ),
            details: Some(serde_json::json!({ "limit": MAX_SNAPSHOTS_PER_TASK, "stored": stored })),
        });
    }
    let result = serde_json::to_value(&status).map_err(|e| CollabError::Internal(e.into()))?;
    let overall = status
        .defined
        .as_ref()
        .map_or(status.overall, |d| d.overall)
        .to_string();
    let row = ops::insert_snapshot(
        db,
        snap_entity::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            repo_id: Set(repo_id),
            task_id: Set(status.task_id.clone()),
            condition_set_version: Set(status
                .defined
                .as_ref()
                .map_or(0, |d| d.condition_set_version)),
            task_version: Set(status.task_version),
            overall: Set(overall),
            result_json: Set(result.to_string()),
            actor_kind: Set(actor.kind.to_string()),
            actor_id: Set(actor.id.clone()),
            client_request_id: Set(client_request_id.map(str::to_string)),
            created_at: Set(super::creation_timestamp(db, now)),
        },
        &status.task_id,
        client_request_id,
    )
    .await?;
    match row {
        InsertOutcome::Inserted(row) => snapshot_view(&row, false),
        InsertOutcome::Replay(row) => snapshot_view(&row, true),
        InsertOutcome::VersionTaken => Err(CollabError::Internal(anyhow::anyhow!(
            "snapshot insert reported a version conflict"
        ))),
    }
}

/// The stored evaluation as a response; the result is read back, never recomputed.
fn snapshot_view(row: &snap_entity::Model, replayed: bool) -> CollabResult<SnapshotView> {
    let result =
        serde_json::from_str(&row.result_json).map_err(|e| CollabError::Internal(e.into()))?;
    Ok(SnapshotView {
        summary: summary(row),
        result,
        immutable: true,
        replayed,
    })
}

pub async fn list_snapshots(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    limit: u64,
) -> CollabResult<Vec<SnapshotSummary>> {
    let task = task::find(db, repo_id, task_id).await?;
    Ok(
        ops::list_snapshots(db, &task.id, limit.clamp(1, MAX_LISTED))
            .await?
            .iter()
            .map(summary)
            .collect(),
    )
}

pub async fn get_snapshot(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
    id: &str,
) -> CollabResult<SnapshotView> {
    let task = task::find(db, repo_id, task_id).await?;
    let row = if validation::is_uuid(id) {
        ops::get_snapshot(db, repo_id, &task.id, id).await?
    } else {
        None
    }
    .ok_or_else(|| CollabError::not_found("snapshot not found"))?;
    snapshot_view(&row, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_are_closed_per_type() {
        assert!(only_keys(&json!({"commit_sha": "x"}), &["commit_sha"], "t").is_ok());
        assert!(only_keys(&json!({}), &["commit_sha"], "t").is_err());
        assert!(only_keys(
            &json!({"commit_sha": "x", "extra": 1}),
            &["commit_sha"],
            "t"
        )
        .is_err());
        assert!(only_keys(&json!([]), &[], "t").is_err());
        assert!(only_keys(&json!({}), &[], "t").is_ok());
    }

    #[test]
    fn overall_never_reads_unknown_or_stale_as_satisfied() {
        let c = |status| DefinedCondition {
            condition: Condition {
                id: "i".into(),
                r#type: MANUAL_CONFIRMATION.into(),
                label: String::new(),
                params: json!({}),
                introduced_in_version: 1,
                created_by_kind: "user".into(),
                created_by_id: "1".into(),
            },
            status,
            detail: String::new(),
            evidence: Vec::new(),
            confirmation: None,
        };
        assert_eq!(overall_of(&[c(SATISFIED), c(SATISFIED)]), "all_satisfied");
        assert_eq!(overall_of(&[c(SATISFIED), c(STALE)]), "incomplete");
        assert_eq!(overall_of(&[c(SATISFIED), c(UNKNOWN)]), "incomplete");
        assert_eq!(overall_of(&[c(UNKNOWN), c(UNSATISFIED)]), "unsatisfied");
    }
}
