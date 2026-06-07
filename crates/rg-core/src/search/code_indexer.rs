//! Code indexing service for code search.
//!
//! This module provides functionality to index Git repository contents
//! into the `code_fts` FTS5 virtual table for fast full-text search.
//!
//! # Usage
//!
//! ```rust,ignore
//! use rg_core::search::code_indexer::CodeIndexer;
//!
//! let indexer = CodeIndexer::new(db.clone());
//! indexer.index_repository(repo_id, repo_path, "main").await?;
//! ```

use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Map file extensions to programming languages.
const EXTENSION_TO_LANGUAGE: &[(&str, &str)] = &[
    ("rs", "Rust"),
    ("py", "Python"),
    ("js", "JavaScript"),
    ("ts", "TypeScript"),
    ("jsx", "JavaScript (JSX)"),
    ("tsx", "TypeScript (TSX)"),
    ("go", "Go"),
    ("java", "Java"),
    ("c", "C"),
    ("h", "C"),
    ("cc", "C++"),
    ("cpp", "C++"),
    ("hpp", "C++"),
    ("cs", "C#"),
    ("rb", "Ruby"),
    ("php", "PHP"),
    ("swift", "Swift"),
    ("kt", "Kotlin"),
    ("kts", "Kotlin"),
    ("sh", "Shell"),
    ("bash", "Shell"),
    ("zsh", "Shell"),
    ("sql", "SQL"),
    ("html", "HTML"),
    ("css", "CSS"),
    ("scss", "SCSS"),
    ("less", "LESS"),
    ("xml", "XML"),
    ("json", "JSON"),
    ("yaml", "YAML"),
    ("yml", "YAML"),
    ("toml", "TOML"),
    ("lock", "Lock file"),
    ("md", "Markdown"),
    ("txt", "Text"),
    ("rst", "reStructuredText"),
    ("lua", "Lua"),
    ("pl", "Perl"),
    ("pm", "Perl"),
    ("r", "R"),
    ("scala", "Scala"),
    ("clj", "Clojure"),
    ("cljs", "ClojureScript"),
    ("elm", "Elm"),
    ("ex", "Elixir"),
    ("exs", "Elixir"),
    ("erl", "Erlang"),
    ("hrl", "Erlang"),
    ("ml", "OCaml"),
    ("mli", "OCaml"),
    ("fs", "F#"),
    ("fsx", "F#"),
    ("hs", "Haskell"),
    ("lhs", "Haskell"),
    ("dart", "Dart"),
    ("vue", "Vue"),
    ("svelte", "Svelte"),
    ("astro", "Astro"),
];

/// Infer the programming language from a file path.
fn infer_language(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    for (extension, language) in EXTENSION_TO_LANGUAGE {
        if *extension == ext {
            return language.to_string();
        }
    }

    // Check for files without extension but with known names
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    match file_name.as_str() {
        "makefile" | "gnumakefile" => "Makefile".to_string(),
        "dockerfile" => "Dockerfile".to_string(),
        "cmakelists.txt" => "CMake".to_string(),
        "cargo.toml" => "TOML".to_string(),
        "package.json" => "JSON".to_string(),
        "tsconfig.json" => "JSON".to_string(),
        _ => "Text".to_string(),
    }
}

/// Check if a file should be indexed (not binary, not too large).
fn should_index(path: &Path, content: &[u8]) -> bool {
    // Skip files that are too large (> 1MB)
    if content.len() > 1_048_576 {
        return false;
    }

    // Skip binary files (check for null bytes)
    if content.contains(&0u8) {
        return false;
    }

    // Skip certain file types
    let path_str = path.to_string_lossy().to_lowercase();
    let skip_extensions = [
        "lock", "min.js", "min.css", "map", "gz", "zip", "tar", "png", "jpg", "jpeg",
        "gif", "bmp", "ico", "woff", "woff2", "ttf", "eot", "mp3", "mp4", "avi", "mov",
        "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "exe", "dll", "so", "dylib",
    ];

    for ext in skip_extensions.iter() {
        if path_str.ends_with(&format!(".{}", ext)) {
            return false;
        }
    }

    // Skip hidden files and directories
    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            if let Some(name_str) = name.to_str() {
                if name_str.starts_with('.') {
                    return false;
                }
            }
        }
    }

    true
}

