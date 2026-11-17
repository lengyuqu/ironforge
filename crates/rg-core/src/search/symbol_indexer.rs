//! Symbol definition indexer (#63 step 2, T28).
//!
//! Writes the *declarations* of a commit into `symbol_definitions`. Reference
//! rows are deliberately absent: the design document
//! (`ironforge-docs/architecture/symbol-graph-design-2026-11.md` §2) measured
//! the reference side at 30× the definition volume with 73.9% of it noise from
//! common names.
//!
//! # Reuse, not re-parsing
//!
//! Extraction is [`OutlineIndexer`]'s: this module only flattens its item tree
//! (children included) and persists the result. Rust `impl` rows are written
//! like any other definition even though their name is a first line rather than
//! an identifier — the query side excludes `kind = 'impl'`.
//!
//! # Retention
//!
//! Every successful index prunes in the same transaction. The retained set is
//! the last `D` commits of the default branch (`git rev-list -n D <branch>`),
//! plus the commit that was just indexed so indexing a side branch cannot
//! delete its own rows. `D` follows the design's budget formula
//! [`retention_depth`]: `clamp(128 MiB / bytes_per_commit, 1, 6)` with
//! `bytes_per_commit ≈ 224 B × definitions` (§2.1 measured).
//!
//! # Idempotency
//!
//! Indexing the same commit twice deletes that commit's definitions first, so
//! the row count is stable. The unique anchor
//! `(commit_id, path, name, kind, start_line)` is a second line of defence.
//! The completeness counters are overwritten by the second run for the same
//! reason: they describe the index that is actually stored.
//!
//! # Completeness
//!
//! Per commit the writer records `files_total`, `files_failed` and
//! `files_truncated` (see [`Completeness`]). They exist so the reader can say
//! "this index is complete" with evidence instead of hedging for every commit;
//! rows written before the columns existed keep the `-1` sentinel and stay
//! `partial`.

use std::collections::HashSet;
use std::path::Path;

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter, Statement, TransactionTrait, Value,
};

use crate::error::{CoreError, CoreResult};
use crate::search::indexer::{OutlineIndexer, SearchIndexer};
use crate::search::outline::{Language, OutlineItem, MAX_FILE_BYTES};

/// Maximum number of commits kept per repository, including HEAD.
pub const MAX_RETAINED_COMMITS: usize = 6;

/// Storage budget per repository for the definition table (§3.2 of the design).
pub const STORAGE_BUDGET_BYTES: u64 = 128 * 1024 * 1024;

/// Measured bytes per definition row, including the two indexes (§2.1 variant D).
pub const ESTIMATED_BYTES_PER_DEFINITION: u64 = 224;

/// Definitions written per multi-row `INSERT`. 8 bind values per row keeps the
/// statement under SQLite's historical 999-variable limit.
const INSERT_CHUNK: usize = 100;

/// One row destined for `symbol_definitions` (without the commit columns).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolDefinition {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub start_line: u32,
    pub end_line: u32,
    /// First source line, whitespace-collapsed. `None` when the extractor
    /// produced an empty signature.
    pub signature: Option<String>,
}

/// Result of one [`index_commit`] call.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct IndexReport {
    /// Full 40-hex SHA the requested ref resolved to.
    pub commit_sha: String,
    /// `symbol_commits.id` of the indexed commit.
    pub commit_id: i32,
    pub definitions_written: usize,
    /// `symbol_commits` rows deleted by this call's prune step.
    pub pruned_commits: usize,
}

/// Structured-outline indexer for the symbol table.
///
/// `supports` and `extract` are exactly [`OutlineIndexer`]'s; the artifact is
/// the flattened definition list rather than the nested outline.
#[derive(Debug, Default, Clone, Copy)]
pub struct SymbolIndexer;

impl SearchIndexer for SymbolIndexer {
    type Artifact = Vec<SymbolDefinition>;

    fn name(&self) -> &'static str {
        "symbol"
    }

    fn supports(&self, path: &str) -> bool {
        OutlineIndexer.supports(path)
    }

    fn extract(&self, path: &str, source: &str) -> Self::Artifact {
        let outline = OutlineIndexer.extract(path, source);
        let mut out = Vec::with_capacity(outline.item_count());
        flatten(&outline.items, path, &mut out);
        out
    }
}

