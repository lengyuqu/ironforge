//! T28: symbol definition table — write path, idempotency and retention.
//!
//! These are integration tests because they need a real git repository (gix
//! reads committed trees, not the worktree) and a migrated database. Every git
//! call goes through `rg_git::cli_gateway`, the sanctioned test gateway; tests
//! are not allowed to spawn `git` directly.

use std::path::{Path, PathBuf};

use rg_core::search::symbol_indexer::{index_commit, retention_depth, MAX_RETAINED_COMMITS};
use rg_db::entities::{repository, user};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseConnection, Statement};
use tempfile::TempDir;

// ── fixtures ──────────────────────────────────────────────────────────────

fn git(args: &[&str], dir: Option<&Path>) -> String {
    let gateway = rg_git::cli_gateway::global_gateway()
        .as_ref()
        .expect("global git gateway");
    let output = gateway.run(args, dir).expect("run git");
    output.ensure_success().expect("git command failed");
    output.stdout_str().trim().to_string()
}

/// A committed git repository with `main` checked out.
struct TestRepo {
    dir: TempDir,
}

impl TestRepo {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_string_lossy().to_string();
        git(&["init", "--initial-branch=main", &path], None);
        git(
            &["config", "user.email", "test@example.com"],
            Some(dir.path()),
        );
        git(&["config", "user.name", "Test"], Some(dir.path()));
        Self { dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Write `files` and commit; returns the new commit SHA.
    fn commit(&self, files: &[(&str, &str)], message: &str) -> String {
        for (name, contents) in files {
            let path = self.dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create parent");
            }
            std::fs::write(path, contents).expect("write file");
        }
        git(&["add", "-A"], Some(self.dir.path()));
        git(&["commit", "-m", message], Some(self.dir.path()));
        git(&["rev-parse", "HEAD"], Some(self.dir.path()))
    }
}

