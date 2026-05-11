//! FTS5 global search service.
//!
//! Supports GitHub-style search qualifiers:
//!   - `repo:owner/name` — filter by repository
//!   - `author:username` — filter by author/owner
//!   - `state:open|closed|all` — filter issue state
//!   - `label:name` — filter by label
//!   - `is:open|closed|merged` — filter issue state (alias)
//!   - `language:rust` — filter by primary language (future)
//!
//! Example: `q=bug fix repo:owner/repo state:open`

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
    /// For issues: the issue state (open/closed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// For issues: the issue number within its repo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<i64>,
}

/// Parsed search qualifiers extracted from the query string.
#[derive(Debug, Default, Clone)]
pub struct SearchFilters {
    /// Filter by repo: "owner/name" → resolved to repo_id
    pub repo: Option<String>,
    /// Filter by issue state: open, closed, all
    pub state: Option<String>,
    /// Filter by author username
    pub author: Option<String>,
    /// Filter by label name
    pub label: Option<String>,
    /// The remaining text query (without qualifiers)
    pub query: String,
}

impl SearchFilters {
    /// Parse a search query string, extracting qualifiers and returning the clean text query.
    pub fn parse(raw: &str) -> Self {
        let mut filters = SearchFilters::default();
        let mut query_parts = Vec::new();
        let tokens: Vec<&str> = raw.split_whitespace().collect();

        for token in tokens {
            if let Some((key, value)) = token.split_once(':') {
                let key_lower = key.to_lowercase();
                let clean_value = value.trim_matches('"').to_string();

                if clean_value.is_empty() {
                    query_parts.push(token.to_string());
                    continue;
                }

                match key_lower.as_str() {
                    "repo" => filters.repo = Some(clean_value),
                    "state" | "is" => filters.state = Some(clean_value.to_lowercase()),
                    "author" | "user" => filters.author = Some(clean_value),
                    "label" => filters.label = Some(clean_value),
                    _ => query_parts.push(token.to_string()),
                }
            } else {
                query_parts.push(token.to_string());
            }
        }

        filters.query = query_parts.join(" ");
        filters
    }
}

