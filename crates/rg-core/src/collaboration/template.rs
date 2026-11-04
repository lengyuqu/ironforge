//! Task templates (O06): repo-admin-managed goal, deliverables and acceptance
//! checklist, with **immutable versions**.
//!
//! * Editing a template appends version n+1; no version row is ever changed.
//! * Creating a task from a template pins `(template_id, version)` on the task
//!   through a link row and stores a *copy* of what the task was created from
//!   (the snapshot). The checklist is also rendered into the task description.
//!   Later template edits therefore never change an existing task.
//! * The checklist is text. It is not tracked per item, not evidence, and says
//!   nothing about whether the work met it (see O04 for conditions).
//! * Template text is written by repository admins and copied verbatim; like
//!   every field here it is `user_content` for the readers.
//! * `rules_commit` is the commit the caller's `rules_ref` resolved to when the
//!   task was created (O01). It records which rules were current, not that the
//!   task's author read or followed them.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use rg_db::entities::{
    agent_session, collab_task, collab_task_template, collab_task_template_link,
    collab_task_template_version,
};
use rg_db::ops::collab_template_ops::{
    self, AddVersionOutcome, CreateFromTemplateOutcome, CreateTemplateOutcome,
};
use sea_orm::{DatabaseConnection, Set};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::cursor::{self, KeysetPosition};
use super::error::{CollabError, CollabResult};
use super::task::{self, TaskView};
use super::{validation, Page, ORIGIN_USER_CONTENT};

pub const MAX_NAME_CHARS: usize = 100;
pub const MAX_GOAL_BYTES: usize = 2048;
pub const MAX_ITEM_BYTES: usize = 256;
pub const MAX_DELIVERABLES: usize = 20;
pub const MAX_CHECKLIST: usize = 30;
pub const MAX_PARAMS: usize = 10;
pub const MAX_PARAM_CHARS: usize = 100;
pub const MAX_NOTES_BYTES: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChecklistItem {
    pub text: String,
    /// Whether the template author marks the item as required. Informational.
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_required() -> bool {
    true
}

/// The editable content of one template version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Content {
    /// Task title with `{name}` placeholders (`[A-Za-z0-9_]{1,32}`), at most 200
    /// characters once rendered.
    pub title_pattern: String,
    pub goal: String,
    pub deliverables: Vec<String>,
    pub checklist: Vec<ChecklistItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VersionView {
    pub template_id: String,
    pub version: i64,
    #[serde(flatten)]
    pub content: Content,
    pub created_by_user_id: i64,
    pub created_at: DateTime<Utc>,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TemplateView {
    pub id: String,
    pub name: String,
    pub latest_version: i64,
    pub created_by_user_id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// The requested version (the latest by default).
    pub version: VersionView,
    pub origin: &'static str,
}

fn version_view(row: collab_task_template_version::Model) -> VersionView {
    VersionView {
        content: Content {
            title_pattern: row.title_pattern,
            goal: row.goal,
            deliverables: serde_json::from_str(&row.deliverables_json).unwrap_or_default(),
            checklist: serde_json::from_str(&row.checklist_json).unwrap_or_default(),
        },
        template_id: row.template_id,
        version: row.version,
        created_by_user_id: row.created_by_user_id,
        created_at: row.created_at,
        origin: ORIGIN_USER_CONTENT,
    }
}

fn template_view(row: collab_task_template::Model, version: VersionView) -> TemplateView {
    TemplateView {
        id: row.id,
        name: row.name,
        latest_version: row.latest_version,
        created_by_user_id: row.created_by_user_id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        version,
        origin: ORIGIN_USER_CONTENT,
    }
}

// ── validation and rendering ──────────────────────────────────────────

/// Placeholder names of a title pattern, in order of appearance. Any brace that
/// is not part of a well-formed `{name}` is rejected, so a pattern means one
/// thing to every reader.
pub fn placeholders(pattern: &str) -> CollabResult<Vec<String>> {
    let bad = || {
        CollabError::bad_request(
            "title_pattern may only use braces for {name} placeholders (name: 1-32 of A-Za-z0-9_)",
        )
    };
    let mut names = Vec::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) if ch.is_ascii_alphanumeric() || ch == '_' => name.push(ch),
                        _ => return Err(bad()),
                    }
                }
                if name.is_empty() || name.len() > 32 {
                    return Err(bad());
                }
                names.push(name);
            }
            '}' => return Err(bad()),
            _ => {}
        }
    }
    Ok(names)
}

