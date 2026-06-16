//! Repository archive download.
//!
//! GET /api/v1/repos/{owner}/{name}/archive/{sha}.zip
//! GET /api/v1/repos/{owner}/{name}/archive/{sha}.tar.gz

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
};
use std::process::Command;

use crate::AppState;

/// GET /api/v1/repos/{owner}/{name}/archive/{sha}.zip
pub async fn download_archive(
    State(state): State<AppState>,
    Path((owner, name, sha, ext)): Path<(String, String, String, String)>,
) -> impl IntoResponse {
    let repo_path = state.repo_root.join(format!("{}/{}.git", owner, name));
    if !repo_path.exists() {
        return (StatusCode::NOT_FOUND, "Repository not found").into_response();
    }

    let format_flag = match ext.as_str() {
        "zip" => "zip",
        "tar.gz" | "tgz" => "tar.gz",
        _ => return (StatusCode::BAD_REQUEST, "Unsupported format").into_response(),
    };

    let mime = match ext.as_str() {
        "zip" => "application/zip",
        _ => "application/gzip",
    };

    let output = match Command::new("git")
        .arg("-C")
        .arg(&repo_path)
        .args(["archive", &format!("--format={}", format_flag), &sha])
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        Ok(_) => return (StatusCode::BAD_REQUEST, "Invalid ref or SHA").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let filename = format!("{}-{}.{}", name, &sha[..7], ext);

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"{}\"", filename)),
        ],
        output,
    )
        .into_response()
}