/// A migrated SQLite database with one repository row (`main`).
async fn seed_database() -> (DatabaseConnection, TempDir, i64) {
    let dir = tempfile::tempdir().expect("db tempdir");
    let path = dir.path().join("symbols.db");
    let db = rg_db::connect(&format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .expect("connect");
    rg_db::run_migrations(&db).await.expect("migrations");

    let now = chrono::Utc::now();
    let owner = user::ActiveModel {
        username: Set("symbol-owner".to_string()),
        email: Set("symbol@example.com".to_string()),
        password_hash: Set("unused".to_string()),
        display_name: Set(Some("symbol".to_string())),
        is_admin: Set(false),
        is_active: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("insert user");
    let repo = repository::ActiveModel {
        owner_id: Set(owner.id),
        name: Set("symbols".to_string()),
        default_branch: Set("main".to_string()),
        is_private: Set(false),
        stars_count: Set(0),
        forks_count: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&db)
    .await
    .expect("insert repository");

    (db, dir, repo.id)
}

async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
    let row = db
        .query_one(Statement::from_string(db.get_database_backend(), sql))
        .await
        .expect("query")
        .expect("row");
    row.try_get_by_index::<i64>(0).expect("count")
}

/// `(files_total, files_failed, files_truncated)` of the only commit row.
async fn stats(db: &DatabaseConnection) -> (i64, i64, i64) {
    let row = db
        .query_one(Statement::from_string(
            db.get_database_backend(),
            "SELECT files_total, files_failed, files_truncated FROM symbol_commits".to_string(),
        ))
        .await
        .expect("query")
        .expect("row");
    (
        row.try_get_by_index::<i64>(0).expect("files_total"),
        row.try_get_by_index::<i64>(1).expect("files_failed"),
        row.try_get_by_index::<i64>(2).expect("files_truncated"),
    )
}

async fn page_for(
    db: &DatabaseConnection,
    repo_id: i64,
    sha: &str,
    name: &str,
) -> rg_core::search::symbol_query::SymbolPage {
    rg_core::search::symbol_query::find_symbols(db, repo_id, sha, Some(name), None, 50)
        .await
        .expect("find symbols")
}

// ── tests ─────────────────────────────────────────────────────────────────

/// A commit every supported file parses cleanly: the writer records
/// `(total, 0, 0)` and the reader answers `partial: false`.
///
/// The static language limits are still listed — they describe the extractor,
/// not this commit — so "not partial" is not a claim that tree-sitter sees
/// everything.
#[tokio::test]
async fn a_clean_commit_is_recorded_as_complete_and_reads_back_not_partial() {
    let (db, _db_dir, repo_id) = seed_database().await;
    let repo = TestRepo::new();
    let sha = repo.commit(
        &[
            ("src/a.rs", "pub fn clean_one() {}\n"),
            ("src/b.go", "package main\n\nfunc CleanTwo() {}\n"),
            // Unsupported extension: never admitted, so it is not a failure
            // and not part of the denominator either.
            ("README.md", "# ignored\n"),
        ],
        "clean",
    );

    let report = index_commit(&db, repo.path(), repo_id, &sha, "main")
        .await
        .expect("index");
    assert_eq!(report.definitions_written, 2);
    assert_eq!(stats(&db).await, (2, 0, 0));

    let page = page_for(&db, repo_id, &sha, "clean_one").await;
    assert!(!page.partial, "{:#?}", page.limitations);
    let joined = page.limitations.join(" | ");
    assert!(joined.contains("svelte"), "{joined}");
    assert!(joined.contains("Rust macro"), "{joined}");
    assert!(!joined.contains("failed to parse"), "{joined}");
    assert!(!joined.contains("unknown"), "{joined}");
}

/// Files the parser rejects are counted, the page is `partial` for a reason,
/// and re-indexing the same commit replaces the counters instead of adding a
/// second run's files on top.
#[tokio::test]
async fn parse_failures_are_counted_and_reindexing_overwrites_the_counters() {
    let (db, _db_dir, repo_id) = seed_database().await;
    let repo = TestRepo::new();
    let sha = repo.commit(
        &[
            ("src/a.rs", "pub fn good_one() {}\n"),
            (
                "src/broken.ts",
                "export function broken( {\n    return 1;\n}\n",
            ),
        ],
        "one broken file",
    );

    for _ in 0..2 {
        index_commit(&db, repo.path(), repo_id, &sha, "main")
            .await
            .expect("index");
        assert_eq!(
            stats(&db).await,
            (2, 1, 0),
            "the second run must overwrite, not accumulate"
        );
    }
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM symbol_commits").await,
        1,
        "one row per commit"
    );

    // The good file is still searchable: partial does not mean empty.
    let page = page_for(&db, repo_id, &sha, "good_one").await;
    assert_eq!(page.items.len(), 1);
    assert!(page.partial);
    assert!(
        page.limitations
            .join(" | ")
            .contains("1 files failed to parse and are only partially indexed"),
        "{:#?}",
        page.limitations
    );
}

/// A file over the item cap is recorded as truncated; the page says so with
/// the count.
#[tokio::test]
async fn truncated_files_are_counted_and_reported() {
    let (db, _db_dir, repo_id) = seed_database().await;
    let repo = TestRepo::new();
    let mut dense = String::new();
    for index in 0..2001 {
        dense.push_str(&format!("pub fn dense_{index}() {{}}\n"));
    }
    let sha = repo.commit(&[("src/dense.rs", &dense)], "dense");

    index_commit(&db, repo.path(), repo_id, &sha, "main")
        .await
        .expect("index");
    assert_eq!(stats(&db).await, (1, 0, 1));

    let page = page_for(&db, repo_id, &sha, "dense_0").await;
    assert!(page.partial);
    assert!(
        page.limitations
            .join(" | ")
            .contains("1 files exceeded the item cap and were truncated"),
        "{:#?}",
        page.limitations
    );
}

/// A row written before the counters existed (the `-1` sentinel) reads back as
/// `partial` with "completeness is unknown", never as complete.
#[tokio::test]
async fn a_row_without_stats_stays_partial_and_says_why() {
    let (db, _db_dir, repo_id) = seed_database().await;
    let repo = TestRepo::new();
    let sha = repo.commit(&[("src/a.rs", "pub fn known() {}\n")], "legacy");
    index_commit(&db, repo.path(), repo_id, &sha, "main")
        .await
        .expect("index");

    db.execute(Statement::from_string(
        db.get_database_backend(),
        "UPDATE symbol_commits SET files_total = -1, files_failed = 0, files_truncated = 0"
            .to_string(),
    ))
    .await
    .expect("emulate a pre-counter row");

    let page = page_for(&db, repo_id, &sha, "known").await;
    assert_eq!(page.items.len(), 1, "the definitions are still served");
    assert!(page.partial);
    let joined = page.limitations.join(" | ");
    assert!(
        joined.contains(
            "this commit was indexed before completeness stats were recorded; \
             completeness is unknown"
        ),
        "{joined}"
    );
    assert!(!joined.contains("failed to parse"), "{joined}");
}

