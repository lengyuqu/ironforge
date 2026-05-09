//! FTS5 global search service.

use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use serde::Serialize;

/// A unified search result.
#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub result_type: String,
    pub id: i64,
    pub title: String,
    pub excerpt: Option<String>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
}

/// Search across repositories, issues, and/or wiki pages using FTS5.
pub async fn search(
    db: &DatabaseConnection,
    query: &str,
    search_type: &str,
    page: u64,
    per_page: u64,
) -> Result<(Vec<SearchResult>, i64)> {
    let offset = (page.saturating_sub(1)) * per_page;
    let limit = per_page.min(100);

    let mut results = Vec::new();
    let mut total = 0i64;

    // Sanitize FTS query (escape special characters)
    let safe_query = fts_escape(query);

    if search_type == "all" || search_type == "repos" {
        let (repos, count) = search_repos(db, &safe_query, offset, limit).await?;
        total += count;
        results.extend(repos);
    }

    if search_type == "all" || search_type == "issues" {
        let (issues, count) = search_issues(db, &safe_query, offset, limit).await?;
        total += count;
        results.extend(issues);
    }

    if search_type == "all" || search_type == "wiki" {
        let (wiki, count) = search_wiki(db, &safe_query, offset, limit).await?;
        total += count;
        results.extend(wiki);
    }

    // For pagination when search_type is "all", apply offset/limit to the combined set
    if search_type == "all" {
        let skip = offset as usize;
        let take = limit as usize;
        let trimmed = results.into_iter().skip(skip).take(take).collect();
        return Ok((trimmed, total));
    }

    Ok((results, total))
}

/// Escape FTS5 special characters in query for safe inclusion in a phrase-search MATCH.
/// Wraps the query in double quotes for FTS5 phrase-search mode (literal matching).
/// Escapes embedded double-quotes by doubling them (`"` → `""`) per FTS5 spec.
fn fts_escape(query: &str) -> String {
    let escaped = query.replace('"', "\"\"");
    format!("\"{}\"", escaped)
}

/// Search repositories by name and description.
async fn search_repos(
    db: &DatabaseConnection,
    query: &str,
    offset: u64,
    limit: u64,
) -> Result<(Vec<SearchResult>, i64)> {
    let sql = format!(
        r#"
        SELECT r.id, r.name as title, r.description as excerpt, u.username as owner_name
        FROM repos_fts f
        JOIN repositories r ON r.id = f.rowid
        LEFT JOIN users u ON u.id = r.owner_id
        WHERE repos_fts MATCH '"{}"'
        ORDER BY rank
        LIMIT {} OFFSET {}
        "#,
        query, limit, offset
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            &sql,
            [],
        ))
        .await
        .context("fts: search repos")?;

    let mut results = Vec::new();
    for row in rows {
        let id: i64 = row.try_get_by_index(0).unwrap_or(0);
        let title: String = row.try_get_by_index(1).unwrap_or_default();
        let excerpt: Option<String> = row.try_get_by_index(2).ok();
        let owner: Option<String> = row.try_get_by_index(3).ok();
        results.push(SearchResult {
            result_type: "repo".to_string(),
            id,
            title,
            excerpt,
            repo_owner: owner.clone(),
            repo_name: None,
        });
    }

    // Count total
    let count_sql = format!(
        r#"SELECT COUNT(*) FROM repos_fts WHERE repos_fts MATCH '"{}"'"#,
        query
    );
    let count_rows = db
        .query_all(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            &count_sql,
            [],
        ))
        .await
        .context("fts: count repos")?;
    let total: i64 = count_rows
        .first()
        .and_then(|r| r.try_get_by_index::<i64>(0).ok())
        .unwrap_or(0);

    Ok((results, total))
}

/// Search issues by title and body.
async fn search_issues(
    db: &DatabaseConnection,
    query: &str,
    offset: u64,
    limit: u64,
) -> Result<(Vec<SearchResult>, i64)> {
    let sql = format!(
        r#"
        SELECT i.id, i.title, i.body as excerpt, i.repo_id, r.name as repo_name, u.username as owner_name
        FROM issues_fts f
        JOIN issues i ON i.id = f.rowid
        JOIN repositories r ON r.id = i.repo_id
        LEFT JOIN users u ON u.id = r.owner_id
        WHERE issues_fts MATCH '"{}"'
        ORDER BY rank
        LIMIT {} OFFSET {}
        "#,
        query, limit, offset
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            &sql,
            [],
        ))
        .await
        .context("fts: search issues")?;

    let mut results = Vec::new();
    for row in rows {
        let id: i64 = row.try_get_by_index(0).unwrap_or(0);
        let title: String = row.try_get_by_index(1).unwrap_or_default();
        let excerpt: Option<String> = row.try_get_by_index(2).ok();
        let _repo_id: i64 = row.try_get_by_index(3).unwrap_or(0);
        let repo_name: Option<String> = row.try_get_by_index(4).ok();
        let owner: Option<String> = row.try_get_by_index(5).ok();
        results.push(SearchResult {
            result_type: "issue".to_string(),
            id,
            title,
            excerpt,
            repo_owner: owner,
            repo_name,
        });
    }

    let count_sql = format!(
        r#"SELECT COUNT(*) FROM issues_fts WHERE issues_fts MATCH '"{}"'"#,
        query
    );
    let count_rows = db
        .query_all(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            &count_sql,
            [],
        ))
        .await
        .context("fts: count issues")?;
    let total: i64 = count_rows
        .first()
        .and_then(|r| r.try_get_by_index::<i64>(0).ok())
        .unwrap_or(0);

    Ok((results, total))
}

/// Search wiki pages by title and content.
async fn search_wiki(
    db: &DatabaseConnection,
    query: &str,
    offset: u64,
    limit: u64,
) -> Result<(Vec<SearchResult>, i64)> {
    let sql = format!(
        r#"
        SELECT w.id, w.title, SUBSTR(w.content, 1, 200) as excerpt, w.repo_id, r.name as repo_name, u.username as owner_name
        FROM wiki_pages_fts f
        JOIN wiki_pages w ON w.id = f.rowid
        JOIN repositories r ON r.id = w.repo_id
        LEFT JOIN users u ON u.id = r.owner_id
        WHERE wiki_pages_fts MATCH '"{}"'
        ORDER BY rank
        LIMIT {} OFFSET {}
        "#,
        query, limit, offset
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            &sql,
            [],
        ))
        .await
        .context("fts: search wiki")?;

    let mut results = Vec::new();
    for row in rows {
        let id: i64 = row.try_get_by_index(0).unwrap_or(0);
        let title: String = row.try_get_by_index(1).unwrap_or_default();
        let excerpt: Option<String> = row.try_get_by_index(2).ok();
        let _repo_id: i64 = row.try_get_by_index(3).unwrap_or(0);
        let repo_name: Option<String> = row.try_get_by_index(4).ok();
        let owner: Option<String> = row.try_get_by_index(5).ok();
        results.push(SearchResult {
            result_type: "wiki".to_string(),
            id,
            title,
            excerpt,
            repo_owner: owner,
            repo_name,
        });
    }

    let count_sql = format!(
        r#"SELECT COUNT(*) FROM wiki_pages_fts WHERE wiki_pages_fts MATCH '"{}"'"#,
        query
    );
    let count_rows = db
        .query_all(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            &count_sql,
            [],
        ))
        .await
        .context("fts: count wiki")?;
    let total: i64 = count_rows
        .first()
        .and_then(|r| r.try_get_by_index::<i64>(0).ok())
        .unwrap_or(0);

    Ok((results, total))
}
