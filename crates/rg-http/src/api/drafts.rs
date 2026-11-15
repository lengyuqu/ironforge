//! The agent draft area (#18): registration, renewal and the TTL cleanup tick.
//!
//! A *draft namespace* is a reserved branch directory `refs/heads/agent/<id>/`.
//! Branches under it may be pushed and they run CI, but their pushes deliver no
//! webhook, no WebSocket message and no mail (see `post_push_hooks` in
//! `lib.rs`), and the directory is deleted on its own once the TTL has passed.
//!
//! Four endpoints, all under `/repos/{owner}/{name}/agent-drafts`:
//!
//! | Method | Path | Effect |
//! |---|---|---|
//! | `POST`   | `/` | register a prefix (`ttl_secs`, `grace_secs`) |
//! | `GET`    | `/` | list the repository's namespaces |
//! | `PATCH`  | `/{id}` | extend (never shorten; capped at 4 × the TTL) |
//! | `DELETE` | `/{id}` | close the gate now; the branches go after the grace |
//!
//! # The lifecycle
//!
//! ```text
//!                 expires_at passes          expires_at + grace_secs passes
//!   active ───────────────────────▶ expiring ─────────────────────────▶ deleted
//!     ▲                                 │
//!     └────────── PATCH (extend) ───────┘
//! ```
//!
//! * `active` — pushes accepted.
//! * `expiring` — the gate is closed, pushes are refused, the branches are
//!   still readable and CI that is already running can finish. `PATCH` reopens.
//! * `deleted` — the branches were deleted (by `run_cleanup_loop`); the row is
//!   kept so a push to the old prefix keeps being refused.
//!
//! The gate always closes **before** anything is deleted: that is what makes
//! "expired" observable and recoverable, and it is why the cleanup tick has two
//! phases rather than one.
//!
//! # Why deletion does not use the branch API
//!
//! `on_delete` (the `git push --delete` / `DELETE /branches/{x}` guard) refuses
//! the default branch, protected branches and any branch an open pull request
//! occupies. A draft branch should hit none of those, but if it ever does
//! (somebody opened a PR from a draft, or a protection rule matched
//! `agent/42/*`), the cleanup would fail forever and warn every hour. The tick
//! therefore deletes through `delete_ref_if` — a compare-and-swap at the ref
//! level, which is what the cleanup actually means — and records in the audit
//! entry *that it deliberately bypassed the guard, and why*.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::api::repo_access::{require_read, require_write};
use crate::error::AppError;
use crate::AppState;

use rg_db::entities::agent_draft_namespace::{Model as Namespace, STATE_DELETED, STATE_EXPIRING};
use rg_db::entities::repository::Model as Repository;

// ── Re-exports of the core rules ──────────────────────────────────────────

pub use rg_core::repo::draft::{
    DEFAULT_GRACE_SECS, EXTEND_CAP_MULTIPLIER, MAX_TTL_SECS, MIN_TTL_SECS,
};

// ── Response and request shapes ──────────────────────────────────────────

#[derive(Debug, Serialize, ToSchema)]
pub struct DraftNamespaceResponse {
    pub id: i64,
    pub repo_id: i64,
    /// The reserved directory, with the trailing slash (`agent/42/`).
    pub prefix: String,
    pub owner_user_id: i64,
    /// The TTL asked for when the namespace was created.
    pub ttl_secs: i64,
    /// The moment pushes stop being accepted.
    pub expires_at: String,
    /// The moment the branches are deleted: `expires_at + grace_secs`.
    pub delete_due_at: String,
    pub grace_secs: i64,
    /// `active` | `expiring` | `deleted`.
    pub state: String,
    pub extended_count: i32,
    pub created_at: String,
}

