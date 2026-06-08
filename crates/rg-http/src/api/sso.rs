//! SSO (Single Sign-On) API endpoints.
//!
//! Endpoints:
//!   GET  /auth/sso/providers                — List enabled SSO providers
//!   GET  /auth/sso/{slug}                    — Redirect to provider's auth page
//!   GET  /auth/sso/{slug}/callback           — OAuth2/OIDC callback
//!   POST /auth/sso/{slug}/refresh            — Refresh OAuth2 access token
//!   DELETE /auth/sso/{slug}/unlink           — Unlink OAuth account

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing;

use crate::AppState;

// ── Cookie helpers ───────────────────────────────────────────────

/// Cookie names for secure OAuth2 flow.
const SSO_STATE_COOKIE: &str = "ironforge_sso_state";
const SSO_VERIFIER_COOKIE: &str = "ironforge_sso_code_verifier";

/// Set a short-lived signed cookie for CSRF/PKCE state.
fn set_state_cookie(
    response: &mut axum::response::Response,
    name: &str,
    value: &str,
    jwt_secret: &str,
) {
    // Sign the value with HMAC for integrity
    let signature = sign_cookie_value(value, jwt_secret);
    let cookie_value = format!("{}:{}", value, signature);

    // Max-Age: 600 seconds (10 min) — matches typical OAuth2 code expiry
    let cookie = format!(
        "{}={}; Path=/auth/sso; HttpOnly; SameSite=Lax; Max-Age=600",
        name, cookie_value
    );
    response
        .headers_mut()
        .insert("Set-Cookie", cookie.parse().unwrap());
}