/// Append every item at every nesting level, parents before children.
fn flatten(items: &[OutlineItem], path: &str, out: &mut Vec<SymbolDefinition>) {
    for item in items {
        out.push(SymbolDefinition {
            path: path.to_string(),
            name: item.name.clone(),
            kind: item.kind.as_str().to_string(),
            start_line: item.start_line,
            end_line: item.end_line,
            signature: (!item.signature.is_empty()).then(|| item.signature.clone()),
        });
        flatten(&item.children, path, out);
    }
}

/// Retention depth for a commit with `definitions` rows:
/// `clamp(128 MiB / (224 B × definitions), 1, 6)`.
///
/// The formula is the design's (§3.2); the 128 MiB budget and the 224 B row
/// estimate are measured values, not tuning knobs. An empty commit keeps
/// [`MAX_RETAINED_COMMITS`] because it costs nothing.
pub fn retention_depth(definitions: usize) -> usize {
    let bytes = (definitions as u64).saturating_mul(ESTIMATED_BYTES_PER_DEFINITION);
    if bytes == 0 {
        return MAX_RETAINED_COMMITS;
    }
    (STORAGE_BUDGET_BYTES / bytes).clamp(1, MAX_RETAINED_COMMITS as u64) as usize
}

/// What the blocking git pass produces.
struct Snapshot {
    /// Fully resolved 40-hex commit SHA.
    commit_sha: String,
    definitions: Vec<SymbolDefinition>,
    /// Completeness of *this* run, stored on `symbol_commits` so the reader can
    /// distinguish "nothing failed" from "nobody counted".
    stats: Completeness,
    /// Commits the prune step must keep (`rev-list -n D <default branch>` plus
    /// the indexed commit).
    retained: Vec<String>,
}

/// Per-run counts of what the extractor could and could not see.
///
/// These describe the *data*, not the extractor's ability: a file the parser
/// rejected, or one whose item list was capped, makes the index for this commit
/// incomplete in a way that is specific to this commit. The language limits
/// that apply to every commit (svelte markup, macros, dynamic dispatch) are not
/// recorded here — they are not a defect of any particular run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Completeness {
    /// Files read and handed to the extractor.
    pub files_total: i32,
    /// Of those, files whose outline carried a parse error.
    pub files_failed: i32,
    /// Of those, files whose outline hit the item cap.
    pub files_truncated: i32,
}

impl Completeness {
    /// Whether this run could not produce a complete index.
    ///
    /// A negative [`Self::files_total`] is
    /// [`rg_db::entities::symbol_commit::UNKNOWN_FILES_TOTAL`]: the row predates
    /// the counters, so completeness is unknown and must not be reported as
    /// complete. Zero failures on a measured run is the only case that is not
    /// partial.
    pub fn is_partial(&self) -> bool {
        self.files_total < 0 || self.files_failed > 0 || self.files_truncated > 0
    }
}

/// Index `commit_ref` of `repo_id` and prune the retained window, atomically.
///
/// `commit_ref` is any revision `gix` can resolve (SHA, branch, tag); an
/// unresolvable value is [`CoreError::NotFound`] so the HTTP layer answers 404.
/// `default_branch` names the branch whose `rev-list` defines the retained set.
pub async fn index_commit(
    db: &DatabaseConnection,
    repo_path: &Path,
    repo_id: i64,
    commit_ref: &str,
    default_branch: &str,
) -> CoreResult<IndexReport> {
    // `gix::Repository` is `!Send`; keep every git access inside one blocking
    // task and return plain data.
    let repo_path = repo_path.to_path_buf();
    let commit_ref = commit_ref.to_string();
    let default_branch = default_branch.to_string();
    let snapshot = tokio::task::spawn_blocking(move || {
        collect_snapshot(&repo_path, &commit_ref, &default_branch)
    })
    .await
    .map_err(|error| CoreError::internal(format!("symbol index task failed: {error}")))??;

    let txn = db
        .begin()
        .await
        .map_err(|error| CoreError::internal(format!("db: begin symbol index: {error}")))?;

    let commit_id = upsert_commit(&txn, repo_id, &snapshot.commit_sha, snapshot.stats).await?;

    // Idempotency: drop this commit's previous rows before writing the new ones.
    execute(
        &txn,
        "DELETE FROM symbol_definitions WHERE commit_id = ?",
        vec![Value::from(commit_id)],
    )
    .await?;

    let definitions_written =
        insert_definitions(&txn, repo_id, commit_id, &snapshot.definitions).await?;

    let pruned_commits = prune(&txn, repo_id, &snapshot.retained).await?;

    txn.commit()
        .await
        .map_err(|error| CoreError::internal(format!("db: commit symbol index: {error}")))?;

    Ok(IndexReport {
        commit_sha: snapshot.commit_sha,
        commit_id,
        definitions_written,
        pruned_commits,
    })
}