fn validate_content(content: &Content) -> CollabResult<()> {
    validation::label(
        "title_pattern",
        &content.title_pattern,
        super::task::MAX_TITLE_CHARS,
    )?;
    placeholders(&content.title_pattern)?;
    validation::text("goal", &content.goal, 1, MAX_GOAL_BYTES)?;
    if content.goal.trim().is_empty() {
        return Err(CollabError::bad_request("goal must not be empty"));
    }
    if content.deliverables.len() > MAX_DELIVERABLES {
        return Err(CollabError::bad_request(format!(
            "deliverables may have at most {MAX_DELIVERABLES} entries"
        )));
    }
    for item in &content.deliverables {
        item_text("deliverables", item)?;
    }
    if content.checklist.len() > MAX_CHECKLIST {
        return Err(CollabError::bad_request(format!(
            "checklist may have at most {MAX_CHECKLIST} entries"
        )));
    }
    for item in &content.checklist {
        item_text("checklist", &item.text)?;
    }
    Ok(())
}

fn item_text(field: &str, text: &str) -> CollabResult<()> {
    if text.trim().is_empty() {
        return Err(CollabError::bad_request(format!(
            "{field} entries must not be empty"
        )));
    }
    validation::text(field, text, 1, MAX_ITEM_BYTES)?;
    if text.contains(['\n', '\r']) {
        return Err(CollabError::bad_request(format!(
            "{field} entries must be a single line"
        )));
    }
    Ok(())
}

fn normalize_content(mut content: Content) -> Content {
    content.title_pattern = content.title_pattern.trim().to_string();
    content.goal = content.goal.trim().to_string();
    for item in &mut content.deliverables {
        *item = item.trim().to_string();
    }
    for item in &mut content.checklist {
        item.text = item.text.trim().to_string();
    }
    content
}

fn hash_of(value: &serde_json::Value) -> String {
    hex::encode(Sha256::digest(value.to_string().as_bytes()))
}

fn content_json(content: &Content) -> CollabResult<(String, String)> {
    let deliverables = serde_json::to_string(&content.deliverables)
        .map_err(|e| CollabError::Internal(e.into()))?;
    let checklist =
        serde_json::to_string(&content.checklist).map_err(|e| CollabError::Internal(e.into()))?;
    Ok((deliverables, checklist))
}

// ── templates ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct NewTemplate {
    pub client_template_id: String,
    pub name: String,
    pub content: Content,
}

pub struct CreatedTemplate {
    pub template: TemplateView,
    pub replayed: bool,
}

