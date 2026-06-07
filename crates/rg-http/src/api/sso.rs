//! SSO (Single Sign-On) API endpoints.
//!
//! Endpoints:
//!   GET  /auth/sso/providers     — List enabled SSO providers
//!   GET  /auth/sso/{slug}        — Redirect to provider's auth page
//!   GET  /auth/sso/{slug}/callback — OAuth2/OIDC callback

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing;

use crate::AppState;

/// Extract base URL from the request's Host header.
fn get_base_url(headers: &HeaderMap) -> String {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:8080");
    let scheme = if host.contains(":443") || host.contains(":8443") { "https" } else { "http" };
    format!("{}://{}", scheme, host)
}

#[derive(Debug, Serialize)]
pub(crate) struct SsoProviderInfo {
    slug: String,
    name: String,
    provider_type: String,
    icon_url: Option<String>,
}

/// GET /auth/sso/providers
/// List enabled SSO providers for the login page.
pub(crate) async fn list_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<SsoProviderInfo>>, (StatusCode, String)> {
    let providers = rg_db::ops::sso_provider_ops::list_enabled(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("DB error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
        })?;

    let infos: Vec<SsoProviderInfo> = providers
        .into_iter()
        .map(|p| SsoProviderInfo {
            slug: p.slug,
            name: p.name,
            provider_type: p.provider_type,
            icon_url: p.icon_url,
        })
        .collect();

    Ok(Json(infos))
}

#[derive(Debug, Deserialize)]
pub(crate) struct SsoCallbackQuery {
    code: String,
    #[allow(dead_code)]
    state: Option<String>,
}

/// GET /auth/sso/{slug}
/// Redirect user to the SSO provider's authorization page.
pub(crate) async fn authorize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let provider = rg_db::ops::sso_provider_ops::find_by_slug(&state.db, &slug)
        .await
        .map_err(|e| {
            tracing::error!("DB error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
        })?
        .ok_or((StatusCode::NOT_FOUND, format!("SSO provider '{}' not found", slug)))?;

    if !provider.enabled {
        return Err((StatusCode::FORBIDDEN, "SSO provider is disabled".into()));
    }

    // Derive base URL from request Host header
    let base_url = get_base_url(&headers);
    let redirect_url = format!("{}/auth/sso/{}/callback", base_url.trim_end_matches('/'), slug);

    // Decrypt client secret
    let enc_key = rg_core::auth::encryption::derive_key(&state.jwt_secret);
    let client_secret = provider
        .client_secret_enc
        .as_ref()
        .map(|s| rg_core::auth::encryption::decrypt(s, &enc_key))
        .transpose()
        .map_err(|e| {
            tracing::error!("Decryption error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "decryption failed".into())
        })?
        .unwrap_or_default();

    let config = rg_core::auth::sso::SsoProviderConfig {
        slug: provider.slug.clone(),
        provider_type: provider.provider_type.clone(),
        client_id: provider.client_id.unwrap_or_default(),
        client_secret,
        redirect_url,
        scopes: provider
            .scopes
            .as_deref()
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        discovery_url: provider.discovery_url.clone(),
    };

    let (auth_url, _csrf_token) = rg_core::auth::sso::oauth2_authorize_url(&config).map_err(
        |e| {
            tracing::error!("SSO authorize error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "SSO authorization failed".into())
        },
    )?;

    Ok(Redirect::temporary(&auth_url))
}

#[derive(Debug, Serialize)]
pub(crate) struct LoginResponse {
    token: String,
    user_id: i64,
    username: String,
    mfa_required: bool,
}

/// GET /auth/sso/{slug}/callback
/// Handle the OAuth2 callback, exchange code for token, and issue JWT.
pub(crate) async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(query): Query<SsoCallbackQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let provider = rg_db::ops::sso_provider_ops::find_by_slug(&state.db, &slug)
        .await
        .map_err(|e| {
            tracing::error!("DB error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
        })?
        .ok_or((StatusCode::NOT_FOUND, format!("SSO provider '{}' not found", slug)))?;

    if !provider.enabled {
        return Err((StatusCode::FORBIDDEN, "SSO provider is disabled".into()));
    }

    let base_url = get_base_url(&headers);
    let redirect_url = format!("{}/auth/sso/{}/callback", base_url.trim_end_matches('/'), slug);

    let enc_key = rg_core::auth::encryption::derive_key(&state.jwt_secret);
    let client_secret = provider
        .client_secret_enc
        .as_ref()
        .map(|s| rg_core::auth::encryption::decrypt(s, &enc_key))
        .transpose()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("decryption: {}", e)))?
        .unwrap_or_default();

    let config = rg_core::auth::sso::SsoProviderConfig {
        slug: provider.slug.clone(),
        provider_type: provider.provider_type.clone(),
        client_id: provider.client_id.unwrap_or_default(),
        client_secret,
        redirect_url,
        scopes: provider
            .scopes
            .as_deref()
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        discovery_url: provider.discovery_url.clone(),
    };

    // Exchange code for token
    let access_token = rg_core::auth::sso::oauth2_exchange_code(&config, &query.code)
        .await
        .map_err(|e| {
            tracing::error!("SSO token exchange error: {}", e);
            (StatusCode::BAD_REQUEST, "failed to exchange authorization code".into())
        })?;

    // Fetch user info
    let user_info =
        rg_core::auth::sso::oauth2_fetch_user_info(&config, &access_token)
            .await
            .map_err(|e| {
                tracing::error!("SSO user info error: {}", e);
                (StatusCode::BAD_REQUEST, "failed to fetch user info".into())
            })?;

    // Find or create user
    let user_id = find_or_create_sso_user(&state, &provider.slug, &user_info, &access_token).await?;

    let user = rg_db::ops::user_ops::find_by_id(&state.db, user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "user not found after creation".into()))?;

    // Log successful login
    let _ = rg_db::ops::login_log_ops::log_attempt(
        &state.db,
        Some(user_id),
        &user.username,
        &provider.slug,
        None,
        None,
        true,
        None,
    )
    .await;

    // If MFA is enabled, redirect to MFA verify
    if user.mfa_enabled {
        return Ok(Json(LoginResponse {
            token: String::new(),
            user_id: user.id,
            username: user.username,
            mfa_required: true,
        }));
    }

    // Issue JWT
    let token =
        rg_core::auth::jwt::generate_token(user.id, &user.username, &state.jwt_secret, 7)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(LoginResponse {
        token,
        user_id: user.id,
        username: user.username,
        mfa_required: false,
    }))
}

