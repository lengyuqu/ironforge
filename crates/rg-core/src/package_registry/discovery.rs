//! Cross-repository discovery of skills and MCP server manifests.
//!
//! One question: *which entries, in repositories this caller may read, match
//! this text?* Two properties matter more than the ranking.
//!
//! **Authorization comes before pagination.** A page is built by walking the
//! candidates in package-id order and keeping only those whose repository the
//! caller may read ([`crate::repo::service::can_read_repo`], the same decision
//! every other read makes, including an agent's owner cap). Nothing is trimmed
//! afterwards, so a page holds `limit` entries the caller may see, not a window
//! over everything with some holes. No total is ever returned, and a hidden
//! entry leaves no trace in the response: it is skipped, not replaced by a
//! placeholder.
//!
//! **A page can be short.** The walk stops after [`MAX_SCAN`] candidates so one
//! request cannot be made to read the whole table. If it stops before filling
//! the page it still returns `next_cursor`; the caller continues from there. The
//! only thing a short page with a `next_cursor` tells a caller is that the scan
//! limit was reached, which is also true when every skipped candidate was
//! public. That is the whole of what this reveals about entries it cannot read:
//! that candidates exist after the cursor, which the same caller learns from
//! the registry holding any entries at all.
//!
//! Results are in publication order of the first version, oldest first. There is
//! no relevance ranking: matching is a case-insensitive substring of the name or
//! description, so the same query gives the same order on every database.
//!
//! Read access is evaluated per request, but `can_read_repo` keeps a 30-second
//! cache for private repositories, so a permission revoked a moment ago can
//! still show an entry for up to that long, exactly as for every other read.
//!
//! Everything a publisher wrote (`description`, the name) is
//! `origin: "user_content"`. An entry that no non-yanked version exists for is
//! not listed: there would be nothing to resolve.

use std::collections::HashMap;

use rg_db::entities::{package, package_version, repository};
use rg_db::ops::registry_discovery_ops::{self, MAX_QUERY_CHARS};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::Serialize;

use super::service::package_types;
use super::version_select;
use super::{mcp_service, skill_service};
use crate::agent::ActorRef;
use crate::error::{CoreError, CoreResult};

/// Entries returned when the caller does not say.
pub const DEFAULT_LIMIT: usize = 20;
/// Most entries one page may hold; more is a 400, not a silent cut.
pub const MAX_LIMIT: usize = 100;
/// Most candidates one request examines.
pub const MAX_SCAN: usize = 200;
/// Candidates read per query.
const BATCH: u64 = 50;

/// What is being discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Skill,
    McpServer,
}

impl Kind {
    fn package_type(self) -> &'static str {
        match self {
            Kind::Skill => package_types::SKILL,
            Kind::McpServer => package_types::MCP_SERVER,
        }
    }
}

pub struct Query<'a> {
    pub kind: Kind,
    /// Substring to look for in name or description.
    pub text: Option<&'a str>,
    pub limit: usize,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<&'a str>,
    /// `None` is an anonymous caller: public repositories only.
    pub actor_id: Option<i64>,
}

/// One entry. Digest fields are the ones a lock file records, so a client can
/// pin straight from a listing; they match what `resolve` returns for the same
/// version.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub owner: String,
    pub repo: String,
    pub name: String,
    pub description: String,
    pub latest_version: String,
    /// Skills: the digest of the unpacked content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    /// Skills: the digest of the downloadable archive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_digest: Option<String>,
    /// MCP servers: the digest of the manifest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// Skills: whether the server built this version from the commit itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_verified: Option<bool>,
    /// MCP servers: always `"unverified"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ownership: Option<&'static str>,
    /// Who published the latest version; `null` if that account is gone. The
    /// `kind` says whether it was a person or an agent.
    pub publisher: Option<ActorRef>,
    pub updated_at: String,
    pub origin: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub entries: Vec<Entry>,
    /// Present when there may be more: pass it back as `cursor`.
    pub next_cursor: Option<String>,
}

fn invalid(code: &'static str, message: impl Into<String>) -> CoreError {
    CoreError::coded(400, code, message)
}

fn parse_cursor(cursor: Option<&str>) -> CoreResult<i64> {
    let Some(cursor) = cursor.filter(|c| !c.is_empty()) else {
        return Ok(0);
    };
    cursor
        .parse::<i64>()
        .ok()
        .filter(|id| *id >= 0)
        .ok_or_else(|| invalid("INVALID_CURSOR", "cursor is not one this endpoint returned"))
}