async fn execute<C: ConnectionTrait>(conn: &C, sql: &str, values: Vec<Value>) -> CoreResult<u64> {
    conn.execute(Statement::from_sql_and_values(
        conn.get_database_backend(),
        rg_db::prepare_sql(conn.get_database_backend(), sql),
        values,
    ))
    .await
    .map(|result| result.rows_affected())
    .map_err(|error| CoreError::internal(format!("db: symbol index: {error}")))
}

/// `SELECT id FROM symbol_commits WHERE repo_id = ? AND commit_sha = ?`, then
/// insert when absent. The unique `(repo_id, commit_sha)` key is the backstop.
///
/// An existing row is **updated** with the new counters: re-indexing the same
/// commit replaces the previous measurement instead of adding to it, exactly
/// like its definitions are replaced. A stale `partial` from an older run would
/// otherwise outlive the reason it was set.
async fn upsert_commit<C: ConnectionTrait>(
    conn: &C,
    repo_id: i64,
    commit_sha: &str,
    stats: Completeness,
) -> CoreResult<i32> {
    use rg_db::entities::symbol_commit::{ActiveModel, Column, Entity};

    let existing = Entity::find()
        .filter(Column::RepoId.eq(repo_id))
        .filter(Column::CommitSha.eq(commit_sha))
        .one(conn)
        .await
        .map_err(|error| CoreError::internal(format!("db: read symbol commit: {error}")))?;
    if let Some(model) = existing {
        if model.files_total != stats.files_total
            || model.files_failed != stats.files_failed
            || model.files_truncated != stats.files_truncated
        {
            ActiveModel {
                id: Set(model.id),
                files_total: Set(stats.files_total),
                files_failed: Set(stats.files_failed),
                files_truncated: Set(stats.files_truncated),
                ..Default::default()
            }
            .update(conn)
            .await
            .map_err(|error| {
                CoreError::internal(format!("db: update symbol commit stats: {error}"))
            })?;
        }
        return Ok(model.id);
    }
    ActiveModel {
        repo_id: Set(repo_id),
        commit_sha: Set(commit_sha.to_string()),
        created_at: Set(chrono::Utc::now()),
        files_total: Set(stats.files_total),
        files_failed: Set(stats.files_failed),
        files_truncated: Set(stats.files_truncated),
        ..Default::default()
    }
    .insert(conn)
    .await
    .map(|model| model.id)
    .map_err(|error| CoreError::internal(format!("db: insert symbol commit: {error}")))
}

async fn insert_definitions<C: ConnectionTrait>(
    conn: &C,
    repo_id: i64,
    commit_id: i32,
    definitions: &[SymbolDefinition],
) -> CoreResult<usize> {
    if definitions.is_empty() {
        return Ok(0);
    }
    let mut written = 0usize;
    for chunk in definitions.chunks(INSERT_CHUNK) {
        let mut placeholders = String::with_capacity(chunk.len() * 18);
        let mut params: Vec<Value> = Vec::with_capacity(chunk.len() * 8);
        for (index, definition) in chunk.iter().enumerate() {
            if index > 0 {
                placeholders.push_str(", ");
            }
            placeholders.push_str("(?, ?, ?, ?, ?, ?, ?, ?)");
            params.push(Value::from(repo_id));
            params.push(Value::from(commit_id));
            params.push(Value::from(definition.path.clone()));
            params.push(Value::from(definition.name.clone()));
            params.push(Value::from(definition.kind.clone()));
            params.push(Value::from(definition.start_line as i64));
            params.push(Value::from(definition.end_line as i64));
            params.push(Value::from(definition.signature.clone()));
        }
        let sql = format!(
            "INSERT INTO symbol_definitions \
             (repo_id, commit_id, path, name, kind, start_line, end_line, signature) \
             VALUES {placeholders}"
        );
        execute(conn, &sql, params).await?;
        written += chunk.len();
    }
    Ok(written)
}

