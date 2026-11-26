//! Symbol definition queries (#63 step 3, T29).
//!
//! Reads [`crate::search::symbol_indexer`] rows back. The indexer and this
//! module are intentionally asymmetric:
//!
//! * the writer stores every declaration, including Rust `impl` rows whose
//!   `name` is a first line rather than an identifier (`ActiveModelBehavior for
//!   ActiveModel`; 4.1% of rows, measured in
//!   `docs/design/code-intelligence.md` §2);
//! * the reader excludes `kind = 'impl'` **unconditionally**. Those rows are not
//!   identifiers, so a name query can never legitimately mean them, and
//!   returning them makes a search for `ActiveModel` answer with 106
//!   same-named different-target pseudo-symbols.
//!
//! # Confidence
//!
//! [`Confidence::Exact`] means the row *is* the declaration the caller asked
//! for: `name` equals the query verbatim, or the caller asked by `path` and the
//! row is that file's own declaration. [`Confidence::NameMatch`] means only the
//! prefix matched (`get_` finds `get_user`) — the name is the same up to a
//! suffix, which is not the same statement as "this is your symbol".
//!
//! Confidence says nothing about completeness; that is
//! [`SymbolPage::partial`] and [`SymbolPage::limitations`].
//!
//! # Never fall back to another commit
//!
//! [`find_symbols`] looks up the exact `(repo_id, commit_sha)` pair and answers
//! [`CoreError::Coded`] `SYMBOLS_NOT_INDEXED` when it is absent. It does **not**
//! retry with the default branch HEAD. The caller asked what the definition is
//! *at that commit*; answering from a different commit returns a wrong answer
//! shaped like a right one, which is worse than an explicit failure — a reader
//! cannot tell it apart from a correct result. There is deliberately no
//! fallback parameter.
//!
//! # Limits
//!
//! `limit` is a SQL `LIMIT`, not a truncation after the fact: a caller must be
//! able to trust that it received the first `limit` rows in the documented
//! order, and a large `limit` must not make the server materialise the whole
//! commit.

use sea_orm::{ConnectionTrait, DatabaseConnection, Statement, Value};

use crate::error::{CoreError, CoreResult};
use crate::search::symbol_indexer::Completeness;

/// Rows returned when the caller does not ask for a specific `limit`.
pub const DEFAULT_LIMIT: usize = 50;

/// Largest `limit` a caller may ask for. Anything above is refused instead of
/// silently clamped, so a caller never mistakes a page for the whole answer.
pub const MAX_LIMIT: usize = 200;

/// Stable machine-readable code for "this commit has no symbol index".
pub const NOT_INDEXED_CODE: &str = "SYMBOLS_NOT_INDEXED";

/// How sure the row is the thing the caller asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// The name equals the query verbatim, or the query was by path.
    Exact,
    /// Only the beginning of the name matched.
    NameMatch,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::NameMatch => "name_match",
        }
    }
}

/// One definition row as returned to the API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolHit {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub start_line: i32,
    pub end_line: i32,
    /// First source line, whitespace-collapsed. `None` when the extractor
    /// produced an empty signature (the indexer stores NULL then).
    pub signature: Option<String>,
    pub confidence: Confidence,
}

/// A page of definitions for one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolPage {
    /// The full 40-hex SHA that was queried.
    pub commit_sha: String,
    /// This commit's index is incomplete, or its completeness was never
    /// measured. False means the indexing run recorded zero failed and zero
    /// truncated files — it does **not** mean static extraction sees
    /// everything; that boundary is [`Self::limitations`].
    pub partial: bool,
    /// What static parsing cannot see, plus this commit's measured defects, in
    /// plain sentences. Never empty.
    pub limitations: Vec<String>,
    pub items: Vec<SymbolHit>,
}

/// What a static tree-sitter index cannot answer, stated as known facts.
///
/// These are static truths about the extractor and hold for **every** commit:
/// they describe the *capability boundary*, not the quality of one indexing
/// run. They are therefore always listed, and they never make a page
/// [`SymbolPage::partial`] — a svelte-heavy repository is not "incompletely
/// indexed", it is indexed by an extractor whose reach has a known edge.
///
/// Defects that belong to one commit (files the parser rejected, files whose
/// item list was capped, rows written before the counters existed) are read
/// from `symbol_commits` and appended by [`measured_limitations`].
const STATIC_LIMITATIONS: &[&str] = &[
    "svelte files are parsed only inside <script>: markup, props and template bindings are not in the symbol table",
    "Rust macro-generated symbols and dyn Trait dispatch are not resolvable by tree-sitter",
    "TypeScript/JavaScript dynamic property access (obj[key]) and `any` propagation are not resolvable",
    "Python getattr(obj, \"x\") and monkey patching are not resolvable",
    "Go interface method sets are decided at run time: interface calls are not resolvable",
];

