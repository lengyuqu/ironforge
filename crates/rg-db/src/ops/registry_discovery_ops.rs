//! Cross-repository candidate listing for the skill and MCP-server registries.
//!
//! This returns **candidates only**: packages of one type, in repositories that
//! exist, in registries that are enabled, ordered by package id. It knows
//! nothing about who may read a repository. The service that calls it decides
//! per candidate, so that a page is built from what the caller may see and not
//! trimmed afterwards (see `rg_core::package_registry::discovery`).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, FromQueryResult, Statement, Value};

/// Longest text a `LIKE` pattern is built from.
pub const MAX_QUERY_CHARS: usize = 200;

#[derive(Debug, Clone, FromQueryResult)]
pub struct Candidate {
    pub package_id: i64,
    pub repo_id: i64,
}

/// Escape `text` for use inside a `LIKE` pattern with `ESCAPE '!'`.
///
/// `!` is the escape character because, unlike a backslash, it means the same
/// in a string literal on SQLite, PostgreSQL and MySQL.
fn like_pattern(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('%');
    for c in text.to_lowercase().chars() {
        if matches!(c, '!' | '%' | '_') {
            out.push('!');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// Up to `limit` candidates with a package id greater than `after_id`, oldest
/// first. `query`, when given, must appear in the package's name or description
/// (case-insensitive substring).
pub async fn candidates(
    db: &DatabaseConnection,
    package_type: &str,
    query: Option<&str>,
    after_id: i64,
    limit: u64,
) -> Result<Vec<Candidate>, DbErr> {
    let backend = db.get_database_backend();
    let mut sql = String::from(
        "SELECT p.id AS package_id, r.repo_id AS repo_id \
         FROM packages p \
         JOIN package_registry r ON r.id = p.package_registry_id \
         JOIN repositories repo ON repo.id = r.repo_id \
         WHERE r.package_type = ? AND r.enabled = ? AND repo.deleted_at IS NULL AND p.id > ?",
    );
    let mut values: Vec<Value> = vec![package_type.into(), true.into(), after_id.into()];
    if let Some(query) = query {
        sql.push_str(
            " AND (LOWER(p.name) LIKE ? ESCAPE '!' OR LOWER(COALESCE(p.description, '')) LIKE ? ESCAPE '!')",
        );
        let pattern = like_pattern(query);
        values.push(pattern.clone().into());
        values.push(pattern.into());
    }
    sql.push_str(" ORDER BY p.id ASC LIMIT ?");
    values.push((limit as i64).into());

    Candidate::find_by_statement(Statement::from_sql_and_values(
        backend,
        crate::prepare_sql(backend, &sql),
        values,
    ))
    .all(db)
    .await
}

#[cfg(test)]
mod tests {
    use super::like_pattern;

    #[test]
    fn like_wildcards_in_the_query_are_literal() {
        assert_eq!(like_pattern("pdf"), "%pdf%");
        assert_eq!(like_pattern("50%_off!"), "%50!%!_off!!%");
        assert_eq!(like_pattern("PDF"), "%pdf%");
    }
}
