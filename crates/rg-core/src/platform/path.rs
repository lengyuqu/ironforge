//! Cross-platform path handling utilities.

use std::path::PathBuf;

/// Get the platform-appropriate temp directory.
///
/// # Examples
/// - Unix: `/tmp` or `$TMPDIR`
/// - Windows: `C:\Users\Username\AppData\Local\Temp`
pub fn temp_dir() -> PathBuf {
    std::env::temp_dir()
}

/// Create a platform-appropriate PathBuf for repository storage.
///
/// # Unix
/// `/tmp/ironforge/repos/{owner}/{repo}.git`
///
/// # Windows
/// `C:\Users\Username\AppData\Local\Temp\ironforge\repos\{owner}\{repo}.git`
pub fn repo_path(owner: &str, repo: &str) -> PathBuf {
    let mut path = temp_dir();
    path.push("ironforge");
    path.push("repos");
    path.push(owner);
    path.push(format!("{}.git", repo));
    path
}

/// Expand `~` to the user's home directory (cross-platform).
///
/// # Errors
/// Returns the original path if `~` cannot be expanded.
pub fn expand_home(path: &str) -> String {
    if !path.starts_with("~") {
        return path.to_string();
    }

    match home::home_dir() {
        Some(home) => {
            let home_str = home.to_string_lossy();
            path.replacen("~", &home_str, 1)
        }
        None => path.to_string(),
    }
}

/// Convert a path to platform-appropriate string representation.
pub fn to_platform_string(path: &std::path::Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temp_dir() {
        let dir = temp_dir();
        assert!(dir.is_absolute());
    }

    #[test]
    fn test_repo_path() {
        let path = repo_path("testowner", "testrepo");
        assert!(path.to_string_lossy().contains("ironforge"));
        assert!(path.to_string_lossy().contains("repos"));
        assert!(path.to_string_lossy().contains("testowner"));
        assert!(path.to_string_lossy().contains("testrepo.git"));
    }

    #[test]
    fn test_expand_home() {
        let result = expand_home("~/.ironforge/config.toml");
        assert!(!result.starts_with('~'));
        assert!(result.contains(".ironforge"));
    }

    #[test]
    fn test_expand_home_no_tilde() {
        let result = expand_home("/absolute/path/config.toml");
        assert_eq!(result, "/absolute/path/config.toml");
    }
}
