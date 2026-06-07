//! MFA (Multi-Factor Authentication) API endpoints.
//!
//! Endpoints:
//!   POST   /users/mfa/setup    — Generate TOTP secret + QR code
//!   POST   /users/mfa/enable   — Verify TOTP code and enable MFA
//!   POST   /users/mfa/disable  — Disable MFA (requires password)
//!   POST   /users/mfa/verify   — Verify TOTP code (during login)
//!   GET    /users/mfa/backup   — Get backup codes
//!   POST   /users/mfa/backup   — Verify and use a backup code

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use tracing;

use crate::AppState;
use crate::api::auth::extract_user_id;
use axum::http::HeaderMap;

#[derive(Debug, Serialize)]
pub(crate) struct SetupMfaResponse {
    secret: String,
    otpauth_url: String,
    qr_svg: String,
}

/// POST /users/mfa/setup
/// Generate a new TOTP secret and return an otpauth URL + QR code SVG.
pub(crate) async fn setup_mfa(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SetupMfaResponse>, (StatusCode, String)> {
    let user_id = extract_user_id(&headers, &state.jwt_secret)
        .ok_or((StatusCode::UNAUTHORIZED, "unauthorized".into()))?;

    // Get username to include in TOTP label
    let user = rg_db::ops::user_ops::find_by_id(&state.db, user_id)
        .await
        .map_err(|e| {
            tracing::error!("DB error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
        })?
        .ok_or((StatusCode::NOT_FOUND, "user not found".into()))?;

    let (secret, otpauth_url, _qr_text) =
        rg_core::auth::totp::generate_secret(&user.username, "IronForge")
            .map_err(|e| {
                tracing::error!("TOTP error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "TOTP generation failed".into())
            })?;

    let qr_svg = rg_core::auth::totp::generate_qr_svg(&otpauth_url);

    // Store the secret temporarily (encrypted) but don't enable MFA yet
    let enc_key = rg_core::auth::encryption::derive_key(&state.jwt_secret);
    let enc_secret = rg_core::auth::encryption::encrypt(&secret, &enc_key)
        .map_err(|e| {
            tracing::error!("Encryption error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "encryption failed".into())
        })?;

    rg_db::ops::user_ops::update_totp_secret(&state.db, user_id, &enc_secret)
        .await
        .map_err(|e| {
            tracing::error!("DB error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
        })?;

    Ok(Json(SetupMfaResponse {
        secret,
        otpauth_url,
        qr_svg,
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct EnableMfaRequest {
    code: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct EnableMfaResponse {
    enabled: bool,
    backup_codes: Vec<String>,
}

/// POST /users/mfa/enable
/// Verify the setup TOTP code and enable MFA, generating backup codes.
pub(crate) async fn enable_mfa(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<EnableMfaRequest>,
) -> Result<Json<EnableMfaResponse>, (StatusCode, String)> {
    let user_id = extract_user_id(&headers, &state.jwt_secret)
        .ok_or((StatusCode::UNAUTHORIZED, "unauthorized".into()))?;

    let user = rg_db::ops::user_ops::find_by_id(&state.db, user_id)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?
        .ok_or((StatusCode::NOT_FOUND, "user not found".into()))?;

    // Decrypt the TOTP secret
    let enc_key = rg_core::auth::encryption::derive_key(&state.jwt_secret);
    let totp_secret = match &user.totp_secret {
        Some(s) => rg_core::auth::encryption::decrypt(s, &enc_key).map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("decryption failed: {}", e))
        })?,
        None => return Err((StatusCode::BAD_REQUEST, "MFA not set up yet".into())),
    };

    // Verify the TOTP code
    let valid = rg_core::auth::totp::verify_code(&totp_secret, &req.code)
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    if !valid {
        return Err((StatusCode::BAD_REQUEST, "invalid TOTP code".into()));
    }

    // Enable MFA and store re-encrypted secret
    rg_db::ops::user_ops::enable_mfa(&state.db, user_id, "totp")
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    // Generate backup codes
    let backup_codes = rg_db::ops::mfa_backup_code_ops::generate_codes(8);
    rg_db::ops::mfa_backup_code_ops::set_codes(&state.db, user_id, &backup_codes)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    Ok(Json(EnableMfaResponse {
        enabled: true,
        backup_codes,
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct VerifyMfaRequest {
    username: String,
    code: String,
    /// If true, verify using a backup code instead of TOTP
    #[serde(default)]
    backup: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct VerifyMfaResponse {
    token: String,
    user_id: i64,
    username: String,
}

/// POST /users/mfa/verify
/// Second step of login: verify MFA code and issue JWT.
pub(crate) async fn verify_mfa(
    State(state): State<AppState>,
    Json(req): Json<VerifyMfaRequest>,
) -> Result<Json<VerifyMfaResponse>, (StatusCode, String)> {
    // Find user by username
    let user = rg_db::ops::user_ops::find_by_username(&state.db, &req.username)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?
        .ok_or_else(|| {
            tracing::warn!(username = %req.username, "MFA verify: user not found");
            (StatusCode::UNAUTHORIZED, "invalid credentials".into())
        })?;

    if !user.mfa_enabled {
        return Err((StatusCode::BAD_REQUEST, "MFA not enabled".into()));
    }

    if req.backup {
        // Verify backup code
        let valid = rg_db::ops::mfa_backup_code_ops::verify_and_consume(
            &state.db, user.id, &req.code,
        )
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

        if !valid {
            return Err((StatusCode::UNAUTHORIZED, "invalid backup code".into()));
        }
    } else {
        // Verify TOTP code
        let enc_key = rg_core::auth::encryption::derive_key(&state.jwt_secret);
        let totp_secret = user.totp_secret.as_ref()
            .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "MFA secret missing".into()))?;

        let secret = rg_core::auth::encryption::decrypt(totp_secret, &enc_key)
            .map_err(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            })?;

        let valid = rg_core::auth::totp::verify_code(&secret, &req.code)
            .map_err(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            })?;

        if !valid {
            return Err((StatusCode::UNAUTHORIZED, "invalid TOTP code".into()));
        }
    }

    // Issue JWT
    let token = rg_core::auth::jwt::generate_token(
        user.id,
        &user.username,
        &state.jwt_secret,
        7,
    )
    .map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    Ok(Json(VerifyMfaResponse {
        token,
        user_id: user.id,
        username: user.username,
    }))
}

/// GET /users/mfa/backup
/// Get existing backup codes status (does not reveal unused codes).
pub(crate) async fn get_backup_codes(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let user_id = extract_user_id(&headers, &state.jwt_secret)
        .ok_or((StatusCode::UNAUTHORIZED, "unauthorized".into()))?;

    let codes = rg_db::ops::mfa_backup_code_ops::list_codes(&state.db, user_id)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    let summary: Vec<serde_json::Value> = codes
        .iter()
        .map(|c| serde_json::json!({
            "used": c.used,
            "used_at": c.used_at,
            "created_at": c.created_at,
        }))
        .collect();

    Ok(Json(serde_json::json!({
        "total": codes.len(),
        "unused": codes.iter().filter(|c| !c.used).count(),
        "codes": summary,
    })))
}

#[derive(Debug, Deserialize)]
pub(crate) struct DisableMfaRequest {
    password: String,
}

/// POST /users/mfa/disable
/// Disable MFA (requires current password for security).
pub(crate) async fn disable_mfa(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DisableMfaRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let user_id = extract_user_id(&headers, &state.jwt_secret)
        .ok_or((StatusCode::UNAUTHORIZED, "unauthorized".into()))?;

    let user = rg_db::ops::user_ops::find_by_id(&state.db, user_id)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?
        .ok_or((StatusCode::NOT_FOUND, "user not found".into()))?;

    // Verify password before disabling MFA
    rg_core::auth::password::verify_password(&req.password, &user.password_hash)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "invalid password".into()))?;

    rg_db::ops::user_ops::disable_mfa(&state.db, user_id)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

    Ok(StatusCode::OK)
}