/// Re-indexing the same commit does not duplicate rows, and `impl` rows are
/// persisted (they are excluded by the query side, not by the writer).
#[tokio::test]
async fn indexing_the_same_commit_twice_is_idempotent_and_keeps_impl_rows() {
    let (db, _db_dir, repo_id) = seed_database().await;
    let repo = TestRepo::new();
    let sha = repo.commit(
        &[(
            "src/lib.rs",
            "pub struct Foo;\n\nimpl Foo {\n    pub fn method(&self) {}\n}\n\n\
             impl std::fmt::Display for Foo {\n    fn fmt(&self) {}\n}\n\n\
             pub fn top() {}\n",
        )],
        "initial",
    );

    let first = index_commit(&db, repo.path(), repo_id, &sha, "main")
        .await
        .expect("first index");
    assert_eq!(first.commit_sha, sha);
    assert_eq!(first.pruned_commits, 0);
    let first_rows = count(&db, "SELECT COUNT(*) FROM symbol_definitions").await;
    assert_eq!(first_rows as usize, first.definitions_written);
    assert!(
        first_rows >= 5,
        "expected struct, impl, method, fn, top — got {first_rows}"
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM symbol_definitions WHERE kind = 'impl'"
        )
        .await,
        2,
        "both impl rows must be stored, not filtered"
    );
    assert!(
        count(
            &db,
            "SELECT COUNT(*) FROM symbol_definitions \
             WHERE kind = 'impl' AND name LIKE '% for %'"
        )
        .await
            >= 1,
        "an impl name is a first line, not an identifier"
    );

    let second = index_commit(&db, repo.path(), repo_id, &sha, "main")
        .await
        .expect("second index");
    assert_eq!(
        second.commit_id, first.commit_id,
        "the same commit reuses its symbol_commits row"
    );
    assert_eq!(second.definitions_written, first.definitions_written);
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM symbol_definitions").await,
        first_rows,
        "re-indexing must not double the rows"
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM symbol_commits").await,
        1,
        "one symbol_commits row per (repo, sha)"
    );
}

/// Indexing `D + 2` commits of the default branch leaves exactly `D` and the
/// oldest commit's definitions are gone.
#[tokio::test]
async fn pruning_keeps_only_the_retention_window() {
    let (db, _db_dir, repo_id) = seed_database().await;
    let repo = TestRepo::new();

    let depth = retention_depth(4);
    assert_eq!(
        depth, MAX_RETAINED_COMMITS,
        "small commits keep the full window"
    );
    let mut shas = Vec::new();
    for index in 0..depth + 2 {
        shas.push(repo.commit(
            &[(
                "a.rs",
                &format!("pub fn symbol_{index}() {{}}\npub struct Widget{index};\n"),
            )],
            &format!("commit {index}"),
        ));
    }

    let mut pruned_total = 0usize;
    for sha in &shas {
        let report = index_commit(&db, repo.path(), repo_id, sha, "main")
            .await
            .expect("index commit");
        assert_eq!(report.commit_sha, *sha);
        pruned_total += report.pruned_commits;
    }

    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM symbol_commits").await as usize,
        depth,
        "only the retention window survives"
    );
    assert_eq!(pruned_total, 2, "the two oldest commits were pruned");
    assert_eq!(
        count(
            &db,
            &format!(
                "SELECT COUNT(*) FROM symbol_definitions WHERE commit_id IN \
                 (SELECT id FROM symbol_commits WHERE commit_sha = '{}')",
                shas[0]
            )
        )
        .await,
        0,
        "definitions of the pruned commit must be deleted with it"
    );
    assert_eq!(
        count(
            &db,
            &format!(
                "SELECT COUNT(*) FROM symbol_definitions WHERE commit_id IN \
                 (SELECT id FROM symbol_commits WHERE commit_sha = '{}')",
                shas[depth + 1]
            )
        )
        .await,
        2,
        "the newest commit's two definitions are still there"
    );
}