/// Append the sentences that describe *this commit's* measured defects.
///
/// Empty for a measured, clean run: there is nothing to disclose beyond
/// [`STATIC_LIMITATIONS`]. The counts are the reason `partial` is true, so the
/// two always agree.
fn measured_limitations(stats: Completeness) -> Vec<String> {
    let mut lines = Vec::new();
    if stats.files_total < 0 {
        lines.push(
            "this commit was indexed before completeness stats were recorded; \
             completeness is unknown"
                .to_string(),
        );
    }
    if stats.files_failed > 0 {
        lines.push(format!(
            "{} files failed to parse and are only partially indexed",
            stats.files_failed
        ));
    }
    if stats.files_truncated > 0 {
        lines.push(format!(
            "{} files exceeded the item cap and were truncated",
            stats.files_truncated
        ));
    }
    lines
}

/// Read definitions of `commit_sha` by `name` and/or `path`.
///
/// At least one of `name` and `path` must be non-empty; both empty is
/// [`CoreError::InvalidInput`] (HTTP 400). When both are given they are ANDed.
///
/// Ordering: with `name`, exact-name rows come first, then prefix matches, each
/// group by `name, path, start_line`; with `path` only, by `path, start_line`.
/// The order is stable so `limit` pages are meaningful.
pub async fn find_symbols(
    db: &DatabaseConnection,
    repo_id: i64,
    commit_sha: &str,
    name: Option<&str>,
    path: Option<&str>,
    limit: usize,
) -> CoreResult<SymbolPage> {
    let name = name.map(str::trim).filter(|value| !value.is_empty());
    let path = path.map(str::trim).filter(|value| !value.is_empty());
    if name.is_none() && path.is_none() {
        return Err(CoreError::invalid_input(
            "one of 'name' or 'path' is required",
        ));
    }
    if limit == 0 || limit > MAX_LIMIT {
        return Err(CoreError::coded(
            400,
            "INVALID_LIMIT",
            format!("limit must be between 1 and {MAX_LIMIT}"),
        ));
    }

    let commit = commit_stats_of(db, repo_id, commit_sha).await?;
    let commit_id = commit.id;

    let backend = db.get_database_backend();
    let mut sql = String::from(
        "SELECT name, kind, path, start_line, end_line, signature \
         FROM symbol_definitions \
         WHERE repo_id = ? AND commit_id = ? AND kind <> 'impl'",
    );
    let mut params: Vec<Value> = vec![Value::from(repo_id), Value::from(commit_id)];

    if let Some(query) = name {
        // `substr(name, 1, n) = query` instead of `LIKE ?%`: the same thing
        // without a metacharacter escape, which differs between SQLite,
        // PostgreSQL and MySQL (`_` and `%` in a symbol name are literals).
        sql.push_str(" AND (name = ? OR substr(name, 1, ?) = ?)");
        params.push(Value::from(query));
        // `i32`: PostgreSQL resolves `substr(character varying, integer,
        // bigint)` to no function at all, so the length must be bound at
        // `INT4` width there.
        params.push(Value::from(query.chars().count() as i32));
        params.push(Value::from(query));
    }
    if let Some(query) = path {
        sql.push_str(" AND path = ?");
        params.push(Value::from(query));
    }

    match name {
        // Exact first, then prefix hits; deterministic within each group.
        Some(query) => {
            sql.push_str(" ORDER BY CASE WHEN name = ? THEN 0 ELSE 1 END, name, path, start_line");
            params.push(Value::from(query));
        }
        None => sql.push_str(" ORDER BY path, start_line"),
    }
    sql.push_str(" LIMIT ?");
    params.push(Value::from(limit as i64));

    let rows = db
        .query_all(Statement::from_sql_and_values(
            backend,
            rg_db::prepare_sql(backend, &sql),
            params,
        ))
        .await
        .map_err(|error| CoreError::internal(format!("db: query symbol definitions: {error}")))?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let row_name: String = text(&row, 0)?;
        let confidence = match name {
            Some(query) if row_name == query => Confidence::Exact,
            Some(_) => Confidence::NameMatch,
            // A path query asks for that file's declarations; each row is one.
            None => Confidence::Exact,
        };
        items.push(SymbolHit {
            name: row_name,
            kind: text(&row, 1)?,
            path: text(&row, 2)?,
            start_line: integer(&row, 3)?,
            end_line: integer(&row, 4)?,
            signature: optional_text(&row, 5)?,
            confidence,
        });
    }

    let mut limitations: Vec<String> = STATIC_LIMITATIONS
        .iter()
        .map(|line| (*line).to_string())
        .collect();
    limitations.extend(measured_limitations(commit.completeness));

    Ok(SymbolPage {
        commit_sha: commit_sha.to_string(),
        partial: commit.completeness.is_partial(),
        limitations,
        items,
    })
}

