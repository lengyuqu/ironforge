//! REST API handlers for organizations and teams.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};

use crate::AppState;

// ── Response types ───────────────────────────────────────────

#[derive(Serialize)]
struct OrgResponse {
    id: i64,
    name: String,
    display_name: Option<String>,
    description: Option<String>,
    owner_id: i64,
    visibility: String,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
struct OrgMemberResponse {
    id: i64,
    org_id: i64,
    user_id: i64,
    role: String,
    created_at: String,
}

#[derive(Serialize)]
struct TeamResponse {
    id: i64,
    org_id: i64,
    name: String,
    description: Option<String>,
    permission: String,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
struct TeamMemberResponse {
    id: i64,
    team_id: i64,
    user_id: i64,
    role: String,
    created_at: String,
}

#[derive(Deserialize)]
pub struct CreateOrgRequest {
    name: String,
    display_name: Option<String>,
    description: Option<String>,
    visibility: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateOrgRequest {
    display_name: Option<String>,
    description: Option<String>,
    visibility: Option<String>,
}

#[derive(Deserialize)]
pub struct AddOrgMemberRequest {
    user_id: i64,
    role: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateTeamRequest {
    name: String,
    description: Option<String>,
    permission: Option<String>,
}

#[derive(Deserialize)]
pub struct AddTeamMemberRequest {
    user_id: i64,
    role: Option<String>,
}

// ── Organization handlers ────────────────────────────────────

/// POST /api/v1/orgs
pub async fn create_org(
    State(state): State<AppState>,
    Json(body): Json<CreateOrgRequest>,
) -> impl IntoResponse {
    // TODO: extract user_id from JWT auth header
    // For now, require owner_id in the request or use a default
    let visibility = body.visibility.as_deref().unwrap_or("public");

    match rg_core::org::create_org(
        &state.db,
        &body.name,
        body.display_name.as_deref(),
        body.description.as_deref(),
        1, // TODO: extract from JWT
        visibility,
    )
    .await
    {
        Ok(org) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": org.id,
                "name": org.name,
                "display_name": org.display_name,
                "visibility": org.visibility,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /api/v1/orgs/:name
pub async fn get_org(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match rg_core::org::get_org_by_name(&state.db, &name).await {
        Ok(Some(org)) => Json(org_to_response(&org)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "organization not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /api/v1/orgs
/// List organizations for a user (query param: user_id)
pub async fn list_orgs(
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let user_id: i64 = params
        .get("user_id")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1); // TODO: extract from JWT

    match rg_core::org::list_user_orgs(&state.db, user_id).await {
        Ok(orgs) => {
            let resp: Vec<OrgResponse> = orgs.iter().map(org_to_response).collect();
            Json(resp).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
        .into_response(),
    }
}

/// PATCH /api/v1/orgs/:name
pub async fn update_org(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<UpdateOrgRequest>,
) -> impl IntoResponse {
    let org = match rg_core::org::get_org_by_name(&state.db, &name).await {
        Ok(Some(o)) => o,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "organization not found"})),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    match rg_core::org::update_org(
        &state.db,
        org.id,
        body.display_name.as_deref(),
        body.description.as_deref(),
        body.visibility.as_deref(),
    )
    .await
    {
        Ok(updated) => Json(org_to_response(&updated)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
        .into_response(),
    }
}

/// DELETE /api/v1/orgs/:name
pub async fn delete_org(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let org = match rg_core::org::get_org_by_name(&state.db, &name).await {
        Ok(Some(o)) => o,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "organization not found"})),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    match rg_core::org::delete_org(&state.db, org.id, 1).await {
        // TODO: extract user from JWT
        Ok(()) => Json(serde_json::json!({"deleted": true})).into_response(),
        Err(e) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── Organization Member handlers ────────────────────────────

/// GET /api/v1/orgs/:name/members
pub async fn list_org_members(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let org = match rg_core::org::get_org_by_name(&state.db, &name).await {
        Ok(Some(o)) => o,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "organization not found"})),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    match rg_core::org::list_org_members(&state.db, org.id).await {
        Ok(members) => {
            let resp: Vec<OrgMemberResponse> = members
                .into_iter()
                .map(|m| OrgMemberResponse {
                    id: m.id,
                    org_id: m.org_id,
                    user_id: m.user_id,
                    role: m.role,
                    created_at: m.created_at.to_string(),
                })
                .collect();
            Json(resp).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
        .into_response(),
    }
}

/// POST /api/v1/orgs/:name/members
pub async fn add_org_member(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<AddOrgMemberRequest>,
) -> impl IntoResponse {
    let org = match rg_core::org::get_org_by_name(&state.db, &name).await {
        Ok(Some(o)) => o,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "organization not found"})),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let role = body.role.as_deref().unwrap_or("member");

    match rg_core::org::add_org_member(&state.db, org.id, body.user_id, role).await {
        Ok(m) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": m.id,
                "org_id": m.org_id,
                "user_id": m.user_id,
                "role": m.role,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// DELETE /api/v1/orgs/:name/members/:user_id
pub async fn remove_org_member(
    State(state): State<AppState>,
    Path((name, user_id)): Path<(String, i64)>,
) -> impl IntoResponse {
    let org = match rg_core::org::get_org_by_name(&state.db, &name).await {
        Ok(Some(o)) => o,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "organization not found"})),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    match rg_core::org::remove_org_member(&state.db, org.id, user_id).await {
        Ok(()) => Json(serde_json::json!({"removed": true})).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── Team handlers ────────────────────────────────────────────

/// POST /api/v1/orgs/:name/teams
pub async fn create_team(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<CreateTeamRequest>,
) -> impl IntoResponse {
    let org = match rg_core::org::get_org_by_name(&state.db, &name).await {
        Ok(Some(o)) => o,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "organization not found"})),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let permission = body.permission.as_deref().unwrap_or("read");

    match rg_core::org::create_team(&state.db, org.id, &body.name, body.description.as_deref(), permission).await {
        Ok(team) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": team.id,
                "org_id": team.org_id,
                "name": team.name,
                "permission": team.permission,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /api/v1/orgs/:name/teams
pub async fn list_org_teams(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let org = match rg_core::org::get_org_by_name(&state.db, &name).await {
        Ok(Some(o)) => o,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "organization not found"})),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    match rg_core::org::list_org_teams(&state.db, org.id).await {
        Ok(teams) => {
            let resp: Vec<TeamResponse> = teams
                .into_iter()
                .map(|t| TeamResponse {
                    id: t.id,
                    org_id: t.org_id,
                    name: t.name,
                    description: t.description,
                    permission: t.permission,
                    created_at: t.created_at.to_string(),
                    updated_at: t.updated_at.to_string(),
                })
                .collect();
            Json(resp).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
        .into_response(),
    }
}

/// GET /api/v1/orgs/:name/teams/:team_id
pub async fn get_team(
    State(state): State<AppState>,
    Path((_name, team_id)): Path<(String, i64)>,
) -> impl IntoResponse {
    match rg_core::org::get_team(&state.db, team_id).await {
        Ok(Some(t)) => Json(TeamResponse {
            id: t.id,
            org_id: t.org_id,
            name: t.name,
            description: t.description,
            permission: t.permission,
            created_at: t.created_at.to_string(),
            updated_at: t.updated_at.to_string(),
        })
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "team not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// DELETE /api/v1/orgs/:name/teams/:team_id
pub async fn delete_team(
    State(state): State<AppState>,
    Path((_name, team_id)): Path<(String, i64)>,
) -> impl IntoResponse {
    match rg_core::org::delete_team(&state.db, team_id).await {
        Ok(()) => Json(serde_json::json!({"deleted": true})).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /api/v1/orgs/:name/teams/:team_id/members
pub async fn list_team_members(
    State(state): State<AppState>,
    Path((_name, team_id)): Path<(String, i64)>,
) -> impl IntoResponse {
    match rg_core::org::list_team_members(&state.db, team_id).await {
        Ok(members) => {
            let resp: Vec<TeamMemberResponse> = members
                .into_iter()
                .map(|m| TeamMemberResponse {
                    id: m.id,
                    team_id: m.team_id,
                    user_id: m.user_id,
                    role: m.role,
                    created_at: m.created_at.to_string(),
                })
                .collect();
            Json(resp).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
        .into_response(),
    }
}

/// POST /api/v1/orgs/:name/teams/:team_id/members
pub async fn add_team_member(
    State(state): State<AppState>,
    Path((_name, team_id)): Path<(String, i64)>,
    Json(body): Json<AddTeamMemberRequest>,
) -> impl IntoResponse {
    let role = body.role.as_deref().unwrap_or("member");

    match rg_core::org::add_team_member(&state.db, team_id, body.user_id, role).await {
        Ok(m) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": m.id,
                "team_id": m.team_id,
                "user_id": m.user_id,
                "role": m.role,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// DELETE /api/v1/orgs/:name/teams/:team_id/members/:user_id
pub async fn remove_team_member(
    State(state): State<AppState>,
    Path((_name, team_id, user_id)): Path<(String, i64, i64)>,
) -> impl IntoResponse {
    match rg_core::org::remove_team_member(&state.db, team_id, user_id).await {
        Ok(()) => Json(serde_json::json!({"removed": true})).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── Helpers ──────────────────────────────────────────────────

fn org_to_response(org: &rg_db::entities::organization::Model) -> OrgResponse {
    OrgResponse {
        id: org.id,
        name: org.name.clone(),
        display_name: org.display_name.clone(),
        description: org.description.clone(),
        owner_id: org.owner_id,
        visibility: org.visibility.clone(),
        created_at: org.created_at.to_string(),
        updated_at: org.updated_at.to_string(),
    }
}