/// A code search result.
#[derive(Debug, serde::Serialize)]
pub struct CodeSearchResult {
    pub repo_id: i64,
    pub file_path: String,
    pub file_name: String,
    pub language: String,
    pub snippet: String,
}

/// Code indexer service.
pub struct CodeIndexer {
    db: DatabaseConnection,
}

impl CodeIndexer {
    /// Create a new code indexer.
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Index a repository by traversing its Git tree.
    ///
    /// # Arguments
    ///
    /// * `repo_id` - The repository ID in the database
    /// * `repo_path` - Path to the bare Git repository on disk
    /// * `ref_name` - The ref to index (e.g., "main", "master")
    pub async fn index_repository(
        &self,
        repo_id: i64,
        repo_path: &Path,
        ref_name: &str,
    ) -> Result<usize> {
        let repo = gix::open(repo_path)
            .with_context(|| format!("Failed to open repository: {}", repo_path.display()))?;

        // Resolve the ref to a commit OID
        let commit_id = repo
            .rev_parse_single(ref_name)
            .with_context(|| format!("Failed to resolve ref: {}", ref_name))?;

        // Find the commit
        let commit = repo
            .find_commit(commit_id)
            .with_context(|| format!("Failed to find commit: {}", commit_id))?;

        // Get the tree OID from the commit
        let decoded = commit
            .decode()
            .with_context(|| "Failed to decode commit".to_string())?;
        let tree_oid = decoded.tree();

        // Find the tree
        let tree = repo
            .find_tree(tree_oid)
            .with_context(|| format!("Failed to find tree: {}", tree_oid))?;

        // Clear existing index for this repo
        self.clear_index_for_repo(repo_id).await?;

        // Traverse the tree and index files
        let mut count = 0usize;
        let mut visited = HashSet::new();
        self.index_tree(&repo, &tree, repo_id, PathBuf::new(), &mut count, &mut visited)
            .await?;

        Ok(count)
    }

