//! Agent accounts a person owns.
//!
//! ```text
//! GET    /api/v1/agents                          the caller's agents
//! POST   /api/v1/agents                          create one
//! GET    /api/v1/agents/{username}
//! PATCH  /api/v1/agents/{username}               description, activate / deactivate
//! GET    /api/v1/agents/{username}/tokens
//! POST   /api/v1/agents/{username}/tokens        issue a token (repository scopes only)
//! DELETE /api/v1/agents/{username}/tokens/{id}
//! ```
//!
//! Every route needs a **person**: an agent's own token cannot reach `/agents`
//! at all (it needs the `user` scope, which an agent token cannot carry), and
//! [`caller`] refuses an agent account anyway. An agent the caller does not own
//! answers exactly like one that does not exist.
//!
//! See `rg_core::agent` for what an agent account is.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use utoipa::ToSchema;

use super::users::{AccessTokenResponse, CreateTokenRequest};
use crate::error::AppError;
use crate::AppState;

#[derive(serde::Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAgentRequest {
    /// Short name; the account is called `<your username>-agent-<name>`.
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(serde::Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAgentRequest {
    /// New description; an empty string clears it. Omit to leave it alone.
    #[serde(default)]
    pub description: Option<String>,
    /// `false` deactivates the agent: its tokens are refused and the sessions
    /// already minted from them stop working. `true` reactivates it.
    #[serde(default)]
    pub is_active: Option<bool>,
}

/// The calling account, which must be a person.
async fn caller(state: &AppState, headers: &HeaderMap) -> Result<i64, AppError> {
    let user_id = super::auth::extract_user_id(headers, &state.jwt_secret)
        .ok_or_else(|| AppError::unauthorized("authentication required"))?;
    let user = rg_core::agent::ensure_human(&state.db, user_id).await?;
    Ok(user.id)
}

/// GET /api/v1/agents
#[utoipa::path(
    get,
    path = "/agents",
    tag = "Agents",
    responses(
        (status = 200, description = "The caller's agent accounts", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "The caller is an agent account", body = serde_json::Value),
    ),
)]
pub async fn list_agents(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let owner_id = match caller(&state, &headers).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    match rg_core::agent::list_agents(&state.db, owner_id).await {
        Ok(agents) => (StatusCode::OK, Json(agents)).into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}

/// POST /api/v1/agents
#[utoipa::path(
    post,
    path = "/agents",
    tag = "Agents",
    request_body = CreateAgentRequest,
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 400, description = "Invalid name or description", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 403, description = "The caller is an agent account", body = serde_json::Value),
        (status = 409, description = "Name taken, or too many agents", body = serde_json::Value),
    ),
)]
pub async fn create_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateAgentRequest>,
) -> impl IntoResponse {
    let owner_id = match caller(&state, &headers).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let description = body
        .description
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty());
    match rg_core::agent::create_agent(&state.db, owner_id, body.name.trim(), description).await {
        Ok(agent) => {
            super::audit::record_write(
                &state.db,
                owner_id,
                "agent.create",
                "user",
                Some(agent.id),
                &agent.username,
                &headers,
                serde_json::json!({}),
            )
            .await;
            (StatusCode::CREATED, Json(agent)).into_response()
        }
        Err(e) => AppError::from(e).into_response(),
    }
}

/// GET /api/v1/agents/{username}
#[utoipa::path(
    get,
    path = "/agents/{username}",
    tag = "Agents",
    params(("username" = String, Path, description = "The agent's username")),
    responses(
        (status = 200, description = "The agent", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "Not one of the caller's agents", body = serde_json::Value),
    ),
)]
pub async fn get_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(username): Path<String>,
) -> impl IntoResponse {
    let owner_id = match caller(&state, &headers).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    match rg_core::agent::find_owned_agent(&state.db, owner_id, &username).await {
        Ok(agent) => (StatusCode::OK, Json(rg_core::agent::AgentInfo::from(agent))).into_response(),
        Err(e) => AppError::from(e).into_response(),
    }
}

/// PATCH /api/v1/agents/{username}
#[utoipa::path(
    patch,
    path = "/agents/{username}",
    tag = "Agents",
    params(("username" = String, Path, description = "The agent's username")),
    request_body = UpdateAgentRequest,
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 400, description = "Invalid description", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "Not one of the caller's agents", body = serde_json::Value),
    ),
)]
pub async fn update_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(username): Path<String>,
    Json(body): Json<UpdateAgentRequest>,
) -> impl IntoResponse {
    let owner_id = match caller(&state, &headers).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let description = body.description.map(|d| {
        let d = d.trim().to_string();
        (!d.is_empty()).then_some(d)
    });
    match rg_core::agent::update_agent(&state.db, owner_id, &username, description, body.is_active)
        .await
    {
        Ok(agent) => {
            super::audit::record_write(
                &state.db,
                owner_id,
                "agent.update",
                "user",
                Some(agent.id),
                &agent.username,
                &headers,
                serde_json::json!({ "is_active": body.is_active }),
            )
            .await;
            (StatusCode::OK, Json(agent)).into_response()
        }
        Err(e) => AppError::from(e).into_response(),
    }
}