/// One page of discovered entries. See the module docs for what it promises.
pub async fn discover(db: &DatabaseConnection, query: Query<'_>) -> CoreResult<Page> {
    if query.limit == 0 || query.limit > MAX_LIMIT {
        return Err(invalid(
            "INVALID_LIMIT",
            format!("limit must be between 1 and {MAX_LIMIT}"),
        ));
    }
    let text = query.text.map(str::trim).filter(|t| !t.is_empty());
    if text.is_some_and(|t| t.chars().count() > MAX_QUERY_CHARS) {
        return Err(invalid(
            "INVALID_QUERY",
            format!("q may be at most {MAX_QUERY_CHARS} characters"),
        ));
    }
    let mut after = parse_cursor(query.cursor)?;

    let mut found: Vec<(package::Model, repository::Model, package_version::Model)> = Vec::new();
    let mut scanned = 0usize;
    let mut exhausted = false;
    let mut owners = OwnerNames::default();

    'scan: while found.len() < query.limit && scanned < MAX_SCAN {
        let want = BATCH.min((MAX_SCAN - scanned) as u64);
        let batch =
            registry_discovery_ops::candidates(db, query.kind.package_type(), text, after, want)
                .await
                .map_err(CoreError::internal)?;
        let short = (batch.len() as u64) < want;

        let package_ids: Vec<i64> = batch.iter().map(|c| c.package_id).collect();
        let repo_ids: Vec<i64> = batch.iter().map(|c| c.repo_id).collect();
        let packages: HashMap<i64, package::Model> = package::Entity::find()
            .filter(package::Column::Id.is_in(package_ids))
            .all(db)
            .await
            .map_err(CoreError::internal)?
            .into_iter()
            .map(|p| (p.id, p))
            .collect();
        let repos: HashMap<i64, repository::Model> = repository::Entity::find()
            .filter(repository::Column::Id.is_in(repo_ids))
            .filter(repository::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(CoreError::internal)?
            .into_iter()
            .map(|r| (r.id, r))
            .collect();

        for candidate in &batch {
            // The cursor moves past a candidate whether or not it is shown.
            after = candidate.package_id;
            scanned += 1;

            let (Some(pkg), Some(repo)) = (
                packages.get(&candidate.package_id),
                repos.get(&candidate.repo_id),
            ) else {
                continue;
            };
            if !crate::repo::service::can_read_repo(db, repo, query.actor_id).await? {
                continue;
            }
            let versions = rg_db::ops::package_version_ops::list_by_package(db, pkg.id)
                .await
                .map_err(CoreError::internal)?;
            let Some(latest) = version_select::pick_latest(&versions) else {
                continue;
            };
            // A version whose stored facts cannot be read is not shown: a lock
            // file needs the digest.
            found.push((pkg.clone(), repo.clone(), latest.clone()));
            if found.len() == query.limit {
                // The batch may have been fully consumed with nothing after it.
                exhausted =
                    short && candidate.package_id == batch.last().map_or(0, |c| c.package_id);
                break;
            }
        }
        if found.len() < query.limit && short {
            exhausted = true;
            break 'scan;
        }
    }

    let publishers =
        crate::agent::resolve_actors(db, found.iter().filter_map(|(_, _, v)| v.author_id)).await?;
    let mut entries = Vec::with_capacity(found.len());
    for (pkg, repo, latest) in found {
        let (owner, repo_name) = owners.names(db, &repo).await?;
        let (description, content_digest, archive_digest, digest, source_verified, ownership) =
            match query.kind {
                Kind::Skill => {
                    let Some(view) = skill_service::discovery_view(&latest) else {
                        continue;
                    };
                    (
                        view.description,
                        Some(view.content_digest),
                        Some(view.archive_digest),
                        None,
                        Some(view.source_verified),
                        None,
                    )
                }
                Kind::McpServer => {
                    let Some(digest) = mcp_service::discovery_digest(&latest) else {
                        continue;
                    };
                    (
                        mcp_service::discovery_description(&pkg, &latest),
                        None,
                        None,
                        Some(digest),
                        None,
                        Some("unverified"),
                    )
                }
            };
        entries.push(Entry {
            owner,
            repo: repo_name,
            name: pkg.name.clone(),
            description,
            latest_version: latest.version.clone(),
            content_digest,
            archive_digest,
            digest,
            source_verified,
            ownership,
            publisher: latest.author_id.and_then(|id| publishers.get(&id).cloned()),
            updated_at: latest.created_at.to_rfc3339(),
            origin: "user_content",
        });
    }

    Ok(Page {
        entries,
        next_cursor: (!exhausted).then(|| after.to_string()),
    })
}

/// Owner logins for the repositories of one request, looked up once each.
#[derive(Default)]
struct OwnerNames {
    users: HashMap<i64, String>,
    orgs: HashMap<i64, String>,
}

impl OwnerNames {
    /// `(owner, repository name)` as used in `/repos/{owner}/{name}`.
    async fn names(
        &mut self,
        db: &DatabaseConnection,
        repo: &repository::Model,
    ) -> CoreResult<(String, String)> {
        let owner = if let Some(org_id) = repo.org_id {
            if let Some(name) = self.orgs.get(&org_id) {
                name.clone()
            } else {
                let name = rg_db::ops::org_ops::get_org(db, org_id)
                    .await
                    .map_err(CoreError::internal)?
                    .map(|o| o.name)
                    .unwrap_or_default();
                self.orgs.insert(org_id, name.clone());
                name
            }
        } else if let Some(name) = self.users.get(&repo.owner_id) {
            name.clone()
        } else {
            let name = rg_db::ops::user_ops::find_by_ids(db, &[repo.owner_id])
                .await
                .map_err(CoreError::internal)?
                .into_iter()
                .next()
                .map(|u| u.username)
                .unwrap_or_default();
            self.users.insert(repo.owner_id, name.clone());
            name
        };
        Ok((owner, repo.name.clone()))
    }
}