impl From<&Namespace> for DraftNamespaceResponse {
    fn from(namespace: &Namespace) -> Self {
        let delete_due_at = rg_core::repo::draft::delete_due_at(namespace);
        Self {
            id: namespace.id,
            repo_id: namespace.repo_id,
            prefix: namespace.prefix.clone(),
            owner_user_id: namespace.owner_user_id,
            ttl_secs: namespace.ttl_secs,
            expires_at: namespace
                .expires_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            delete_due_at: delete_due_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            grace_secs: namespace.grace_secs,
            state: namespace.state.clone(),
            extended_count: namespace.extended_count,
            created_at: namespace
                .created_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDraftRequest {
    /// The directory to reserve, e.g. `agent/42/`.
    pub prefix: String,
    /// Lifetime in seconds, 60 … 30 days.
    pub ttl_secs: i64,
    /// How long the branches survive after expiry (default one day).
    #[serde(default)]
    pub grace_secs: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ExtendDraftRequest {
    /// Seconds to add to the deadline.
    pub ttl_secs: i64,
}

// ── Handlers ─────────────────────────────────────────────────────────────

/// Register a draft namespace.
#[utoipa::path(
    post,
    path = "/repos/{owner}/{name}/agent-drafts",
    tag = "Agent Drafts",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    request_body = CreateDraftRequest,
    responses(
        (status = 201, description = "Created", body = DraftNamespaceResponse),
        (status = 400, description = "Invalid prefix or TTL", body = serde_json::Value),
        (status = 403, description = "Forbidden", body = serde_json::Value),
        (status = 409, description = "The prefix already exists", body = serde_json::Value),
    ),
)]
pub async fn create(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<CreateDraftRequest>,
) -> impl IntoResponse {
    let (repo, actor_id) = match require_write(&state, &headers, &owner, &name).await {
        Ok(pair) => pair,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = rg_core::repo::draft::validate_prefix(&body.prefix) {
        return AppError::bad_request(error).into_response();
    }
    if !(MIN_TTL_SECS..=MAX_TTL_SECS).contains(&body.ttl_secs) {
        return AppError::bad_request(format!(
            "ttl_secs must be between {MIN_TTL_SECS} and {MAX_TTL_SECS}"
        ))
        .into_response();
    }
    let grace_secs = body.grace_secs.unwrap_or(DEFAULT_GRACE_SECS);
    if !(0..=MAX_TTL_SECS).contains(&grace_secs) {
        return AppError::bad_request(format!("grace_secs must be between 0 and {MAX_TTL_SECS}"))
            .into_response();
    }

    // The agent segment is the caller's own agent (or the caller). Without
    // this, one agent could reserve `agent/<other>/` and the owner of that
    // directory would find its pushes refused.
    match agent_segment_is_owned_by(&state, &body.prefix, actor_id).await {
        Ok(true) => {}
        Ok(false) => {
            return AppError::forbidden("the agent segment of this prefix is not an agent you own")
                .into_response();
        }
        Err(error) => return error.into_response(),
    }

    // A token limited to some branches may only reserve a directory it could
    // push to: the reservation and the ability to push must be the same answer.
    if let Some(claims) = crate::api::auth::extract_bearer_claims(&headers, &state.jwt_secret) {
        if let Err(error) =
            crate::api::auth::ensure_token_may_write_branch(&state.db, &claims, &body.prefix).await
        {
            return error.into_response();
        }
    }

    match rg_db::ops::agent_draft_ops::find_by_repo_and_prefix(&state.db, repo.id, &body.prefix)
        .await
    {
        Ok(Some(existing)) => {
            let message = match existing.state.as_str() {
                STATE_DELETED => format!(
                    "draft namespace '{}' was already cleaned up; choose a new prefix",
                    existing.prefix
                ),
                STATE_EXPIRING => format!(
                    "draft namespace '{}' expired at {}; extend it (PATCH .../{}) or choose a new prefix",
                    existing.prefix,
                    existing
                        .expires_at
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    existing.id
                ),
                _ => format!(
                    "draft namespace '{}' already exists (id {}, expires at {}); extend it instead",
                    existing.prefix,
                    existing.id,
                    existing
                        .expires_at
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                ),
            };
            return AppError::conflict(message).into_response();
        }
        Ok(None) => {}
        Err(error) => return AppError::internal(error).into_response(),
    }

    let expires_at = Utc::now() + Duration::seconds(body.ttl_secs);
    match rg_db::ops::agent_draft_ops::create(
        &state.db,
        repo.id,
        &body.prefix,
        actor_id,
        body.ttl_secs,
        expires_at,
        grace_secs,
    )
    .await
    {
        Ok(created) => (
            StatusCode::CREATED,
            Json(DraftNamespaceResponse::from(&created)),
        )
            .into_response(),
        // Two simultaneous creates: the loser re-reads and reports the conflict
        // instead of turning a race into a 500.
        Err(error) => match rg_db::ops::agent_draft_ops::find_by_repo_and_prefix(
            &state.db,
            repo.id,
            &body.prefix,
        )
        .await
        {
            Ok(Some(existing)) => AppError::conflict(format!(
                "draft namespace '{}' already exists (id {})",
                existing.prefix, existing.id
            ))
            .into_response(),
            _ => AppError::internal(error).into_response(),
        },
    }
}

/// List the repository's draft namespaces.
#[utoipa::path(
    get,
    path = "/repos/{owner}/{name}/agent-drafts",
    tag = "Agent Drafts",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
    ),
    responses(
        (status = 200, description = "The namespaces", body = Vec<DraftNamespaceResponse>),
    ),
)]
pub async fn list(
    State(state): State<AppState>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let repo = match require_read(&state, &headers, &owner, &name).await {
        Ok(repo) => repo,
        Err(error) => return error.into_response(),
    };
    match rg_db::ops::agent_draft_ops::list_by_repo(&state.db, repo.id).await {
        Ok(namespaces) => Json(
            namespaces
                .iter()
                .map(DraftNamespaceResponse::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(error) => AppError::internal(error).into_response(),
    }
}

/// Extend a namespace. Never shortens, and the deadline may not exceed
/// `created_at + 4 × ttl_secs`.
#[utoipa::path(
    patch,
    path = "/repos/{owner}/{name}/agent-drafts/{id}",
    tag = "Agent Drafts",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("id" = i64, Path, description = "draft namespace id"),
    ),
    request_body = ExtendDraftRequest,
    responses(
        (status = 200, description = "Extended", body = DraftNamespaceResponse),
        (status = 400, description = "Invalid TTL or already at the cap", body = serde_json::Value),
        (status = 404, description = "Not found", body = serde_json::Value),
        (status = 409, description = "Already cleaned up", body = serde_json::Value),
    ),
)]
pub async fn extend(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
    Json(body): Json<ExtendDraftRequest>,
) -> impl IntoResponse {
    let (repo, _) = match require_write(&state, &headers, &owner, &name).await {
        Ok(pair) => pair,
        Err(error) => return error.into_response(),
    };
    let namespace = match load_namespace(&state, repo.id, id).await {
        Ok(namespace) => namespace,
        Err(error) => return error.into_response(),
    };
    if let Some(claims) = crate::api::auth::extract_bearer_claims(&headers, &state.jwt_secret) {
        if let Err(error) =
            crate::api::auth::ensure_token_may_write_branch(&state.db, &claims, &namespace.prefix)
                .await
        {
            return error.into_response();
        }
    }
    if namespace.state == STATE_DELETED {
        return AppError::conflict(format!(
            "draft namespace '{}' was already cleaned up; register a new prefix",
            namespace.prefix
        ))
        .into_response();
    }
    if !(MIN_TTL_SECS..=MAX_TTL_SECS).contains(&body.ttl_secs) {
        return AppError::bad_request(format!(
            "ttl_secs must be between {MIN_TTL_SECS} and {MAX_TTL_SECS}"
        ))
        .into_response();
    }

    let now = Utc::now();
    let deadline = match rg_core::repo::draft::extend_deadline(&namespace, now, body.ttl_secs) {
        Ok(deadline) => deadline,
        Err(error) => return AppError::bad_request(error).into_response(),
    };
    let extended_count = namespace.extended_count.saturating_add(1);
    let updated = if namespace.state == STATE_EXPIRING {
        // The grace period is the rescue window: extending a closed namespace
        // reopens it, which is the only way back from `expiring`.
        rg_db::ops::agent_draft_ops::reopen_gate(&state.db, namespace.id, deadline, extended_count)
            .await
    } else {
        rg_db::ops::agent_draft_ops::extend(&state.db, namespace.id, deadline, extended_count).await
    };
    match updated {
        Ok(true) => match rg_db::ops::agent_draft_ops::find_by_id(&state.db, namespace.id).await {
            Ok(Some(updated)) => Json(DraftNamespaceResponse::from(&updated)).into_response(),
            Ok(None) => AppError::not_found("draft namespace disappeared").into_response(),
            Err(error) => AppError::internal(error).into_response(),
        },
        Ok(false) => {
            AppError::conflict("draft namespace changed while extending; re-read it and retry")
                .into_response()
        }
        Err(error) => AppError::internal(error).into_response(),
    }
}