/// Find existing SSO account or create new user + account.
async fn find_or_create_sso_user(
    state: &AppState,
    provider_slug: &str,
    user_info: &rg_core::auth::sso::SsoUserInfo,
    access_token: &str,
) -> Result<i64, (StatusCode, String)> {
    let db = &state.db;

    // Check if OAuth account already exists
    if let Some(oauth) = rg_db::ops::oauth_account_ops::find_by_provider_and_uid(
        db,
        provider_slug,
        &user_info.provider_user_id,
    )
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "DB error".into()))?
    {
        return Ok(oauth.user_id);
    }

    // Check if user with this email already exists
    let user_id =
        if let Some(existing) = rg_db::ops::user_ops::find_by_email(db, &user_info.email)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "DB error".into()))?
        {
            existing.id
        } else {
            // Create new user
            let username = generate_unique_username(db, &user_info.provider_username)
                .await
                .map_err(|_| {
                    (StatusCode::INTERNAL_SERVER_ERROR, "failed to generate username".into())
                })?;

            let enc_key = rg_core::auth::encryption::derive_key(&state.jwt_secret);
            let _enc_access = rg_core::auth::encryption::encrypt(access_token, &enc_key)
                .unwrap_or_default();

            rg_db::ops::user_ops::create_user(
                db,
                &username,
                &user_info.email,
                "", // no password for SSO users
                user_info.display_name.as_deref().unwrap_or(&username),
            )
            .await
            .map_err(|e| {
                tracing::error!("Failed to create SSO user: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("user creation failed: {}", e),
                )
            })?
            .id
        };

    // Upsert OAuth account
    rg_db::ops::oauth_account_ops::upsert(
        db,
        user_id,
        provider_slug,
        &user_info.provider_user_id,
        &user_info.provider_username,
        &user_info.email,
        Some(access_token),
        None,
        None,
    )
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "failed to link OAuth account".into()))?;

    Ok(user_id)
}

/// Generate a unique username based on the provider username.
async fn generate_unique_username(
    db: &sea_orm::DatabaseConnection,
    base: &str,
) -> Result<String, anyhow::Error> {
    // Try the base name first
    if rg_db::ops::user_ops::find_by_username(db, base)
        .await?
        .is_none()
    {
        return Ok(base.to_string());
    }
    // Append numbers
    for i in 1..100 {
        let candidate = format!("{}_{}", base, i);
        if rg_db::ops::user_ops::find_by_username(db, &candidate)
            .await?
            .is_none()
        {
            return Ok(candidate);
        }
    }
    // Fallback: random suffix
    let suffix: String = std::iter::repeat(())
        .take(6)
        .map(|_| rand::random::<u8>() % 26 + b'a')
        .map(|c| c as char)
        .collect();
    Ok(format!("{}_{}", base, suffix))
}