/// An unknown commit is a 404-class error, not a 500 and not a silent no-op.
#[tokio::test]
async fn unknown_commit_is_not_found() {
    let (db, _db_dir, repo_id) = seed_database().await;
    let repo = TestRepo::new();
    repo.commit(&[("a.rs", "pub fn one() {}\n")], "initial");

    let error = index_commit(
        &db,
        repo.path(),
        repo_id,
        "0000000000000000000000000000000000000000",
        "main",
    )
    .await
    .expect_err("unknown commit must fail");
    assert!(
        matches!(error, rg_core::error::CoreError::NotFound(_)),
        "expected NotFound, got {error:?}"
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM symbol_commits").await,
        0,
        "nothing may be written for an unknown commit"
    );
}

/// The definition row count for this repository must match the design
/// document's measurement (12,208 ±5%).
///
/// Ignored by default: it parses the whole checkout (~3 s) and is a manual
/// acceptance check, not a unit test. Run with
/// `cargo test -p rg-core --release measure_this_repository_symbols -- --ignored --nocapture`.
#[tokio::test]
#[ignore]
async fn measure_this_repository_symbols() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (db, _db_dir, repo_id) = seed_database().await;
    let head = git(&["rev-parse", "HEAD"], Some(&repo_root));

    let before = page_bytes(&db).await;
    let started = std::time::Instant::now();
    let report = index_commit(&db, &repo_root, repo_id, &head, "main")
        .await
        .expect("index this repository");
    let elapsed = started.elapsed();
    let after = page_bytes(&db).await;

    let impl_rows = count(
        &db,
        "SELECT COUNT(*) FROM symbol_definitions WHERE kind = 'impl'",
    )
    .await;
    let db_bytes = after.saturating_sub(before);

    println!("commit: {head}");
    println!("definitions_written: {}", report.definitions_written);
    println!(
        "completeness (files_total, files_failed, files_truncated): {:?}",
        stats(&db).await
    );
    println!(
        "symbol_definitions rows: {}",
        count(&db, "SELECT COUNT(*) FROM symbol_definitions").await
    );
    println!("impl rows: {impl_rows}");
    println!(
        "db growth: {db_bytes} B ({:.2} MiB)",
        db_bytes as f64 / 1_048_576.0
    );
    println!("elapsed: {:.2?}", elapsed);

    if let Ok(rows) = db
        .query_all(Statement::from_string(
            db.get_database_backend(),
            "SELECT name, SUM(pgsize) FROM dbstat GROUP BY name ORDER BY 2 DESC".to_string(),
        ))
        .await
    {
        for row in rows {
            let name: String = row.try_get_by_index(0).unwrap_or_default();
            let size: i64 = row.try_get_by_index(1).unwrap_or_default();
            println!("  {name}: {} KiB", size / 1024);
        }
    }
}

/// The write path (upsert, batch insert, prune) is raw SQL; run it once on the
/// backend named by `IRONFORGE_TEST_DATABASE_URL` so PostgreSQL's numbered
/// parameters and MySQL's multi-row insert are real coverage, not assumptions.
/// Without the variable there is nothing to do (SQLite is covered above).
#[tokio::test]
async fn write_path_round_trip_on_the_backend_from_the_environment() {
    let Ok(url) = std::env::var("IRONFORGE_TEST_DATABASE_URL") else {
        return;
    };
    let db = rg_db::connect_with_pool(&url, 10, 60, 4)
        .await
        .expect("connect to IRONFORGE_TEST_DATABASE_URL");
    rg_db::run_migrations(&db).await.expect("migrations");
    let (repo_id, user_id) = seed_repository_rows(&db, "env").await;

    let repo = TestRepo::new();
    let sha_one = repo.commit(&[("a.rs", "pub fn one() {}\npub struct One;\n")], "one");
    let sha_two = repo.commit(&[("a.rs", "pub fn two() {}\npub struct Two;\n")], "two");

    let first = index_commit(&db, repo.path(), repo_id, &sha_one, "main")
        .await
        .expect("index one");
    assert_eq!(first.definitions_written, 2);
    let second = index_commit(&db, repo.path(), repo_id, &sha_one, "main")
        .await
        .expect("re-index one");
    assert_eq!(second.definitions_written, first.definitions_written);
    assert_eq!(
        count(
            &db,
            &format!("SELECT COUNT(*) FROM symbol_definitions WHERE repo_id = {repo_id}")
        )
        .await,
        2,
        "idempotent on this backend too"
    );

    index_commit(&db, repo.path(), repo_id, &sha_two, "main")
        .await
        .expect("index two");
    index_commit(&db, repo.path(), repo_id, &sha_two, "main")
        .await
        .expect("re-index two");
    assert_eq!(
        count(
            &db,
            &format!("SELECT COUNT(*) FROM symbol_definitions WHERE repo_id = {repo_id}")
        )
        .await,
        4,
        "the older commit stays inside the retention window"
    );

    // Clean up the shared database: rows first, then their parents.
    for sql in [
        format!("DELETE FROM symbol_definitions WHERE repo_id = {repo_id}"),
        format!("DELETE FROM symbol_commits WHERE repo_id = {repo_id}"),
        format!("DELETE FROM repositories WHERE id = {repo_id}"),
        format!("DELETE FROM users WHERE id = {user_id}"),
    ] {
        db.execute(Statement::from_string(db.get_database_backend(), sql))
            .await
            .expect("cleanup");
    }
}