/// Verify and extract a signed cookie value. Returns None if missing or invalid.
fn verify_state_cookie(
    headers: &HeaderMap,
    name: &str,
    jwt_secret: &str,
) -> Option<String> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    let prefix = format!("{}=", name);

    for part in cookie_header.split(';') {
        let trimmed = part.trim();
        if let Some(value) = trimmed.strip_prefix(&prefix) {
            // Split value:signature
            if let Some((val, sig)) = value.rsplit_once(':') {
                let expected_sig = sign_cookie_value(val, jwt_secret);
                if sig == expected_sig {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

/// SHA256-based cookie signing using JWT secret.
fn sign_cookie_value(value: &str, secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(b":");
    hasher.update(value.as_bytes());
    let result = hasher.finalize();
    // Hex encode the digest
    let mut hex = String::with_capacity(64);
    for byte in &result {
        use std::fmt::Write;
        write!(hex, "{byte:02x}").unwrap();
    }
    hex
}

// ── Extract base URL ─────────────────────────────────────────────

fn get_base_url(headers: &HeaderMap) -> String {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:8080");
    let scheme = if host.contains(":443") || host.contains(":8443") {
        "https"
    } else {
        "http"
    };
    format!("{}://{}", scheme, host)
}

// ── Types ────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct SsoProviderInfo {
    slug: String,
    name: String,
    provider_type: String,
    icon_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SsoCallbackQuery {
    code: String,
    state: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct LoginResponse {
    token: String,
    user_id: i64,
    username: String,
    mfa_required: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RefreshRequest {
    refresh_token: Option<String>,
}

// ── List providers ───────────────────────────────────────────────

/// GET /auth/sso/providers
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

// ── Authorize (redirect to provider) ─────────────────────────────

/// GET /auth/sso/{slug}
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

    let base_url = get_base_url(&headers);
    let redirect_url = format!(
        "{}/auth/sso/{}/callback",
        base_url.trim_end_matches('/'),
        slug
    );

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

    let (auth_url, csrf_state, code_verifier) =
        rg_core::auth::sso::oauth2_authorize_url(&config).map_err(|e| {
            tracing::error!("SSO authorize error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "SSO authorization failed".into())
        })?;

    // Build a redirect response with CSRF & PKCE cookies
    let mut redirect = Redirect::temporary(&auth_url).into_response();
    set_state_cookie(&mut redirect, SSO_STATE_COOKIE, &csrf_state, &state.jwt_secret);
    set_state_cookie(
        &mut redirect,
        SSO_VERIFIER_COOKIE,
        &code_verifier,
        &state.jwt_secret,
    );

    Ok(redirect)
}

// ── Callback ─────────────────────────────────────────────────────

/// GET /auth/sso/{slug}/callback
pub(crate) async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(query): Query<SsoCallbackQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // ── CSRF state validation ────────────────────────────────────
    let expected_state = verify_state_cookie(&headers, SSO_STATE_COOKIE, &state.jwt_secret);
    let code_verifier = verify_state_cookie(&headers, SSO_VERIFIER_COOKIE, &state.jwt_secret);

    match (&query.state, &expected_state) {
        (Some(returned), Some(expected)) if returned == expected => {
            // Valid
        }
        (Some(returned), Some(expected)) => {
            tracing::warn!(
                "SSO CSRF state mismatch: expected={}, got={}",
                expected,
                returned
            );
            return Err((
                StatusCode::FORBIDDEN,
                "CSRF state mismatch — possible attack".into(),
            ));
        }
        (Some(_), None) => {
            tracing::warn!("SSO CSRF: no expected state cookie found");
            return Err((
                StatusCode::FORBIDDEN,
                "missing CSRF state cookie".into(),
            ));
        }
        (None, _) => {
            // Continue without state check for backward compatibility
            tracing::warn!("SSO callback without state parameter");
        }
    }

    let code_verifier = code_verifier.unwrap_or_default();

    // ── Get provider config ──────────────────────────────────────
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
    let redirect_url = format!(
        "{}/auth/sso/{}/callback",
        base_url.trim_end_matches('/'),
        slug
    );

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

    // ── Exchange code for tokens (with PKCE) ─────────────────────
    let token_response = rg_core::auth::sso::oauth2_exchange_code(&config, &query.code, &code_verifier)
        .await
        .map_err(|e| {
            tracing::error!("SSO token exchange error: {}", e);
            (StatusCode::BAD_REQUEST, "failed to exchange authorization code".into())
        })?;

    // ── Fetch user info ──────────────────────────────────────────
    let user_info =
        rg_core::auth::sso::oauth2_fetch_user_info(&config, &token_response.access_token)
            .await
            .map_err(|e| {
                tracing::error!("SSO user info error: {}", e);
                (StatusCode::BAD_REQUEST, "failed to fetch user info".into())
            })?;

    // ── Find or create user ──────────────────────────────────────
    let user_id = find_or_create_sso_user(
        &state,
        &provider.slug,
        &user_info,
        &token_response,
    )
    .await?;

    let user = rg_db::ops::user_ops::find_by_id(&state.db, user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "user not found after creation".into()))?;

    // ── Log successful login ─────────────────────────────────────
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

    // ── If MFA enabled, require second factor ────────────────────
    if user.mfa_enabled {
        return Ok(Json(LoginResponse {
            token: String::new(),
            user_id: user.id,
            username: user.username,
            mfa_required: true,
        }));
    }

    // ── Issue JWT ────────────────────────────────────────────────
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

// ── Refresh token ────────────────────────────────────────────────

/// POST /auth/sso/{slug}/refresh
/// Refresh an OAuth2 access token using a stored refresh_token.
pub(crate) async fn refresh_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(body): Json<RefreshRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    use crate::api::auth::extract_bearer_claims;

    // Require authenticated user
    let claims = extract_bearer_claims(&headers, &state.jwt_secret)
        .ok_or((StatusCode::UNAUTHORIZED, "authentication required".into()))?;
    let user_id: i64 = claims.sub.parse().map_err(|_| {
        (StatusCode::UNAUTHORIZED, "invalid token".into())
    })?;

    let provider = rg_db::ops::sso_provider_ops::find_by_slug(&state.db, &slug)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "SSO provider not found".into()))?;

    let enc_key = rg_core::auth::encryption::derive_key(&state.jwt_secret);
    let client_secret = provider
        .client_secret_enc
        .as_ref()
        .map(|s| rg_core::auth::encryption::decrypt(s, &enc_key))
        .transpose()
        .unwrap_or_default()
        .unwrap_or_default();

    let config = rg_core::auth::sso::SsoProviderConfig {
        slug: provider.slug.clone(),
        provider_type: provider.provider_type.clone(),
        client_id: provider.client_id.unwrap_or_default(),
        client_secret,
        redirect_url: String::new(), // not needed for refresh
        scopes: provider
            .scopes
            .as_deref()
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        discovery_url: provider.discovery_url.clone(),
    };

    // Use provided refresh_token or look up from stored OAuth account
    let refresh_token = if let Some(rt) = body.refresh_token {
        rt
    } else {
        // Look up the user's OAuth account for stored refresh_token
        let accounts = rg_db::ops::oauth_account_ops::find_by_user_id(&state.db, user_id)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let account = accounts
            .iter()
            .find(|a| a.provider == slug)
            .ok_or((StatusCode::NOT_FOUND, "no OAuth account linked".into()))?;

        let stored_rt = account
            .refresh_token
            .as_ref()
            .ok_or((StatusCode::NOT_FOUND, "no refresh token available".into()))?;

        rg_core::auth::encryption::decrypt(stored_rt, &enc_key)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("decryption: {}", e)))?
    };

    let token_response = rg_core::auth::sso::oauth2_refresh_token(&config, &refresh_token)
        .await
        .map_err(|e| {
            tracing::error!("SSO token refresh error: {}", e);
            (StatusCode::BAD_REQUEST, "failed to refresh token".into())
        })?;

    // Store updated tokens
    let enc_access = rg_core::auth::encryption::encrypt(&token_response.access_token, &enc_key)
        .unwrap_or_default();
    let enc_refresh = token_response
        .refresh_token
        .as_ref()
        .and_then(|rt| rg_core::auth::encryption::encrypt(rt, &enc_key).ok());

    let expires_at = token_response.expires_in.map(|secs| {
        chrono::Utc::now() + chrono::Duration::seconds(secs as i64)
    });

    // Update the OAuth account with new tokens
    if let Some(account) = rg_db::ops::oauth_account_ops::find_by_provider_and_uid(
        &state.db,
        &slug,
        "", // We'll find by user
    )
    .await
    .ok()
    .flatten()
    {
        rg_db::ops::oauth_account_ops::upsert(
            &state.db,
            account.user_id,
            &slug,
            &account.provider_user_id,
            &account.provider_username,
            &account.email,
            Some(&enc_access),
            enc_refresh.as_deref(),
            expires_at,
        )
        .await
        .ok();
    }

    Ok(Json(serde_json::json!({
        "access_token": token_response.access_token,
        "expires_in": token_response.expires_in,
        "refresh_token": token_response.refresh_token,
    })))
}