/// Search across repositories, issues, and/or wiki pages using FTS5.
/// Supports qualifier-based filtering via `q` parameter.
pub async fn search(
    db: &DatabaseConnection,
    raw_query: &str,
    search_type: &str,
    page: u64,
    per_page: u64,
) -> Result<(Vec<SearchResult>, i64)> {
    let offset = (page.saturating_sub(1)) * per_page;
    let limit = per_page.min(100);

    let filters = SearchFilters::parse(raw_query);

    let mut results = Vec::new();
    let mut total = 0i64;

    // Sanitize FTS query (escape special characters)
    let safe_query = if filters.query.is_empty() {
        "*".to_string() // match everything if no text query
    } else {
        fts_escape(&filters.query)
    };

    if search_type == "all" || search_type == "repos" {
        let (repos, count) = search_repos(db, &safe_query, &filters, offset, limit).await?;
        total += count;
        results.extend(repos);
    }

    if search_type == "all" || search_type == "issues" {
        let (issues, count) = search_issues(db, &safe_query, &filters, offset, limit).await?;
        total += count;
        results.extend(issues);
    }

    if search_type == "all" || search_type == "wiki" {
        let (wiki, count) = search_wiki(db, &safe_query, &filters, offset, limit).await?;
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

/// Build SQL WHERE clauses from filters.
fn build_filter_clauses(filters: &SearchFilters, table_alias: &str) -> (Vec<String>, Vec<String>) {
    let mut clauses = Vec::new();
    let mut joins = Vec::new();

    if let Some(ref repo) = filters.repo {
        // Parse "owner/name" format
        if let Some((owner, name)) = repo.split_once('/') {
            joins.push(format!(
                "JOIN repositories r_filt ON r_filt.id = {}.repo_id",
                table_alias
            ));
            joins.push(format!(
                "LEFT JOIN users u_filt ON u_filt.id = r_filt.owner_id"
            ));
            clauses.push(format!(
                "u_filt.username = '{}' AND r_filt.name = '{}'",
                owner.replace('\'', "''"),
                name.replace('\'', "''")
            ));
        } else {
            // Just repo name, match by name
            joins.push(format!(
                "JOIN repositories r_filt ON r_filt.id = {}.repo_id",
                table_alias
            ));
            clauses.push(format!(
                "r_filt.name = '{}'",
                repo.replace('\'', "''")
            ));
        }
    }

    if let Some(ref author) = filters.author {
        joins.push(format!(
            "LEFT JOIN users u_auth ON u_auth.username = '{}'",
            author.replace('\'', "''")
        ));
        clauses.push(format!("{}.author_id = u_auth.id", table_alias));
    }

    (clauses, joins)
}

/// Build issue-specific filter clauses (state, label).
fn build_issue_filter_clauses(filters: &SearchFilters) -> (Vec<String>, Vec<String>) {
    let mut clauses = Vec::new();
    let mut joins = Vec::new();

    if let Some(ref state) = filters.state {
        if state != "all" {
            clauses.push(format!("i.state = '{}'", state.replace('\'', "''")));
        }
    }

    if let Some(ref label) = filters.label {
        joins.push(format!(
            "LEFT JOIN issue_labels il_filt ON il_filt.issue_id = i.id"
        ));
        joins.push(format!(
            "LEFT JOIN labels lbl_filt ON lbl_filt.id = il_filt.label_id"
        ));
        clauses.push(format!(
            "lbl_filt.name = '{}'",
            label.replace('\'', "''")
        ));
    }

    (clauses, joins)
}

/// Search repositories by name and description, with optional filters.
async fn search_repos(
    db: &DatabaseConnection,
    query: &str,
    filters: &SearchFilters,
    offset: u64,
    limit: u64,
) -> Result<(Vec<SearchResult>, i64)> {
    let (filter_clauses, extra_joins) = build_filter_clauses(filters, "r");

    // Base query
    let base_where = format!("repos_fts MATCH {}", query);

    // Add filter clauses
    if !filter_clauses.is_empty() {
        let filter_sql = filter_clauses.join(" AND ");
        let sql = format!(
            r#"
            SELECT r.id, r.name as title, r.description as excerpt, u.username as owner_name
            FROM repos_fts f
            JOIN repositories r ON r.id = f.rowid
            LEFT JOIN users u ON u.id = r.owner_id
            {}
            WHERE {} AND ({})
            ORDER BY rank
            LIMIT {} OFFSET {}
            "#,
            extra_joins.join("\n"),
            base_where,
            filter_sql,
            limit,
            offset
        );

        let rows = db
            .query_all(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Sqlite,
                &sql,
                [],
            ))
            .await
            .context("fts: search repos filtered")?;

        let count = rows.len() as i64;
        let mut results = Vec::new();
        for row in &rows {
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
                state: None,
                number: None,
            });
        }

        return Ok((results, count));
    }

    let sql = format!(
        r#"
        SELECT r.id, r.name as title, r.description as excerpt, u.username as owner_name
        FROM repos_fts f
        JOIN repositories r ON r.id = f.rowid
        LEFT JOIN users u ON u.id = r.owner_id
        WHERE {}
        ORDER BY rank
        LIMIT {} OFFSET {}
        "#,
        base_where, limit, offset
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
            state: None,
            number: None,
        });
    }

    // Count total
    let count_sql = format!(r#"SELECT COUNT(*) FROM repos_fts WHERE {}"#, base_where);
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

/// Search issues by title and body, with optional filters (repo, state, author, label).
async fn search_issues(
    db: &DatabaseConnection,
    query: &str,
    filters: &SearchFilters,
    offset: u64,
    limit: u64,
) -> Result<(Vec<SearchResult>, i64)> {
    let (mut common_clauses, common_joins) = build_filter_clauses(filters, "i");
    let (issue_clauses, issue_joins) = build_issue_filter_clauses(filters);

    common_clauses.extend(issue_clauses);
    let all_joins = format!("{}\n{}", common_joins.join("\n"), issue_joins.join("\n"));

    let base_where = format!("issues_fts MATCH {}", query);

    // Build WHERE clause
    let where_clause = if common_clauses.is_empty() {
        base_where.clone()
    } else {
        format!("{} AND ({})", base_where, common_clauses.join(" AND "))
    };

    let sql = format!(
        r#"
        SELECT i.id, i.title, i.body as excerpt, i.repo_id, r.name as repo_name, u.username as owner_name, i.state, i.number
        FROM issues_fts f
        JOIN issues i ON i.id = f.rowid
        JOIN repositories r ON r.id = i.repo_id
        LEFT JOIN users u ON u.id = r.owner_id
        {}
        WHERE {}
        ORDER BY rank
        LIMIT {} OFFSET {}
        "#,
        all_joins, where_clause, limit, offset
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
        let state: Option<String> = row.try_get_by_index(6).ok();
        let number: Option<i64> = row.try_get_by_index(7).ok();
        results.push(SearchResult {
            result_type: "issue".to_string(),
            id,
            title,
            excerpt,
            repo_owner: owner,
            repo_name,
            state,
            number,
        });
    }

    // Count total (with same filters)
    let count_where = if common_clauses.is_empty() {
        base_where
    } else {
        format!("{} AND ({})", base_where, common_clauses.join(" AND "))
    };
    let count_sql = format!(
        r#"
        SELECT COUNT(*)
        FROM issues_fts f
        JOIN issues i ON i.id = f.rowid
        {}
        WHERE {}
        "#,
        all_joins, count_where
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

/// Search wiki pages by title and content, with optional filters.
async fn search_wiki(
    db: &DatabaseConnection,
    query: &str,
    filters: &SearchFilters,
    offset: u64,
    limit: u64,
) -> Result<(Vec<SearchResult>, i64)> {
    let (filter_clauses, extra_joins) = build_filter_clauses(filters, "w");

    let base_where = format!("wiki_pages_fts MATCH {}", query);

    let where_clause = if filter_clauses.is_empty() {
        base_where.clone()
    } else {
        format!("{} AND ({})", base_where, filter_clauses.join(" AND "))
    };

    let sql = format!(
        r#"
        SELECT w.id, w.title, SUBSTR(w.content, 1, 200) as excerpt, w.repo_id, r.name as repo_name, u.username as owner_name
        FROM wiki_pages_fts f
        JOIN wiki_pages w ON w.id = f.rowid
        JOIN repositories r ON r.id = w.repo_id
        LEFT JOIN users u ON u.id = r.owner_id
        {}
        WHERE {}
        ORDER BY rank
        LIMIT {} OFFSET {}
        "#,
        extra_joins.join("\n"),
        where_clause,
        limit,
        offset
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
            state: None,
            number: None,
        });
    }

    // Count total
    let count_sql = format!(
        r#"SELECT COUNT(*) FROM wiki_pages_fts WHERE {}"#,
        base_where
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