/// Delete every `symbol_commits` row of this repository whose SHA is outside
/// `retained`, and its definitions with it.
///
/// Both tables are deleted explicitly (definitions first) instead of relying on
/// `ON DELETE CASCADE`: the constraint is still declared as a backstop, but this
/// path must not depend on the backend having foreign keys enabled.
async fn prune<C: ConnectionTrait>(
    conn: &C,
    repo_id: i64,
    retained: &[String],
) -> CoreResult<usize> {
    use rg_db::entities::symbol_commit::{Column, Entity};

    let rows = Entity::find()
        .filter(Column::RepoId.eq(repo_id))
        .all(conn)
        .await
        .map_err(|error| CoreError::internal(format!("db: read symbol commits: {error}")))?;
    let keep: HashSet<&str> = retained.iter().map(String::as_str).collect();
    let doomed: Vec<i32> = rows
        .into_iter()
        .filter(|row| !keep.contains(row.commit_sha.as_str()))
        .map(|row| row.id)
        .collect();
    if doomed.is_empty() {
        return Ok(0);
    }

    for chunk in doomed.chunks(INSERT_CHUNK) {
        let placeholders = vec!["?"; chunk.len()].join(", ");
        let params: Vec<Value> = chunk.iter().map(|id| Value::from(*id)).collect();
        execute(
            conn,
            &format!("DELETE FROM symbol_definitions WHERE commit_id IN ({placeholders})"),
            params.clone(),
        )
        .await?;
        execute(
            conn,
            &format!("DELETE FROM symbol_commits WHERE id IN ({placeholders})"),
            params,
        )
        .await?;
    }
    Ok(doomed.len())
}

// ── git pass ──────────────────────────────────────────────────────────────

fn collect_snapshot(
    repo_path: &Path,
    commit_ref: &str,
    default_branch: &str,
) -> CoreResult<Snapshot> {
    let repo = gix::open(repo_path).map_err(|error| {
        CoreError::internal(format!(
            "failed to open repository '{}': {error}",
            repo_path.display()
        ))
    })?;
    let commit_id = repo
        .rev_parse_single(commit_ref)
        .map_err(|_| CoreError::not_found(format!("commit '{commit_ref}' not found")))?
        .detach();
    // A tag or tree resolves too; only a commit can be indexed.
    let commit = repo
        .find_commit(commit_id)
        .map_err(|_| CoreError::not_found(format!("commit '{commit_ref}' not found")))?;
    let tree_id = commit
        .tree_id()
        .map_err(|error| CoreError::internal(format!("failed to read commit tree: {error}")))?
        .detach();

    let mut blobs = Vec::new();
    collect_blobs(&repo, tree_id, "", &mut blobs)?;

    let indexer = OutlineIndexer;
    let mut definitions = Vec::new();
    let mut stats = Completeness::default();
    for blob in blobs {
        // Every admitted blob counts as "seen": `files_total` is the
        // denominator of the two failure counters, so a file that disappears
        // here would silently shrink the measurement.
        stats.files_total += 1;
        let Ok(object) = repo.find_object(blob.oid) else {
            stats.files_failed += 1;
            continue;
        };
        let Ok(blob_object) = object.try_into_blob() else {
            stats.files_failed += 1;
            continue;
        };
        let Ok(source) = std::str::from_utf8(&blob_object.data) else {
            // Undecodable source is source the extractor never saw; calling it
            // "indexed" would be a claim of completeness.
            stats.files_failed += 1;
            continue;
        };
        // The outline is taken directly rather than through
        // `SymbolIndexer::extract`, so `error`/`truncated` — the evidence the
        // reader needs — are visible before the item tree is flattened.
        let outline = indexer.extract(&blob.path, source);
        if outline.error.is_some() {
            stats.files_failed += 1;
        }
        if outline.truncated {
            stats.files_truncated += 1;
        }
        flatten(&outline.items, &blob.path, &mut definitions);
    }

    // The unique anchor tolerates no duplicate; collapse identical rows before
    // they reach the database.
    let mut seen = HashSet::new();
    definitions.retain(|definition| {
        seen.insert((
            definition.path.clone(),
            definition.name.clone(),
            definition.kind.clone(),
            definition.start_line,
        ))
    });

    let depth = retention_depth(definitions.len());
    let retained = retained_commits(&repo, default_branch, commit_id, depth);

    Ok(Snapshot {
        commit_sha: commit_id.to_string(),
        definitions,
        stats,
        retained,
    })
}