/// One `symbol_commits` row as the reader needs it.
struct CommitStats {
    id: i32,
    completeness: Completeness,
}

/// The `symbol_commits` row for this exact commit, or `SYMBOLS_NOT_INDEXED`.
///
/// The counters are read in the same statement as the id: `partial` must
/// describe the row whose definitions are being returned, and a second query
/// could disagree with the first if the commit were re-indexed in between.
async fn commit_stats_of(
    db: &DatabaseConnection,
    repo_id: i64,
    commit_sha: &str,
) -> CoreResult<CommitStats> {
    let backend = db.get_database_backend();
    let row = db
        .query_one(Statement::from_sql_and_values(
            backend,
            rg_db::prepare_sql(
                backend,
                "SELECT id, files_total, files_failed, files_truncated \
                 FROM symbol_commits WHERE repo_id = ? AND commit_sha = ?",
            ),
            [Value::from(repo_id), Value::from(commit_sha)],
        ))
        .await
        .map_err(|error| CoreError::internal(format!("db: read symbol commit: {error}")))?;

    match row {
        Some(row) => Ok(CommitStats {
            id: integer(&row, 0)?,
            completeness: Completeness {
                files_total: integer(&row, 1)?,
                files_failed: integer(&row, 2)?,
                files_truncated: integer(&row, 3)?,
            },
        }),
        // Deliberately no fallback to the default branch HEAD: see the module
        // documentation. The message carries the commit that was asked for, so
        // the caller can index exactly that commit and retry.
        None => Err(CoreError::coded(
            404,
            NOT_INDEXED_CODE,
            format!("commit {commit_sha} has no symbol index in this repository"),
        )),
    }
}

fn text(row: &sea_orm::QueryResult, index: usize) -> CoreResult<String> {
    row.try_get_by_index::<String>(index)
        .map_err(|error| CoreError::internal(format!("db: decode symbol row: {error}")))
}

fn optional_text(row: &sea_orm::QueryResult, index: usize) -> CoreResult<Option<String>> {
    row.try_get_by_index::<Option<String>>(index)
        .map_err(|error| CoreError::internal(format!("db: decode symbol row: {error}")))
}