/// GET /api/v1/agents/{username}/tokens
#[utoipa::path(
    get,
    path = "/agents/{username}/tokens",
    tag = "Agents",
    params(("username" = String, Path, description = "The agent's username")),
    responses(
        (status = 200, description = "The agent's tokens (never the secrets)", body = [AccessTokenResponse]),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "Not one of the caller's agents", body = serde_json::Value),
    ),
)]
pub async fn list_agent_tokens(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(username): Path<String>,
) -> impl IntoResponse {
    let owner_id = match caller(&state, &headers).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let agent = match rg_core::agent::find_owned_agent(&state.db, owner_id, &username).await {
        Ok(agent) => agent,
        Err(e) => return AppError::from(e).into_response(),
    };
    match rg_db::ops::token_ops::list_by_user(&state.db, agent.id).await {
        Ok(tokens) => {
            let tokens: Vec<AccessTokenResponse> = tokens.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(tokens)).into_response()
        }
        Err(e) => AppError::from(e).into_response(),
    }
}

/// POST /api/v1/agents/{username}/tokens
///
/// Same body as `POST /users/tokens`. The token may only carry repository
/// scopes and defaults to `repo:write`; `repos` and `refs` limit it further.
#[utoipa::path(
    post,
    path = "/agents/{username}/tokens",
    tag = "Agents",
    params(("username" = String, Path, description = "The agent's username")),
    request_body = CreateTokenRequest,
    responses(
        (status = 201, description = "Created; the only time the token is shown", body = serde_json::Value),
        (status = 400, description = "Invalid name, scopes or limits", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "Not one of the caller's agents", body = serde_json::Value),
        (status = 409, description = "The agent is deactivated", body = serde_json::Value),
    ),
)]
pub async fn create_agent_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(username): Path<String>,
    Json(body): Json<CreateTokenRequest>,
) -> impl IntoResponse {
    let owner_id = match caller(&state, &headers).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let agent = match rg_core::agent::find_owned_agent(&state.db, owner_id, &username).await {
        Ok(agent) => agent,
        Err(e) => return AppError::from(e).into_response(),
    };
    if !agent.is_active {
        return AppError::conflict("the agent is deactivated; reactivate it first").into_response();
    }
    let token_name = body.name.clone();
    let response = super::users::issue_token(&state, agent.id, body, true).await;
    if response.status() == StatusCode::CREATED {
        super::audit::record_write(
            &state.db,
            owner_id,
            "agent.token.create",
            "user",
            Some(agent.id),
            &agent.username,
            &headers,
            serde_json::json!({ "token_name": token_name }),
        )
        .await;
    }
    response
}

/// DELETE /api/v1/agents/{username}/tokens/{id}
#[utoipa::path(
    delete,
    path = "/agents/{username}/tokens/{id}",
    tag = "Agents",
    params(
        ("username" = String, Path, description = "The agent's username"),
        ("id" = i64, Path, description = "Token id"),
    ),
    responses(
        (status = 204, description = "Revoked"),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "No such agent or token", body = serde_json::Value),
    ),
)]
pub async fn delete_agent_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((username, id)): Path<(String, i64)>,
) -> impl IntoResponse {
    let owner_id = match caller(&state, &headers).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let agent = match rg_core::agent::find_owned_agent(&state.db, owner_id, &username).await {
        Ok(agent) => agent,
        Err(e) => return AppError::from(e).into_response(),
    };
    match rg_db::ops::token_ops::find_by_id(&state.db, id).await {
        Ok(Some(token)) if token.user_id == agent.id => {}
        Ok(_) => return AppError::not_found("token not found").into_response(),
        Err(e) => return AppError::from(e).into_response(),
    }
    match rg_db::ops::token_ops::delete_by_id(&state.db, id).await {
        Ok(()) => {
            super::audit::record_write(
                &state.db,
                owner_id,
                "agent.token.delete",
                "user",
                Some(agent.id),
                &agent.username,
                &headers,
                serde_json::json!({ "token_id": id }),
            )
            .await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => AppError::from(e).into_response(),
    }
}
