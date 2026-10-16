//! Guard: every REST handler carrying `#[utoipa::path]` must be registered in
//! `openapi.rs::paths(...)`.
//!
//! `#[utoipa::path]` alone does nothing — the handler still has to be listed in
//! the `#[openapi(paths(...))]` attribute or it is silently absent from
//! `/api-docs/openapi.json`. That is exactly how the two team-repository routes
//! shipped undocumented. This test scans the API sources for annotated handlers
//! and asserts each one is referenced by `openapi.rs`, so the omission fails here
//! instead of being discovered in the docs.

use std::collections::BTreeSet;
use std::path::Path;

/// Collect the handler function names that carry a `#[utoipa::path]` attribute.
fn annotated_handlers(source: &str) -> BTreeSet<String> {
    let mut handlers = BTreeSet::new();
    let lines: Vec<&str> = source.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        if !line.trim_start().starts_with("#[utoipa::path") {
            continue;
        }
        // The attribute spans multiple lines; find the `pub async fn <name>`
        // that follows it.
        for candidate in lines.iter().skip(index) {
            let trimmed = candidate.trim_start();
            if let Some(rest) = trimmed.strip_prefix("pub async fn ") {
                if let Some(name) = rest.split(['(', '<']).next() {
                    handlers.insert(name.trim().to_string());
                }
                break;
            }
            // Stop if we hit the next attribute (a new item started).
            if trimmed.starts_with("#[") && trimmed != line.trim_start() {
                break;
            }
        }
    }
    handlers
}

/// Every `crate::api::<module>::<handler>` referenced in openapi.rs.
fn registered_handlers(source: &str) -> BTreeSet<String> {
    let mut registered = BTreeSet::new();
    for line in source.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("crate::api::") else {
            continue;
        };
        let Some((_module, handler)) = rest.split_once("::") else {
            continue;
        };
        let handler = handler.trim_end_matches(',').trim();
        if !handler.is_empty() && handler.chars().all(|c| c.is_alphanumeric() || c == '_') {
            registered.insert(handler.to_string());
        }
    }
    registered
}

#[test]
fn every_annotated_handler_is_registered_in_openapi() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let api_dir = Path::new(manifest).join("src/api");
    let openapi_src = std::fs::read_to_string(Path::new(manifest).join("src/openapi.rs"))
        .expect("read openapi.rs");
    let registered = registered_handlers(&openapi_src);

    let mut missing = Vec::new();
    let mut seen = 0usize;
    for entry in std::fs::read_dir(&api_dir).expect("read src/api") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("read api source");
        for handler in annotated_handlers(&source) {
            seen += 1;
            if !registered.contains(&handler) {
                missing.push(format!("{}::{}", path.file_name().unwrap().to_string_lossy(), handler));
            }
        }
    }

    // The scan must find exactly as many handlers as there are annotations,
    // otherwise it could silently skip one and let an omission through.
    let mut annotations = 0usize;
    for entry in std::fs::read_dir(&api_dir).expect("read src/api (2nd pass)") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("read api source");
        annotations += source.matches("#[utoipa::path").count();
    }
    assert_eq!(
        seen, annotations,
        "the scanner found {seen} handlers for {annotations} annotations; \
         a mismatch means it is skipping handlers and would hide omissions"
    );
    assert!(seen > 100, "scan found only {seen} annotated handlers");
    assert!(
        missing.is_empty(),
        "these handlers have #[utoipa::path] but are missing from openapi.rs::paths(...), \
         so they do not appear in /api-docs/openapi.json: {missing:?}"
    );
}

/// The two team-repository routes must be present in the generated spec.
#[test]
fn team_repository_routes_are_in_the_spec() {
    let spec = rg_http::openapi::openapi_spec();
    assert!(
        spec.contains("/orgs/{name}/teams/{team_id}/repositories"),
        "the team-repository route must be documented in the OpenAPI spec"
    );
}