/// Soft-delete: close the gate now, delete the branches after the grace period.
#[utoipa::path(
    delete,
    path = "/repos/{owner}/{name}/agent-drafts/{id}",
    tag = "Agent Drafts",
    params(
        ("owner" = String, Path, description = "owner"),
        ("name" = String, Path, description = "name"),
        ("id" = i64, Path, description = "draft namespace id"),
    ),
    responses(
        (status = 202, description = "Closed; deletion happens after the grace period", body = DraftNamespaceResponse),
        (status = 404, description = "Not found", body = serde_json::Value),
        (status = 409, description = "Already cleaned up", body = serde_json::Value),
    ),
)]
pub async fn delete(
    State(state): State<AppState>,
    Path((owner, name, id)): Path<(String, String, i64)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (repo, _) = match require_write(&state, &headers, &owner, &name).await {
        Ok(pair) => pair,
        Err(error) => return error.into_response(),
    };
    let namespace = match load_namespace(&state, repo.id, id).await {
        Ok(namespace) => namespace,
        Err(error) => return error.into_response(),
    };
    if let Some(claims) = crate::api::auth::extract_bearer_claims(&headers, &state.jwt_secret) {
        if let Err(error) =
            crate::api::auth::ensure_token_may_write_branch(&state.db, &claims, &namespace.prefix)
                .await
        {
            return error.into_response();
        }
    }
    if namespace.state == STATE_DELETED {
        return AppError::conflict(format!(
            "draft namespace '{}' was already cleaned up",
            namespace.prefix
        ))
        .into_response();
    }
    if namespace.state == STATE_EXPIRING {
        // Already closed: the request is satisfied, report the current state.
        return (
            StatusCode::ACCEPTED,
            Json(DraftNamespaceResponse::from(&namespace)),
        )
            .into_response();
    }

    match rg_db::ops::agent_draft_ops::soft_delete(&state.db, namespace.id, Utc::now()).await {
        Ok(true) => match rg_db::ops::agent_draft_ops::find_by_id(&state.db, namespace.id).await {
            Ok(Some(closed)) => (
                StatusCode::ACCEPTED,
                Json(DraftNamespaceResponse::from(&closed)),
            )
                .into_response(),
            Ok(None) => AppError::not_found("draft namespace disappeared").into_response(),
            Err(error) => AppError::internal(error).into_response(),
        },
        Ok(false) => {
            AppError::conflict("draft namespace changed while closing; retry").into_response()
        }
        Err(error) => AppError::internal(error).into_response(),
    }
}