struct BlobRef {
    path: String,
    oid: gix::ObjectId,
}

/// Depth-first walk admitting exactly what the outline extractor accepts:
/// supported language, at most [`MAX_FILE_BYTES`], blob.
fn collect_blobs(
    repo: &gix::Repository,
    tree_id: gix::ObjectId,
    prefix: &str,
    out: &mut Vec<BlobRef>,
) -> CoreResult<()> {
    let tree = repo.find_tree(tree_id).map_err(|error| {
        CoreError::internal(format!("failed to read tree '{tree_id}': {error}"))
    })?;
    for entry in tree.iter() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let name = entry.filename().to_string();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let oid = entry.oid().to_owned();
        if entry.mode().is_tree() {
            collect_blobs(repo, oid, &path, out)?;
            continue;
        }
        if !entry.mode().is_blob() || !Language::from_path(&path).is_supported() {
            continue;
        }
        // Reject oversized blobs before reading them, exactly like `/map`.
        let size = repo
            .find_header(oid)
            .map(|header| header.size())
            .unwrap_or(u64::MAX);
        if size > MAX_FILE_BYTES as u64 {
            continue;
        }
        out.push(BlobRef { path, oid });
    }
    Ok(())
}

/// `git rev-list -n <depth> <default_branch>` (commit-time order, newest
/// first), with the indexed commit forced in.
///
/// An unresolvable default branch is not an error: the target commit alone is
/// retained, which is the smallest safe set. No `git` process is spawned.
fn retained_commits(
    repo: &gix::Repository,
    default_branch: &str,
    target: gix::ObjectId,
    depth: usize,
) -> Vec<String> {
    use gix::revision::walk::Sorting;
    use gix::traverse::commit::simple::CommitTimeOrder;

    let mut retained: Vec<String> = Vec::with_capacity(depth + 1);
    retained.push(target.to_string());
    if depth <= 1 {
        return retained;
    }
    let Ok(head) = repo.rev_parse_single(default_branch) else {
        return retained;
    };
    let walk = repo
        .rev_walk([head.detach()])
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst));
    let Ok(iter) = walk.all() else {
        return retained;
    };
    for info in iter.take(depth) {
        let Ok(info) = info else { continue };
        let sha = info.id.to_string();
        if !retained.iter().any(|kept| kept == &sha) {
            retained.push(sha);
        }
        if retained.len() >= depth {
            break;
        }
    }
    retained.truncate(depth.max(1));
    retained
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::outline::ItemKind;

    #[test]
    fn flattens_children_and_keeps_impl_rows() {
        let source = "pub struct Outer;\n\
                      impl Outer {\n    pub fn method(&self) {}\n}\n\
                      impl std::fmt::Display for Outer {\n    fn fmt(&self) {}\n}\n";
        let definitions = SymbolIndexer.extract("a.rs", source);
        assert!(
            definitions
                .iter()
                .any(|definition| definition.kind == ItemKind::Impl.as_str()),
            "impl rows must be stored, got {definitions:#?}"
        );
        assert!(definitions
            .iter()
            .any(|definition| definition.name == "method"));
        assert!(
            definitions
                .iter()
                .all(|definition| definition.path == "a.rs"),
            "every row carries the file path"
        );
    }

    #[test]
    fn retention_depth_follows_the_budget_formula() {
        // 0 rows costs nothing; keep the full window.
        assert_eq!(retention_depth(0), MAX_RETAINED_COMMITS);
        // 12,208 definitions × 224 B ≈ 2.67 MiB → 128 MiB / 2.67 MiB = 47 → 6.
        assert_eq!(retention_depth(12_208), 6);
        // 10,000-file projection: 95,900 × 224 B ≈ 20.5 MiB → 6.
        assert_eq!(retention_depth(95_900), 6);
        // 50,000-file projection: 479,000 × 224 B ≈ 102 MiB → 1.
        assert_eq!(retention_depth(479_000), 1);
        // Absurdly large commits clamp at 1, never 0.
        assert_eq!(retention_depth(10_000_000), 1);
    }

    #[test]
    fn unsupported_paths_are_rejected_before_parsing() {
        let indexer = SymbolIndexer;
        assert_eq!(indexer.name(), "symbol");
        assert!(indexer.supports("a.rs"));
        assert!(indexer.supports("a.go"));
        assert!(!indexer.supports("README.md"));
        assert!(indexer.extract("README.md", "# hi").is_empty());
    }
}