/// Create a template with version 1. The caller has established repository
/// admin rights.
pub async fn create(
    db: &DatabaseConnection,
    repo_id: i64,
    user_id: i64,
    input: NewTemplate,
    now: DateTime<Utc>,
) -> CollabResult<CreatedTemplate> {
    validation::client_key("client_template_id", &input.client_template_id)?;
    let name = validation::label("name", &input.name, MAX_NAME_CHARS)?;
    let content = normalize_content(input.content);
    validate_content(&content)?;
    let (deliverables_json, checklist_json) = content_json(&content)?;
    let hash = hash_of(&serde_json::json!([repo_id, name, content]));

    let template_id = uuid::Uuid::new_v4().to_string();
    let outcome = collab_template_ops::create_template(
        db,
        collab_task_template::ActiveModel {
            id: Set(template_id.clone()),
            repo_id: Set(repo_id),
            name: Set(name),
            latest_version: Set(1),
            created_by_user_id: Set(user_id),
            client_template_id: Set(input.client_template_id.clone()),
            payload_hash: Set(hash.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        },
        collab_task_template_version::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            repo_id: Set(repo_id),
            template_id: Set(template_id),
            version: Set(1),
            title_pattern: Set(content.title_pattern.clone()),
            goal: Set(content.goal.clone()),
            deliverables_json: Set(deliverables_json),
            checklist_json: Set(checklist_json),
            created_by_user_id: Set(user_id),
            // Version 1 is keyed by the template's own client id.
            client_version_id: Set(format!("#tpl:{}", input.client_template_id)),
            payload_hash: Set(hash.clone()),
            created_at: Set(now),
        },
    )
    .await?;
    match outcome {
        CreateTemplateOutcome::Created { template, version } => Ok(CreatedTemplate {
            template: template_view(template, version_view(version)),
            replayed: false,
        }),
        CreateTemplateOutcome::Existing { template, version } if template.payload_hash == hash => {
            Ok(CreatedTemplate {
                template: template_view(template, version_view(version)),
                replayed: true,
            })
        }
        CreateTemplateOutcome::Existing { .. } => Err(CollabError::IdempotencyKeyReused),
        CreateTemplateOutcome::NameTaken => Err(CollabError::coded_conflict(
            "TEMPLATE_NAME_TAKEN",
            "a template with this name already exists in the repository",
        )),
    }
}

#[derive(Debug, Clone)]
pub struct NewVersion {
    pub client_version_id: String,
    /// The template's `latest_version` the author saw.
    pub expected_version: i64,
    pub content: Content,
}

/// Append version `expected_version + 1`. A stale expectation is
/// [`CollabError::VersionConflict`] carrying the current latest version.
pub async fn new_version(
    db: &DatabaseConnection,
    repo_id: i64,
    user_id: i64,
    template_id: &str,
    input: NewVersion,
    now: DateTime<Utc>,
) -> CollabResult<CreatedTemplate> {
    validation::client_key("client_version_id", &input.client_version_id)?;
    if input.expected_version < 1 {
        return Err(CollabError::bad_request(
            "expected_version must be at least 1",
        ));
    }
    let content = normalize_content(input.content);
    validate_content(&content)?;
    let (deliverables_json, checklist_json) = content_json(&content)?;
    if !validation::is_uuid(template_id) {
        return Err(CollabError::not_found("template not found"));
    }
    let hash = hash_of(&serde_json::json!([
        template_id,
        input.expected_version,
        content
    ]));

    let outcome = collab_template_ops::add_version(
        db,
        repo_id,
        template_id,
        input.expected_version,
        |version| collab_task_template_version::ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            repo_id: Set(repo_id),
            template_id: Set(template_id.to_string()),
            version: Set(version),
            title_pattern: Set(content.title_pattern.clone()),
            goal: Set(content.goal.clone()),
            deliverables_json: Set(deliverables_json.clone()),
            checklist_json: Set(checklist_json.clone()),
            created_by_user_id: Set(user_id),
            client_version_id: Set(input.client_version_id.clone()),
            payload_hash: Set(hash.clone()),
            created_at: Set(now),
        },
        now,
    )
    .await?;
    match outcome {
        AddVersionOutcome::Created { template, version } => Ok(CreatedTemplate {
            template: template_view(template, version_view(version)),
            replayed: false,
        }),
        AddVersionOutcome::Existing(version) if version.payload_hash == hash => {
            let template = collab_template_ops::find_template(db, repo_id, &version.template_id)
                .await?
                .ok_or_else(|| CollabError::not_found("template not found"))?;
            Ok(CreatedTemplate {
                template: template_view(template, version_view(version)),
                replayed: true,
            })
        }
        AddVersionOutcome::Existing(_) => Err(CollabError::IdempotencyKeyReused),
        AddVersionOutcome::Conflict { current } => Err(CollabError::VersionConflict {
            current_version: current.latest_version,
        }),
        AddVersionOutcome::NotFound => Err(CollabError::not_found("template not found")),
    }
}

