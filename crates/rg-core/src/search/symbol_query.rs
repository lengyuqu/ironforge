//! Symbol definition queries (#63 step 3, T29).
//!
//! Reads [`crate::search::symbol_indexer`] rows back. The indexer and this
//! module are intentionally asymmetric:
//!
//! * the writer stores every declaration, including Rust `impl` rows whose
//!   `name` is a first line rather than an identifier (`ActiveModelBehavior for
//!   ActiveModel`; 4.1% of rows, measured in
//!   `ironforge-docs/architecture/symbol-graph-design-2026-11.md` §1.5);
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
    /// The index for this commit is known to be incomplete.
    pub partial: bool,
    /// What static parsing cannot see, in plain sentences. Never empty.
    pub limitations: Vec<String>,
    pub items: Vec<SymbolHit>,
}

/// What a static tree-sitter index cannot answer, stated as known facts.
///
/// These are static truths about the extractor, not measurements of a
/// particular commit: the indexer does not record which files failed to parse
/// or which languages a commit contains, so per-commit numbers cannot be given
/// here and are not invented.
const LIMITATIONS: &[&str] = &[
    "svelte files are parsed only inside <script>: markup, props and template bindings are not in the symbol table",
    "parse failures are not recorded per commit: a file the parser rejected contributes whatever the partial tree yielded, or nothing, and this response cannot say which",
    "Rust macro-generated symbols and dyn Trait dispatch are not resolvable by tree-sitter",
    "TypeScript/JavaScript dynamic property access (obj[key]) and `any` propagation are not resolvable",
    "Python getattr(obj, \"x\") and monkey patching are not resolvable",
    "Go interface method sets are decided at run time: interface calls are not resolvable",
];

/// Why every page is `partial`.
///
/// Static extraction is incomplete for every supported language (macro
/// expansion, dynamic dispatch, reflection), and the index does not record
/// parse failures, so there is no commit for which completeness can be proven.
/// Claiming `partial: false` would require evidence the writer never kept;
/// `partial: true` with named [`LIMITATIONS`] is the honest answer.
const PARTIAL: bool = true;

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

    let commit_id = commit_id_of(db, repo_id, commit_sha).await?;

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
        params.push(Value::from(query.chars().count() as i64));
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
            start_line: integer(&row, 3)? as i32,
            end_line: integer(&row, 4)? as i32,
            signature: optional_text(&row, 5)?,
            confidence,
        });
    }

    Ok(SymbolPage {
        commit_sha: commit_sha.to_string(),
        partial: PARTIAL,
        limitations: LIMITATIONS.iter().map(|line| line.to_string()).collect(),
        items,
    })
}

/// `symbol_commits.id` for this exact commit, or `SYMBOLS_NOT_INDEXED`.
async fn commit_id_of(db: &DatabaseConnection, repo_id: i64, commit_sha: &str) -> CoreResult<i32> {
    let backend = db.get_database_backend();
    let row = db
        .query_one(Statement::from_sql_and_values(
            backend,
            rg_db::prepare_sql(
                backend,
                "SELECT id FROM symbol_commits WHERE repo_id = ? AND commit_sha = ?",
            ),
            [Value::from(repo_id), Value::from(commit_sha)],
        ))
        .await
        .map_err(|error| CoreError::internal(format!("db: read symbol commit: {error}")))?;

    match row {
        Some(row) => Ok(integer(&row, 0)? as i32),
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

fn integer(row: &sea_orm::QueryResult, index: usize) -> CoreResult<i64> {
    row.try_get_by_index::<i64>(index)
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

    #[tokio::test]
    async fn limitations_are_always_stated_and_partial_is_honest() {
        let db = memory_db().await;
        seed(&db).await;

        let page = find_symbols(&db, REPO, SHA, None, Some("a.rs"), 50)
            .await
            .unwrap();
        assert!(page.partial);
        assert!(page.limitations.iter().any(|line| line.contains("svelte")));
        assert!(page
            .limitations
            .iter()
            .any(|line| line.contains("Rust macro")));
        assert!(page
            .limitations
            .iter()
            .any(|line| line.contains("Go interface")));
    }
}