// ── Cleanup ──────────────────────────────────────────────────────────────

/// What one cleanup tick did.
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct CleanupSummary {
    /// Namespaces whose gate was closed this tick (`active` → `expiring`).
    pub gated: u64,
    /// Namespaces whose branches were deleted this tick.
    pub deleted: u64,
    /// Branches removed.
    pub deleted_refs: u64,
    /// Branches that were already gone when the CAS ran.
    pub vanished_refs: u64,
    /// Branches (or namespaces) that could not be cleaned; retried next tick.
    pub failures: u64,
}

/// The hourly tick: close gates whose deadline passed, then delete the branches
/// of namespaces whose grace period is over.
///
/// Split into two phases on purpose. Phase 1 is what refuses new pushes, so it
/// must happen for every expired namespace even if phase 2 fails. Phase 2 never
/// touches a namespace phase 1 has not closed.
pub async fn run_cleanup_loop(state: AppState) {
    loop {
        match run_cleanup_once(&state).await {
            Ok(summary) if summary.gated > 0 || summary.deleted > 0 || summary.failures > 0 => {
                tracing::info!(
                    gated = summary.gated,
                    deleted = summary.deleted,
                    deleted_refs = summary.deleted_refs,
                    vanished_refs = summary.vanished_refs,
                    failures = summary.failures,
                    "agent draft cleanup completed"
                )
            }
            Ok(_) => {}
            Err(error) => tracing::error!(%error, "agent draft cleanup failed"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}

/// One pass of [`run_cleanup_loop`], callable from tests and from an operator.
pub async fn run_cleanup_once(state: &AppState) -> anyhow::Result<CleanupSummary> {
    let now = Utc::now();
    let mut summary = CleanupSummary::default();

    // Phase 1: close the gate. Pushes are refused from here on.
    for namespace in rg_db::ops::agent_draft_ops::list_expired_active(&state.db, now).await? {
        match rg_db::ops::agent_draft_ops::close_gate(&state.db, namespace.id).await {
            Ok(true) => {
                summary.gated += 1;
                tracing::info!(
                    namespace_id = namespace.id,
                    prefix = %namespace.prefix,
                    "agent draft namespace expired: pushes are now refused"
                );
            }
            Ok(false) => {}
            Err(error) => {
                summary.failures += 1;
                tracing::warn!(namespace_id = namespace.id, %error, "could not close a draft gate");
            }
        }
    }

    // Phase 2: delete the branches of namespaces whose grace period is over.
    for namespace in rg_db::ops::agent_draft_ops::list_expiring(&state.db).await? {
        if !rg_core::repo::draft::deletion_is_due(&namespace, now) {
            continue;
        }
        match clean_namespace(state, &namespace).await {
            Ok(outcome) if outcome.failures == 0 => {
                summary.deleted += 1;
                summary.deleted_refs += outcome.deleted_refs;
                summary.vanished_refs += outcome.vanished_refs;
                if let Err(error) =
                    rg_db::ops::agent_draft_ops::mark_deleted(&state.db, namespace.id).await
                {
                    summary.failures += 1;
                    tracing::warn!(namespace_id = namespace.id, %error, "could not mark a draft deleted");
                }
            }
            Ok(outcome) => {
                summary.failures += outcome.failures;
                tracing::warn!(
                    namespace_id = namespace.id,
                    prefix = %namespace.prefix,
                    failures = outcome.failures,
                    "draft cleanup left branches behind; retrying next tick"
                );
            }
            Err(error) => {
                summary.failures += 1;
                tracing::warn!(
                    namespace_id = namespace.id,
                    prefix = %namespace.prefix,
                    %error,
                    "draft cleanup failed; retrying next tick"
                );
            }
        }
    }

    Ok(summary)
}

#[derive(Debug, Default)]
struct CleanOutcome {
    deleted_refs: u64,
    vanished_refs: u64,
    failures: u64,
    refs: Vec<String>,
}

/// Delete every branch under one namespace, bypassing the branch-delete guard,
/// and record what was done — including the bypass.
async fn clean_namespace(state: &AppState, namespace: &Namespace) -> anyhow::Result<CleanOutcome> {
    let repo = rg_db::ops::repo_ops::find_by_id(&state.db, namespace.repo_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("repository {} no longer exists", namespace.repo_id))?;
    let repo_path = repo_disk_path(state, &repo).await?;
    if repo.deleted_at.is_some() {
        // The repository was deleted out of band; its branches are not this
        // tick's problem. Mark the namespace cleaned instead of retrying (and
        // warning) every hour forever.
        tracing::info!(
            namespace_id = namespace.id,
            repo_id = namespace.repo_id,
            "draft namespace belongs to a deleted repository; marking it cleaned"
        );
        record_cleanup_audit(state, namespace, &CleanOutcome::default()).await;
        return Ok(CleanOutcome::default());
    }

    let mut outcome = CleanOutcome::default();
    let prefix = format!("refs/heads/{}", namespace.prefix);
    // Collect first, then delete: the ref iterator borrows the repository's
    // mapped files, and `delete_ref_if` opens the repository again.
    let targets = {
        let repo_handle = gix::open(&repo_path)
            .map_err(|error| anyhow::anyhow!("failed to open {}: {error}", repo_path.display()))?;
        let platform = repo_handle
            .references()
            .map_err(|error| anyhow::anyhow!("failed to list refs: {error}"))?;
        let refs = platform
            .all()
            .map_err(|error| anyhow::anyhow!("failed to iterate refs: {error}"))?;
        let mut targets: Vec<(String, String)> = Vec::new();
        for reference in refs.flatten() {
            let refname = reference.name().as_bstr().to_string();
            if !refname.starts_with(&prefix) || refname == prefix {
                continue;
            }
            let target = reference.target();
            let Some(id) = target.try_id() else {
                // A symbolic ref under a draft prefix is not something the push
                // path can create, so leave it alone and let a human look.
                tracing::warn!(refname = %refname, "skipping a non-object draft ref");
                continue;
            };
            targets.push((refname, gix::hash::ObjectId::from(id).to_string()));
        }
        targets
    };

    for (refname, sha) in targets {
        // Compare-and-swap: the branch must still point where it did when it was
        // listed. A push that landed in between fails the CAS instead of losing
        // work; the next tick picks the namespace up again.
        match rg_git::ops::delete_ref_if(&repo_path, &refname, &sha) {
            Ok(()) => {
                outcome.deleted_refs += 1;
                outcome.refs.push(refname);
            }
            Err(error) => {
                // Gone already is the normal outcome for a branch the agent
                // deleted itself; not a failure.
                if error.to_string().contains("moved or no longer exists") {
                    outcome.vanished_refs += 1;
                } else {
                    outcome.failures += 1;
                    tracing::warn!(namespace_id = namespace.id, refname = %refname, %error, "could not delete a draft branch");
                }
            }
        }
    }

    if outcome.failures == 0 {
        record_cleanup_audit(state, namespace, &outcome).await;
    }
    Ok(outcome)
}

/// The audit entry for a cleanup, including the deliberate guard bypass.
async fn record_cleanup_audit(state: &AppState, namespace: &Namespace, outcome: &CleanOutcome) {
    let repo_name = rg_db::ops::repo_ops::find_by_id(&state.db, namespace.repo_id)
        .await
        .ok()
        .flatten()
        .map(|repo| format!("{}/{}", repo.owner_id, repo.name))
        .unwrap_or_else(|| format!("repo#{}", namespace.repo_id));
    let details = serde_json::json!({
        "prefix": namespace.prefix,
        "deleted_refs": outcome.deleted_refs,
        "vanished_refs": outcome.vanished_refs,
        "refs": outcome.refs,
        // The one place this feature deliberately steps around a guard. The
        // reason is recorded, not just the fact, so an auditor can judge it.
        "bypassed_guard": "on_delete",
        "bypass_reason": "the draft cleanup is not a user deleting a branch: on_delete refuses \
                          the default branch, protected branches and any branch an open pull \
                          request occupies, which would make a draft undeletable and re-alert \
                          every hour. The cleanup deletes at the ref level with a \
                          compare-and-swap instead.",
    });
    let entry = rg_db::entities::audit_log::ActiveModel {
        id: sea_orm::NotSet,
        user_id: sea_orm::Set(None),
        username: sea_orm::Set(None),
        action: sea_orm::Set("agent_draft.cleaned".to_string()),
        resource_type: sea_orm::Set(Some("agent_draft_namespace".to_string())),
        resource_id: sea_orm::Set(Some(namespace.id)),
        resource_name: sea_orm::Set(Some(namespace.prefix.clone())),
        ip_address: sea_orm::Set(None),
        user_agent: sea_orm::Set(None),
        details: sea_orm::Set(Some(details.to_string())),
        on_behalf_of_user_id: sea_orm::Set(None),
        created_at: sea_orm::Set(Utc::now()),
    };
    if let Err(error) = rg_db::ops::audit_log_ops::insert(&state.db, entry).await {
        tracing::warn!(
            namespace_id = namespace.id,
            repo = %repo_name,
            %error,
            "could not write the draft cleanup audit entry"
        );
    }
}

// ── helpers ──────────────────────────────────────────────────────────────

async fn load_namespace(state: &AppState, repo_id: i64, id: i64) -> Result<Namespace, AppError> {
    let namespace = rg_db::ops::agent_draft_ops::find_by_id(&state.db, id)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::not_found("draft namespace not found"))?;
    if namespace.repo_id != repo_id {
        // Same answer as "not found": the id belongs to another repository.
        return Err(AppError::not_found("draft namespace not found"));
    }
    Ok(namespace)
}

/// Whether the `agent/<segment>/` in `prefix` belongs to `actor_id`.
///
/// The segment is either the caller's own id/username or an agent the caller
/// owns. Without this, an agent could reserve another agent's directory and
/// that agent's pushes would be refused by the TTL gate.
async fn agent_segment_is_owned_by(
    state: &AppState,
    prefix: &str,
    actor_id: i64,
) -> Result<bool, AppError> {
    let segment = prefix
        .split('/')
        .nth(1)
        .ok_or_else(|| AppError::bad_request("draft prefix is missing its agent segment"))?;
    let account = if let Ok(id) = segment.parse::<i64>() {
        rg_db::ops::user_ops::find_by_id(&state.db, id)
            .await
            .map_err(AppError::internal)?
    } else {
        rg_db::ops::user_ops::find_by_username(&state.db, segment)
            .await
            .map_err(AppError::internal)?
    };
    let Some(account) = account else {
        return Ok(false);
    };
    if account.id == actor_id {
        return Ok(true);
    }
    Ok(rg_core::agent::is_agent(&account) && account.owner_user_id == Some(actor_id))
}

/// The bare repository's path on disk, resolved from its owner.
pub(crate) async fn repo_disk_path(
    state: &AppState,
    repo: &Repository,
) -> anyhow::Result<std::path::PathBuf> {
    let owner = match repo.org_id {
        Some(org_id) => rg_db::ops::org_ops::get_org(&state.db, org_id)
            .await?
            .map(|org| org.name),
        None => rg_db::ops::user_ops::find_by_id(&state.db, repo.owner_id)
            .await?
            .map(|user| user.username),
    }
    .ok_or_else(|| anyhow::anyhow!("owner of repository {} no longer exists", repo.id))?;
    Ok(state.repo_root.join(format!("{owner}/{}.git", repo.name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg_db::entities::agent_draft_namespace::STATE_ACTIVE;

    #[test]
    fn the_response_carries_the_delete_deadline_not_just_the_expiry() {
        let now = Utc::now();
        let namespace = Namespace {
            id: 7,
            repo_id: 3,
            prefix: "agent/42/".into(),
            owner_user_id: 11,
            ttl_secs: 3600,
            expires_at: now,
            grace_secs: 60,
            state: STATE_ACTIVE.into(),
            extended_count: 0,
            created_at: now - Duration::seconds(3600),
        };
        let response = DraftNamespaceResponse::from(&namespace);
        assert_eq!(response.id, 7);
        assert_eq!(response.prefix, "agent/42/");
        assert_eq!(
            response.delete_due_at,
            (now + Duration::seconds(60)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        );
    }
}