/// Decode an `INTEGER` column.
///
/// `i32`, not `i64`: PostgreSQL maps the schema's `INTEGER` to `INT4`, and
/// asking sqlx for an `INT8` fails with `ColumnDecode` on that backend (MySQL
/// and SQLite accept both). Every column read through this helper is declared
/// `INTEGER`, so `i32` is the exact width.
fn integer(row: &sea_orm::QueryResult, index: usize) -> CoreResult<i32> {
    row.try_get_by_index::<i32>(index)
        .map_err(|error| CoreError::internal(format!("db: decode symbol row: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::DatabaseConnection;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const OTHER_SHA: &str = "fedcba9876543210fedcba9876543210fedcba98";
    const REPO: i64 = 7;

    async fn memory_db() -> DatabaseConnection {
        let db = rg_db::connect("sqlite::memory:").await.unwrap();
        rg_db::run_migrations(&db).await.unwrap();
        db
    }

    async fn seed_repository(db: &DatabaseConnection, id: i64) {
        use sea_orm::ActiveModelTrait;
        use sea_orm::ActiveValue::Set;

        let now = chrono::Utc::now();
        let owner = rg_db::entities::user::ActiveModel {
            username: Set(format!("sym-owner-{id}")),
            email: Set(format!("sym-{id}@example.com")),
            password_hash: Set("unused".to_string()),
            display_name: Set(Some("sym".to_string())),
            is_admin: Set(false),
            is_active: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert user");
        rg_db::entities::repository::ActiveModel {
            id: Set(id),
            owner_id: Set(owner.id),
            name: Set(format!("sym-{id}")),
            default_branch: Set("main".to_string()),
            is_private: Set(false),
            stars_count: Set(0),
            forks_count: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert repository");
    }

    async fn seed(db: &DatabaseConnection) {
        let backend = db.get_database_backend();
        // `symbol_commits.repo_id` references `repositories.id`, so the parent
        // rows must exist; nothing else about a repository matters here.
        seed_repository(db, REPO).await;
        db.execute(Statement::from_sql_and_values(
            backend,
            "INSERT INTO symbol_commits (id, repo_id, commit_sha, created_at) VALUES (?, ?, ?, ?)",
            [
                Value::from(1_i32),
                Value::from(REPO),
                Value::from(SHA),
                Value::from("2026-11-18T00:00:00Z"),
            ],
        ))
        .await
        .unwrap();
        let rows = [
            ("index_commit", "function", "a.rs", 10, 20),
            ("index_repository", "function", "a.rs", 30, 40),
            ("index_of", "function", "b.rs", 5, 9),
            // Impl rows carry a first line as their name; the reader must drop them.
            ("ActiveModelBehavior for ActiveModel", "impl", "c.rs", 1, 8),
            ("index_like", "const", "c.rs", 12, 12),
        ];
        for (name, kind, path, start, end) in rows {
            db.execute(Statement::from_sql_and_values(
                backend,
                "INSERT INTO symbol_definitions \
                 (repo_id, commit_id, path, name, kind, start_line, end_line, signature) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                [
                    Value::from(REPO),
                    Value::from(1_i32),
                    Value::from(path),
                    Value::from(name),
                    Value::from(kind),
                    Value::from(start as i64),
                    Value::from(end as i64),
                    Value::from(Some(format!("sig {name}"))),
                ],
            ))
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn exact_hits_come_first_and_are_marked_exact() {
        let db = memory_db().await;
        seed(&db).await;

        let page = find_symbols(&db, REPO, SHA, Some("index_commit"), None, 50)
            .await
            .unwrap();
        assert_eq!(page.commit_sha, SHA);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].name, "index_commit");
        assert_eq!(page.items[0].confidence, Confidence::Exact);
        assert_eq!(page.items[0].path, "a.rs");
        assert_eq!(page.items[0].signature.as_deref(), Some("sig index_commit"));

        let page = find_symbols(&db, REPO, SHA, Some("index_"), None, 50)
            .await
            .unwrap();
        assert!(page.items.len() >= 2, "{:#?}", page.items);
        assert!(
            page.items
                .iter()
                .all(|item| item.confidence == Confidence::NameMatch),
            "a prefix that is nobody's whole name must not be exact: {:#?}",
            page.items
        );
        assert!(page
            .items
            .iter()
            .all(|item| item.name.starts_with("index_")));
    }

    #[tokio::test]
    async fn impl_rows_are_never_returned() {
        let db = memory_db().await;
        seed(&db).await;

        let page = find_symbols(
            &db,
            REPO,
            SHA,
            Some("ActiveModelBehavior for ActiveModel"),
            None,
            50,
        )
        .await
        .unwrap();
        assert!(page.items.is_empty(), "{:#?}", page.items);

        // Prefix search for the same text finds nothing either.
        let page = find_symbols(&db, REPO, SHA, Some("ActiveModel"), None, 50)
            .await
            .unwrap();
        assert!(page.items.is_empty(), "{:#?}", page.items);

        // A path listing of a file with an impl row leaves it out.
        let page = find_symbols(&db, REPO, SHA, None, Some("c.rs"), 50)
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].name, "index_like");
        assert!(page.items.iter().all(|item| item.kind != "impl"));
    }

    #[tokio::test]
    async fn path_queries_return_the_files_declarations_in_order() {
        let db = memory_db().await;
        seed(&db).await;

        let page = find_symbols(&db, REPO, SHA, None, Some("a.rs"), 50)
            .await
            .unwrap();
        assert_eq!(
            page.items
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["index_commit", "index_repository"]
        );
        assert!(page
            .items
            .iter()
            .all(|item| item.confidence == Confidence::Exact));
    }

    #[tokio::test]
    async fn limit_is_applied_and_out_of_range_is_refused() {
        let db = memory_db().await;
        seed(&db).await;

        let page = find_symbols(&db, REPO, SHA, Some("index_"), None, 2)
            .await
            .unwrap();
        assert_eq!(page.items.len(), 2);

        for bad in [0, MAX_LIMIT + 1] {
            let error = find_symbols(&db, REPO, SHA, Some("index_"), None, bad)
                .await
                .unwrap_err();
            assert!(
                matches!(
                    &error,
                    CoreError::Coded {
                        status: 400,
                        code: "INVALID_LIMIT",
                        ..
                    }
                ),
                "{error:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_unindexed_commit_is_reported_not_answered_from_another_commit() {
        let db = memory_db().await;
        seed(&db).await;

        let error = find_symbols(&db, REPO, OTHER_SHA, Some("index_commit"), None, 50)
            .await
            .unwrap_err();
        match error {
            CoreError::Coded {
                status,
                code,
                message,
            } => {
                assert_eq!(status, 404);
                assert_eq!(code, NOT_INDEXED_CODE);
                assert!(message.contains(OTHER_SHA), "{message}");
            }
            other => panic!("expected a coded 404, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn name_and_path_are_both_required_to_be_absent_by_accident() {
        let db = memory_db().await;
        seed(&db).await;

        let error = find_symbols(&db, REPO, SHA, None, None, 50)
            .await
            .unwrap_err();
        assert!(matches!(error, CoreError::InvalidInput(_)), "{error:?}");
        let error = find_symbols(&db, REPO, SHA, Some("  "), Some(""), 50)
            .await
            .unwrap_err();
        assert!(matches!(error, CoreError::InvalidInput(_)), "{error:?}");
    }

    /// Overwrite the counters of the seeded commit row (id 1).
    async fn set_stats(db: &DatabaseConnection, total: i32, failed: i32, truncated: i32) {
        db.execute(Statement::from_sql_and_values(
            db.get_database_backend(),
            "UPDATE symbol_commits SET files_total = ?, files_failed = ?, \
             files_truncated = ? WHERE id = 1",
            [
                Value::from(total),
                Value::from(failed),
                Value::from(truncated),
            ],
        ))
        .await
        .unwrap();
    }

    fn joined(page: &SymbolPage) -> String {
        page.limitations.join(" | ")
    }

    /// The static sentences are always present — they describe the extractor,
    /// not this commit — but a measured clean run is not `partial`.
    #[tokio::test]
    async fn a_measured_clean_commit_is_not_partial_and_still_states_the_static_limits() {
        let db = memory_db().await;
        seed(&db).await;
        set_stats(&db, 12, 0, 0).await;

        let page = find_symbols(&db, REPO, SHA, None, Some("a.rs"), 50)
            .await
            .unwrap();
        assert!(!page.partial, "{:#?}", page.limitations);
        let joined = joined(&page);
        for expected in [
            "svelte",
            "Rust macro",
            "TypeScript",
            "Python",
            "Go interface",
        ] {
            assert!(joined.contains(expected), "{expected} missing: {joined}");
        }
        assert!(
            !joined.contains("failed to parse"),
            "a clean run must not invent failures: {joined}"
        );
        assert!(!joined.contains("unknown"), "{joined}");
    }

    /// Failures recorded by the writer make the page partial, with the number
    /// the caller needs to judge what is missing.
    #[tokio::test]
    async fn recorded_parse_failures_make_the_page_partial_with_the_count() {
        let db = memory_db().await;
        seed(&db).await;
        set_stats(&db, 16, 3, 0).await;

        let page = find_symbols(&db, REPO, SHA, None, Some("a.rs"), 50)
            .await
            .unwrap();
        assert!(page.partial);
        assert!(
            joined(&page).contains("3 files failed to parse and are only partially indexed"),
            "{}",
            joined(&page)
        );
        assert!(
            joined(&page).contains("svelte"),
            "static limits stay listed next to the measured defect: {}",
            joined(&page)
        );
        assert!(!joined(&page).contains("exceeded the item cap"));
    }

    #[tokio::test]
    async fn recorded_truncation_makes_the_page_partial_with_the_count() {
        let db = memory_db().await;
        seed(&db).await;
        set_stats(&db, 5, 0, 2).await;

        let page = find_symbols(&db, REPO, SHA, None, Some("a.rs"), 50)
            .await
            .unwrap();
        assert!(page.partial);
        assert!(
            joined(&page).contains("2 files exceeded the item cap and were truncated"),
            "{}",
            joined(&page)
        );
        assert!(!joined(&page).contains("failed to parse"));
    }

    /// A row written before the counters existed cannot prove anything, so it
    /// must stay partial and say why.
    #[tokio::test]
    async fn a_row_without_stats_is_partial_and_says_completeness_is_unknown() {
        use rg_db::entities::symbol_commit::UNKNOWN_FILES_TOTAL;

        let db = memory_db().await;
        seed(&db).await;
        set_stats(&db, UNKNOWN_FILES_TOTAL, 0, 0).await;

        let page = find_symbols(&db, REPO, SHA, None, Some("a.rs"), 50)
            .await
            .unwrap();
        assert!(page.partial);
        assert!(
            joined(&page).contains(
                "this commit was indexed before completeness stats were recorded; \
                 completeness is unknown"
            ),
            "{}",
            joined(&page)
        );
        // Unknown is not the same as "we know files failed": no invented count.
        assert!(
            !joined(&page).contains("failed to parse"),
            "{}",
            joined(&page)
        );
    }
}