/// One template with `version` (default: the latest).
pub async fn get(
    db: &DatabaseConnection,
    repo_id: i64,
    template_id: &str,
    version: Option<i64>,
) -> CollabResult<TemplateView> {
    if !validation::is_uuid(template_id) {
        return Err(CollabError::not_found("template not found"));
    }
    let template = collab_template_ops::find_template(db, repo_id, template_id)
        .await?
        .ok_or_else(|| CollabError::not_found("template not found"))?;
    let wanted = version.unwrap_or(template.latest_version);
    let row = collab_template_ops::find_version(db, &template.id, wanted)
        .await?
        .ok_or_else(|| CollabError::not_found("template version not found"))?;
    Ok(template_view(template, version_view(row)))
}

fn list_scope(repo_id: i64) -> String {
    format!("task-templates:v1:repo={repo_id}:order=created_desc,id_desc")
}

/// Templates newest first, each with its latest version.
pub async fn list(
    db: &DatabaseConnection,
    cursor_key: &[u8],
    repo_id: i64,
    limit: u64,
    cursor_token: Option<&str>,
) -> CollabResult<Page<TemplateView>> {
    let scope = list_scope(repo_id);
    let after = cursor_token
        .map(|token| cursor::decode(cursor_key, &scope, token))
        .transpose()?
        .map(|p| (p.at, p.id));
    let mut rows = collab_template_ops::list_templates(db, repo_id, after, limit + 1).await?;
    let has_more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let next_cursor = has_more.then(|| rows.last()).flatten().map(|last| {
        cursor::encode(
            cursor_key,
            &scope,
            &KeysetPosition {
                at: last.created_at,
                id: last.id.clone(),
            },
        )
    });
    let ids: Vec<String> = rows.iter().map(|t| t.id.clone()).collect();
    let versions = collab_template_ops::versions_of(db, &ids).await?;
    let items = rows
        .into_iter()
        .filter_map(|template| {
            let latest = versions
                .iter()
                .find(|v| v.template_id == template.id && v.version == template.latest_version)?
                .clone();
            Some(template_view(template, version_view(latest)))
        })
        .collect();
    Ok(Page { items, next_cursor })
}

// ── tasks from templates ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FromTemplate {
    /// Idempotency key, unique per creating session.
    pub client_task_id: String,
    pub template_id: String,
    /// Pin this version; default the latest at the time of the request.
    pub version: Option<i64>,
    /// Values for the title pattern's placeholders; exactly those names.
    pub params: BTreeMap<String, String>,
    /// Free text appended to the rendered description.
    pub notes: Option<String>,
    pub owner_session_id: Option<String>,
    pub branch: Option<String>,
    pub paths: Vec<String>,
    /// The ref the caller asked to record rules for (hashed for idempotency; the
    /// commit it resolved to is passed separately).
    pub rules_ref: Option<String>,
}

/// What a task was created from. Immutable; later template edits do not reach it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    pub template_id: String,
    pub template_name: String,
    pub template_version: i64,
    pub title_pattern: String,
    pub goal: String,
    pub deliverables: Vec<String>,
    pub checklist: Vec<ChecklistItem>,
    pub params: BTreeMap<String, String>,
    pub rendered_title: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LinkView {
    pub task_id: String,
    pub template_id: String,
    pub template_version: i64,
    pub snapshot: Snapshot,
    /// Commit the caller's `rules_ref` resolved to at creation; `null` when no
    /// ref was supplied.
    pub rules_commit: Option<String>,
    pub creator_kind: String,
    pub creator_id: String,
    pub created_at: DateTime<Utc>,
    /// The template's newest version right now. Informational: the task stays
    /// pinned to `template_version` whatever this says.
    pub template_latest_version: Option<i64>,
    pub newer_version_available: bool,
    pub origin: &'static str,
}

