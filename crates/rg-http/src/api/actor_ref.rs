//! T26: attach "who is this account" ([`rg_core::agent::ActorRef`]) to API rows.
//!
//! The existing id/username fields stay exactly as they were; each row gains a
//! sibling `<name>_ref` key (`author_id` -> `author_ref`, `reviewer_id` ->
//! `reviewer_ref`, `user_id` -> `user_ref`). A new key rather than a reused
//! `author` because the web client already types `author` as a string.
//!
//! Enrichment never fails a request: if the lookup errors, or the account no
//! longer exists, the ref is `null` (same fail-soft rule as the fork
//! enrichment of a pull request).

use std::collections::HashMap;

use rg_core::agent::ActorRef;
use serde::Serialize;
use utoipa::ToSchema;

use crate::AppState;

/// Documentation-only mirror of [`ActorRef`] for the OpenAPI document (rg-core
/// does not depend on utoipa). A test pins its field names to `ActorRef`'s
/// serialized form.
#[derive(Debug, Serialize, ToSchema)]
#[allow(dead_code)]
pub struct ActorRefSchema {
    pub id: i64,
    pub username: String,
    /// `human` or `agent`.
    pub kind: String,
    /// For an agent, the account that owns it; null for a person.
    pub owner_id: Option<i64>,
    /// Null for a person, or when the owner account no longer exists.
    pub owner_username: Option<String>,
}

/// A row plus one extra `<key>: ActorRef | null` field, flattened beside the
/// row's own fields.
#[derive(Debug, Serialize)]
pub struct WithActorRef<T: Serialize> {
    #[serde(flatten)]
    pub row: T,
    #[serde(flatten)]
    pub actor: HashMap<&'static str, Option<ActorRef>>,
}

/// Resolve `ids` (one batch, no per-row query). Failure is logged and yields an
/// empty map, so every ref renders `null`.
pub async fn actor_map(
    state: &AppState,
    ids: impl IntoIterator<Item = i64>,
) -> HashMap<i64, ActorRef> {
    match rg_core::agent::resolve_actors(&state.db, ids).await {
        Ok(map) => map,
        Err(error) => {
            tracing::debug!(%error, "could not resolve actor refs");
            HashMap::new()
        }
    }
}

/// Wrap every row with `key: <the ref of id_of(row)>`, resolving all ids at once.
pub async fn annotate<T: Serialize>(
    state: &AppState,
    rows: Vec<T>,
    key: &'static str,
    id_of: impl Fn(&T) -> i64,
) -> Vec<WithActorRef<T>> {
    let map = actor_map(state, rows.iter().map(&id_of)).await;
    rows.into_iter()
        .map(|row| {
            let actor = map.get(&id_of(&row)).cloned();
            WithActorRef {
                row,
                actor: HashMap::from([(key, actor)]),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_openapi_mirror_has_the_fields_of_the_real_type() {
        let real = serde_json::to_value(ActorRef {
            id: 1,
            username: "x".into(),
            kind: "human",
            owner_id: None,
            owner_username: None,
        })
        .unwrap();
        let mirror = serde_json::to_value(ActorRefSchema {
            id: 1,
            username: "x".into(),
            kind: "human".into(),
            owner_id: None,
            owner_username: None,
        })
        .unwrap();
        let keys = |v: &serde_json::Value| {
            let mut k: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
            k.sort();
            k
        };
        assert_eq!(keys(&real), keys(&mirror));
    }
}