    /// Clear existing index for a repository.
    async fn clear_index_for_repo(&self, repo_id: i64) -> Result<()> {
        let sql = format!(
            "DELETE FROM code_fts WHERE repo_id = {}",
            repo_id
        );
        self.db
            .execute(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                sql,
            ))
            .await?;
        Ok(())
    }

    /// Index a Git tree iteratively (non-recursive to avoid async recursion issues).
    async fn index_tree(
        &self,
        repo: &gix::Repository,
        tree: &gix::Tree<'_>,
        repo_id: i64,
        base_path: PathBuf,
        count: &mut usize,
        visited: &mut HashSet<gix::ObjectId>,
    ) -> Result<()> {
        // Use a work stack for iterative DFS: (tree_oid, base_path)
        let mut stack: Vec<(gix::ObjectId, PathBuf)> = Vec::new();
        
        // Process initial tree
        self.process_tree_entries(repo, tree, repo_id, base_path, count, visited, &mut stack)
            .await?;
        
        // Process remaining trees from the stack
        while let Some((tree_oid, path)) = stack.pop() {
            if let Ok(object) = repo.find_object(tree_oid) {
                if let Ok(tree) = object.try_into_tree() {
                    self.process_tree_entries(repo, &tree, repo_id, path, count, visited, &mut stack)
                        .await?;
                }
            }
        }
        
        Ok(())
    }
    
    /// Process all entries in a single tree.
    async fn process_tree_entries(
        &self,
        repo: &gix::Repository,
        tree: &gix::Tree<'_>,
        repo_id: i64,
        base_path: PathBuf,
        count: &mut usize,
        visited: &mut HashSet<gix::ObjectId>,
        stack: &mut Vec<(gix::ObjectId, PathBuf)>,
    ) -> Result<()> {
        for item in tree.iter() {
            let item = match item {
                Ok(i) => i,
                Err(_) => continue,
            };

            let name = String::from_utf8_lossy(item.filename());
            let path = base_path.join(name.as_ref());

            let mode = item.mode();

            if mode.is_tree() {
                // Add subdirectory to stack for later processing
                let oid = item.oid().to_owned();
                if !visited.contains(&oid) {
                    visited.insert(oid.clone());
                    stack.push((oid, path));
                }
            } else if mode.is_blob() || mode.is_executable() {
                // Index file content
                let oid = item.oid().to_owned();
                if let Ok(object) = repo.find_object(oid) {
                    if let Ok(blob) = object.try_into_blob() {
                        let content = &blob.data;
                        if should_index(&path, content) {
                            let file_path = path.to_string_lossy().to_string();
                            let file_name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("")
                                .to_string();
                            let language = infer_language(&path);
                            let content_str = String::from_utf8_lossy(content).to_string();

                            self.insert_into_fts(
                                repo_id,
                                &file_path,
                                &file_name,
                                &content_str,
                                &language,
                            )
                            .await?;
                            *count += 1;
                        }
                    }
                }
            } else {
                // Skip other entry types (symlink, submodule, etc.)
            }
        }
        Ok(())
    }

    /// Insert a file into the FTS5 index.
    async fn insert_into_fts(
        &self,
        repo_id: i64,
        file_path: &str,
        file_name: &str,
        content: &str,
        language: &str,
    ) -> Result<()> {
        // Escape single quotes in strings for SQL
        let escape = |s: &str| s.replace('\'', "''");

        let sql = format!(
            "INSERT INTO code_fts(repo_id, file_path, file_name, content, language) VALUES({}, '{}', '{}', '{}', '{}')",
            repo_id,
            escape(file_path),
            escape(file_name),
            escape(content),
            escape(language)
        );

        self.db
            .execute(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                sql,
            ))
            .await?;

        Ok(())
    }

    /// Search code using FTS5.
    ///
    /// # Arguments
    ///
    /// * `query` - The search query (supports FTS5 syntax)
    /// * `repo_id` - Optional repository ID to filter by
    /// * `limit` - Maximum number of results
    /// * `offset` - Offset for pagination
    pub async fn search_code(
        &self,
        query: &str,
        repo_id: Option<i64>,
        limit: u64,
        offset: u64,
    ) -> Result<(Vec<CodeSearchResult>, i64)> {
        let safe_query = fts_escape(query);

        // Build WHERE clause
        let repo_filter = if let Some(rid) = repo_id {
            format!("repo_id = {}", rid)
        } else {
            "1".to_string()
        };

        // Get total count
        let count_sql = format!(
            "SELECT COUNT(*) as cnt FROM code_fts WHERE code_fts MATCH '{}' AND {}",
            safe_query, repo_filter
        );
        let count_result = self
            .db
            .query_one(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                count_sql,
            ))
            .await?
            .with_context(|| "Failed to get count")?;
        let total: i64 = count_result.try_get_by_index(0)?;

        // Get results with snippet
        let results_sql = format!(
            "SELECT rank, repo_id, file_path, file_name, language, snippet(code_fts, 3, '<b>', '</b>', '...', 20) as snippet \
             FROM code_fts \
             WHERE code_fts MATCH '{}' AND {} \
             ORDER BY rank \
             LIMIT {} OFFSET {}",
            safe_query, repo_filter, limit, offset
        );
        let rows = self
            .db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                results_sql,
            ))
            .await?;

        let results = rows
            .into_iter()
            .map(|row| {
                Ok(CodeSearchResult {
                    repo_id: row.try_get_by_index(1)?,
                    file_path: row.try_get_by_index(2)?,
                    file_name: row.try_get_by_index(3)?,
                    language: row.try_get_by_index(4)?,
                    snippet: row.try_get_by_index(5)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok((results, total))
    }
}

/// Escape a query string for FTS5.
///
/// Wraps the query in double quotes and escapes embedded double quotes.
fn fts_escape(query: &str) -> String {
    let escaped = query.replace('"', "\"\"");
    format!("\"{}\"", escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_language() {
        assert_eq!(infer_language(Path::new("main.rs")), "Rust");
        assert_eq!(infer_language(Path::new("app.js")), "JavaScript");
        assert_eq!(infer_language(Path::new("README.md")), "Markdown");
        assert_eq!(infer_language(Path::new("unknown.xyz")), "Text");
    }

    #[test]
    fn test_should_index() {
        assert!(should_index(Path::new("src/main.rs"), b"fn main() {}"));
        assert!(!should_index(Path::new("image.png"), &[0u8; 100]));
        assert!(!should_index(Path::new("large_file.rs"), &[0u8; 2_000_000]));
    }

    #[test]
    fn test_fts_escape() {
        assert_eq!(fts_escape("hello world"), "\"hello world\"");
        assert_eq!(fts_escape("say \"hello\""), "\"say \"\"hello\"\"\"");
    }
}