pub struct CreatedFromTemplate {
    pub task: TaskView,
    pub link: LinkView,
    pub replayed: bool,
}

async fn link_view(
    db: &DatabaseConnection,
    row: collab_task_template_link::Model,
) -> CollabResult<LinkView> {
    let snapshot: Snapshot = serde_json::from_str(&row.snapshot_json)
        .map_err(|e| CollabError::Internal(anyhow::anyhow!("corrupt template snapshot: {e}")))?;
    let latest = collab_template_ops::find_template(db, row.repo_id, &row.template_id)
        .await?
        .map(|t| t.latest_version);
    Ok(LinkView {
        newer_version_available: latest.is_some_and(|l| l > row.template_version),
        template_latest_version: latest,
        task_id: row.task_id,
        template_id: row.template_id,
        template_version: row.template_version,
        snapshot,
        rules_commit: row.rules_commit,
        creator_kind: row.creator_kind,
        creator_id: row.creator_id,
        created_at: row.created_at,
        origin: ORIGIN_USER_CONTENT,
    })
}

fn check_input(input: &FromTemplate) -> CollabResult<()> {
    validation::client_key("client_task_id", &input.client_task_id)?;
    if !validation::is_uuid(&input.template_id) {
        return Err(CollabError::not_found("template not found"));
    }
    if input.params.len() > MAX_PARAMS {
        return Err(CollabError::bad_request(format!(
            "at most {MAX_PARAMS} params are allowed"
        )));
    }
    for (key, value) in &input.params {
        validation::label("param name", key, 32)?;
        validation::label(&format!("param {key}"), value, MAX_PARAM_CHARS)?;
    }
    if let Some(notes) = &input.notes {
        validation::text("notes", notes, 0, MAX_NOTES_BYTES)?;
    }
    if let Some(version) = input.version {
        if version < 1 {
            return Err(CollabError::bad_request("version must be at least 1"));
        }
    }
    if let Some(branch) = &input.branch {
        task::validate_branch(branch)?;
    }
    task::validate_paths(&input.paths)?;
    Ok(())
}

fn request_hash(repo_id: i64, input: &FromTemplate) -> String {
    hash_of(&serde_json::json!([
        repo_id,
        input.template_id,
        input.version,
        input.params,
        input.notes,
        input.owner_session_id,
        input.branch,
        input.paths,
        input.rules_ref
    ]))
}

async fn replayed(
    db: &DatabaseConnection,
    repo_id: i64,
    actor: &agent_session::Model,
    input: &FromTemplate,
) -> CollabResult<Option<CreatedFromTemplate>> {
    let Some(link) = collab_template_ops::find_link_by_client_id(
        db,
        "session",
        &actor.id,
        &input.client_task_id,
    )
    .await?
    else {
        return Ok(None);
    };
    if link.payload_hash != request_hash(repo_id, input) {
        return Err(CollabError::IdempotencyKeyReused);
    }
    let task = collab_template_ops::find_task(db, repo_id, &link.task_id)
        .await?
        .ok_or_else(|| CollabError::not_found("task not found"))?;
    Ok(Some(CreatedFromTemplate {
        task: task::view(task),
        link: link_view(db, link).await?,
        replayed: true,
    }))
}

/// Answer a retry of an earlier create without touching anything else: `None`
/// when the key was never used. Runs before any precondition (template, owner,
/// rules ref) is checked, so a retry still gets its answer after they changed.
pub async fn replay_from_template(
    db: &DatabaseConnection,
    repo_id: i64,
    actor: &agent_session::Model,
    input: &FromTemplate,
) -> CollabResult<Option<CreatedFromTemplate>> {
    check_input(input)?;
    replayed(db, repo_id, actor, input).await
}