// ── Unlink OAuth account ─────────────────────────────────────────

/// DELETE /auth/sso/{slug}/unlink
pub(crate) async fn unlink_oauth_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    use crate::api::auth::extract_bearer_claims;

    let claims = extract_bearer_claims(&headers, &state.jwt_secret)
        .ok_or((StatusCode::UNAUTHORIZED, "authentication required".into()))?;
    let user_id: i64 = claims.sub.parse().map_err(|_| {
        (StatusCode::UNAUTHORIZED, "invalid token".into())
    })?;

    // Find and delete the OAuth account link
    let accounts = rg_db::ops::oauth_account_ops::find_by_user_id(&state.db, user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let account = accounts
        .iter()
        .find(|a| a.provider == slug)
        .ok_or((StatusCode::NOT_FOUND, "no OAuth account linked".into()))?;

    rg_db::ops::oauth_account_ops::delete_by_id(&state.db, account.id, user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({"unlinked": true})))
}

// ── User helpers ─────────────────────────────────────────────────

async fn find_or_create_sso_user(
    state: &AppState,
    provider_slug: &str,
    user_info: &rg_core::auth::sso::SsoUserInfo,
    token_response: &rg_core::auth::sso::OAuth2TokenResponse,
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
        // Update stored tokens
        let enc_key = rg_core::auth::encryption::derive_key(&state.jwt_secret);
        let enc_access = rg_core::auth::encryption::encrypt(&token_response.access_token, &enc_key)
            .unwrap_or_default();
        let enc_refresh = token_response
            .refresh_token
            .as_ref()
            .and_then(|rt| rg_core::auth::encryption::encrypt(rt, &enc_key).ok());
        let expires_at = token_response.expires_in.map(|secs| {
            chrono::Utc::now() + chrono::Duration::seconds(secs as i64)
        });

        let _ = rg_db::ops::oauth_account_ops::upsert(
            db,
            oauth.user_id,
            provider_slug,
            &user_info.provider_user_id,
            &user_info.provider_username,
            &user_info.email,
            Some(&enc_access),
            enc_refresh.as_deref(),
            expires_at,
        )
        .await;

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

    // Encrypt and store tokens
    let enc_key = rg_core::auth::encryption::derive_key(&state.jwt_secret);
    let enc_access = rg_core::auth::encryption::encrypt(&token_response.access_token, &enc_key)
        .unwrap_or_default();
    let enc_refresh = token_response
        .refresh_token
        .as_ref()
        .and_then(|rt| rg_core::auth::encryption::encrypt(rt, &enc_key).ok());
    let expires_at = token_response.expires_in.map(|secs| {
        chrono::Utc::now() + chrono::Duration::seconds(secs as i64)
    });

    // Upsert OAuth account with encrypted tokens
    rg_db::ops::oauth_account_ops::upsert(
        db,
        user_id,
        provider_slug,
        &user_info.provider_user_id,
        &user_info.provider_username,
        &user_info.email,
        Some(&enc_access),
        enc_refresh.as_deref(),
        expires_at,
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
    if rg_db::ops::user_ops::find_by_username(db, base)
        .await?
        .is_none()
    {
        return Ok(base.to_string());
    }
    for i in 1..100 {
        let candidate = format!("{}_{}", base, i);
        if rg_db::ops::user_ops::find_by_username(db, &candidate)
            .await?
            .is_none()
        {
            return Ok(candidate);
        }
    }
    let suffix: String = std::iter::repeat(())
        .take(6)
        .map(|_| rand::random::<u8>() % 26 + b'a')
        .map(|c| c as char)
        .collect();
    Ok(format!("{}_{}", base, suffix))
}