/// The read path (`find_symbols`) decodes the counters as well; run it once on
/// the backend named by `IRONFORGE_TEST_DATABASE_URL` so PostgreSQL's `INT4`
/// columns and MySQL's `int` are real coverage. Without the variable there is
/// nothing to do (SQLite is covered above).
#[tokio::test]
async fn symbol_query_round_trip_on_the_backend_from_the_environment() {
    let Ok(url) = std::env::var("IRONFORGE_TEST_DATABASE_URL") else {
        return;
    };
    let db = rg_db::connect_with_pool(&url, 10, 60, 4)
        .await
        .expect("connect to IRONFORGE_TEST_DATABASE_URL");
    rg_db::run_migrations(&db).await.expect("migrations");
    let (repo_id, user_id) = seed_repository_rows(&db, "query").await;

    let repo = TestRepo::new();
    let sha = repo.commit(
        &[
            ("src/a.rs", "pub fn queried_one() {}\n"),
            (
                "src/broken.ts",
                "export function broken( {\n    return 1;\n}\n",
            ),
        ],
        "queryable",
    );
    index_commit(&db, repo.path(), repo_id, &sha, "main")
        .await
        .expect("index");

    let page = page_for(&db, repo_id, &sha, "queried_one").await;
    assert_eq!(page.items.len(), 1, "the clean file is answered: {page:#?}");
    assert!(page.partial, "the broken file makes this commit partial");
    assert!(
        page.limitations
            .join(" | ")
            .contains("1 files failed to parse"),
        "{:#?}",
        page.limitations
    );

    for sql in [
        format!("DELETE FROM symbol_definitions WHERE repo_id = {repo_id}"),
        format!("DELETE FROM symbol_commits WHERE repo_id = {repo_id}"),
        format!("DELETE FROM repositories WHERE id = {repo_id}"),
        format!("DELETE FROM users WHERE id = {user_id}"),
    ] {
        db.execute(Statement::from_string(db.get_database_backend(), sql))
            .await
            .expect("cleanup");
    }
}

/// Insert a `(repository, user)` pair with names unique per process, so the
/// shared environment database can be reused by concurrent runs.
async fn seed_repository_rows(db: &DatabaseConnection, prefix: &str) -> (i64, i64) {
    let suffix = format!("{}-{}", std::process::id(), prefix);
    let now = chrono::Utc::now();
    let owner = user::ActiveModel {
        username: Set(format!("sym-env-{suffix}")),
        email: Set(format!("sym-env-{suffix}@example.com")),
        password_hash: Set("unused".to_string()),
        display_name: Set(Some("symbol".to_string())),
        is_admin: Set(false),
        is_active: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert user");
    let repo = repository::ActiveModel {
        owner_id: Set(owner.id),
        name: Set(format!("symbols-{suffix}")),
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
    (repo.id, owner.id)
}

async fn page_bytes(db: &DatabaseConnection) -> u64 {
    db.query_one(Statement::from_string(
        db.get_database_backend(),
        "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
    ))
    .await
    .expect("page count")
    .expect("row")
    .try_get_by_index::<i64>(0)
    .unwrap_or(0) as u64
}