/// Description of a task made from a template: goal, deliverables and checklist
/// as plain text, then the caller's notes. The template text is copied, not
/// referenced.
pub fn render_description(snapshot: &Snapshot, notes: Option<&str>) -> String {
    let mut out = format!(
        "Created from template \"{}\" v{}. The text below is a copy; later template edits do not change this task.\n\nGoal:\n{}\n",
        snapshot.template_name, snapshot.template_version, snapshot.goal
    );
    if !snapshot.deliverables.is_empty() {
        out.push_str("\nDeliverables:\n");
        for item in &snapshot.deliverables {
            out.push_str(&format!("- {item}\n"));
        }
    }
    if !snapshot.checklist.is_empty() {
        out.push_str("\nAcceptance checklist (text only; items are not tracked or verified):\n");
        for item in &snapshot.checklist {
            let mark = if item.required {
                "required"
            } else {
                "optional"
            };
            out.push_str(&format!("- [{mark}] {}\n", item.text));
        }
    }
    if let Some(notes) = notes.filter(|n| !n.trim().is_empty()) {
        out.push_str(&format!("\nNotes:\n{notes}\n"));
    }
    out
}

fn render_title(pattern: &str, params: &BTreeMap<String, String>) -> CollabResult<String> {
    let wanted: BTreeSet<String> = placeholders(pattern)?.into_iter().collect();
    let given: BTreeSet<String> = params.keys().cloned().collect();
    let missing: Vec<&String> = wanted.difference(&given).collect();
    let unknown: Vec<&String> = given.difference(&wanted).collect();
    if !missing.is_empty() || !unknown.is_empty() {
        return Err(CollabError::bad_request(format!(
            "params must match the title pattern's placeholders exactly (missing: [{}], unknown: [{}])",
            missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
            unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        )));
    }
    let mut out = pattern.to_string();
    for (key, value) in params {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    validation::label("rendered title", &out, task::MAX_TITLE_CHARS)
}

/// Create a task from a template version and pin it. `rules_commit` is the
/// already-resolved commit of the caller's `rules_ref` (the REST layer does the
/// Git lookup). Retries of an earlier request are answered first.
pub async fn create_from_template(
    db: &DatabaseConnection,
    repo_id: i64,
    actor: &agent_session::Model,
    input: FromTemplate,
    rules_commit: Option<String>,
    now: DateTime<Utc>,
) -> CollabResult<CreatedFromTemplate> {
    check_input(&input)?;
    if let Some(done) = replayed(db, repo_id, actor, &input).await? {
        return Ok(done);
    }

    let template = collab_template_ops::find_template(db, repo_id, &input.template_id)
        .await?
        .ok_or_else(|| CollabError::not_found("template not found"))?;
    let wanted = input.version.unwrap_or(template.latest_version);
    let version = collab_template_ops::find_version(db, &template.id, wanted)
        .await?
        .ok_or_else(|| CollabError::not_found("template version not found"))?;
    let version = version_view(version);

    let title = render_title(&version.content.title_pattern, &input.params)?;
    let snapshot = Snapshot {
        template_id: template.id.clone(),
        template_name: template.name.clone(),
        template_version: version.version,
        title_pattern: version.content.title_pattern,
        goal: version.content.goal,
        deliverables: version.content.deliverables,
        checklist: version.content.checklist,
        params: input.params.clone(),
        rendered_title: title.clone(),
    };
    let description = render_description(&snapshot, input.notes.as_deref());
    validation::text("description", &description, 0, task::MAX_DESCRIPTION_BYTES)?;
    if let Some(owner) = &input.owner_session_id {
        task::require_live_owner(db, repo_id, owner, now).await?;
    }
    let paths_json = task::validate_paths(&input.paths)?;
    let snapshot_json =
        serde_json::to_string(&snapshot).map_err(|e| CollabError::Internal(e.into()))?;
    let task_id = uuid::Uuid::new_v4().to_string();

    let outcome = collab_template_ops::create_task_with_link(
        db,
        collab_task::ActiveModel {
            id: Set(task_id.clone()),
            repo_id: Set(repo_id),
            title: Set(title),
            description: Set(Some(description)),
            status: Set("open".to_string()),
            owner_session_id: Set(input.owner_session_id.clone()),
            branch: Set(input.branch.clone()),
            paths_json: Set(paths_json),
            issue_number: Set(None),
            pr_number: Set(None),
            version: Set(1),
            next_seq: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
        },
        collab_task_template_link::ActiveModel {
            task_id: Set(task_id),
            repo_id: Set(repo_id),
            template_id: Set(template.id.clone()),
            template_version: Set(version.version),
            snapshot_json: Set(snapshot_json),
            rules_commit: Set(rules_commit),
            creator_kind: Set("session".to_string()),
            creator_id: Set(actor.id.clone()),
            client_task_id: Set(input.client_task_id.clone()),
            payload_hash: Set(request_hash(repo_id, &input)),
            created_at: Set(now),
        },
    )
    .await?;
    match outcome {
        CreateFromTemplateOutcome::Created { task, link } => Ok(CreatedFromTemplate {
            task: task::view(task),
            link: link_view(db, link).await?,
            replayed: false,
        }),
        CreateFromTemplateOutcome::Existing(link) => {
            if link.payload_hash != request_hash(repo_id, &input) {
                return Err(CollabError::IdempotencyKeyReused);
            }
            let task = collab_template_ops::find_task(db, repo_id, &link.task_id)
                .await?
                .ok_or_else(|| CollabError::not_found("task not found"))?;
            Ok(CreatedFromTemplate {
                task: task::view(task),
                link: link_view(db, link).await?,
                replayed: true,
            })
        }
    }
}

/// The template pin of a task, or `NotFound` for a task made without one.
pub async fn get_link(
    db: &DatabaseConnection,
    repo_id: i64,
    task_id: &str,
) -> CollabResult<LinkView> {
    let task = task::find(db, repo_id, task_id).await?;
    let link = collab_template_ops::find_link(db, repo_id, &task.id)
        .await?
        .ok_or_else(|| CollabError::not_found("task was not created from a template"))?;
    link_view(db, link).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_strict() {
        assert_eq!(
            placeholders("Fix {area} in {crate}").unwrap(),
            vec!["area", "crate"]
        );
        assert!(placeholders("no placeholders").unwrap().is_empty());
        for bad in [
            "{",
            "}",
            "{}",
            "{a b}",
            "{a{b}}",
            "x}",
            "{a",
            &format!("{{{}}}", "a".repeat(33)),
        ] {
            assert!(placeholders(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn title_params_must_match_exactly() {
        let mut p = BTreeMap::new();
        p.insert("area".to_string(), "auth".to_string());
        assert_eq!(render_title("Fix {area}", &p).unwrap(), "Fix auth");
        assert!(render_title("Fix {area} {crate}", &p).is_err(), "missing");
        assert!(render_title("Fix", &p).is_err(), "unknown");
        let mut long = BTreeMap::new();
        long.insert("area".to_string(), "x".repeat(100));
        assert!(render_title(&format!("{}{{area}}", "y".repeat(101)), &long).is_err());
    }

    #[test]
    fn description_copies_the_checklist() {
        let snapshot = Snapshot {
            template_id: "t".into(),
            template_name: "Bugfix".into(),
            template_version: 2,
            title_pattern: "Fix {x}".into(),
            goal: "Fix it".into(),
            deliverables: vec!["a patch".into()],
            checklist: vec![
                ChecklistItem {
                    text: "tests added".into(),
                    required: true,
                },
                ChecklistItem {
                    text: "docs".into(),
                    required: false,
                },
            ],
            params: BTreeMap::new(),
            rendered_title: "Fix y".into(),
        };
        let text = render_description(&snapshot, Some("see issue"));
        assert!(text.contains("Bugfix\" v2"));
        assert!(text.contains("- [required] tests added"));
        assert!(text.contains("- [optional] docs"));
        assert!(text.contains("Notes:\nsee issue"));
    }
}
