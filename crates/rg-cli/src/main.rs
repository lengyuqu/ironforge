//! IronForge CLI — main entry point.
use std::net::IpAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Subcommand)]
enum PackageCmd {
    /// Publish a package file
    Publish {
        /// Package type: cargo, npm, generic, etc.
        pkg_type: String,

        /// Package name
        name: String,

        /// Version string
        version: String,

        /// Path to the package file to upload
        file: String,

        /// Owner of the target repository
        #[arg(long)]
        owner: String,

        /// Repository name
        #[arg(long)]
        repo: String,

        /// Access token for authentication (skips JWT auth)
        #[arg(long)]
        token: Option<String>,

        /// IronForge server URL (for token-based auth)
        #[arg(long, default_value = "http://localhost:8080")]
        server_url: String,
    },

    /// List packages in a repository registry
    List {
        /// Owner of the repository
        owner: String,

        /// Repository name
        repo: String,

        /// Package type to list
        pkg_type: String,

        /// Database URL for direct DB access (SQLite, PostgreSQL, or MySQL)
        #[arg(long, default_value = "sqlite://./ironforge.db?mode=rwc")]
        db_url: String,
    },
}

#[derive(Parser)]
#[command(name = "ironforge", about = "A Git hosting platform written in Rust")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Start the IronForge server
    Serve {
        /// Root directory for git repositories
        #[arg(long, default_value = "./repos")]
        repo_root: String,

        /// HTTP listen address
        #[arg(long, default_value = "0.0.0.0:8080")]
        http_addr: String,

        /// SSH listen address
        #[arg(long, default_value = "0.0.0.0:2222")]
        ssh_addr: String,

        /// Path to SSH host key
        #[arg(long)]
        host_key: Option<String>,

        /// Database URL (sqlite://, postgres://, or mysql://)
        #[arg(long, default_value = "sqlite://./ironforge.db?mode=rwc")]
        db_url: String,

        /// JWT secret key (use a long random string in production)
        #[arg(long)]
        jwt_secret: Option<String>,

        /// Enable Docker runner for CI jobs with `image` field
        #[arg(long, default_value_t = false)]
        docker: bool,

        /// Use external runners instead of embedded runner for CI jobs
        #[arg(long, default_value_t = false)]
        external_runners: bool,

        /// Allow CI jobs without an `image` to run directly on the host via `sh -c`.
        ///
        /// This is a security-sensitive opt-in: the job script is repository
        /// content, so host execution grants its author code execution as the
        /// server user. Off by default.
        #[arg(long, default_value_t = false)]
        allow_host_execution: bool,

        /// Rate limit: max requests per window per IP (0 = disabled)
        #[arg(long, default_value_t = 0)]
        rate_limit_max: u32,

        /// Rate limit: window duration in seconds
        #[arg(long, default_value_t = 60)]
        rate_limit_window: u64,

        /// Comma-separated proxy IPs whose X-Forwarded-For / X-Real-IP headers are trusted
        #[arg(long, value_delimiter = ',')]
        rate_limit_trusted_proxies: Vec<String>,

        /// SMTP server host (enables email notifications)
        #[arg(long)]
        smtp_host: Option<String>,

        /// SMTP server port
        #[arg(long, default_value_t = 587)]
        smtp_port: u16,

        /// SMTP username
        #[arg(long)]
        smtp_user: Option<String>,

        /// SMTP password
        #[arg(long)]
        smtp_pass: Option<String>,

        /// SMTP from email address
        #[arg(long)]
        smtp_from: Option<String>,

        /// Path to TLS certificate file (PEM format, enables HTTPS)
        #[arg(long)]
        tls_cert: Option<String>,

        /// Path to TLS private key file (PEM format)
        #[arg(long)]
        tls_key: Option<String>,

        /// Path to TOML configuration file (overrides CLI defaults)
        #[arg(long)]
        config: Option<String>,

        /// Log file path (enables file logging with rotation). If not set, logs to stderr only.
        #[arg(long)]
        log_file: Option<String>,

        /// Log rotation: nominal max log file size in MB. NOTE: the file
        /// appender rotates daily, not by size — this value is advisory only.
        #[arg(long, default_value_t = 10)]
        log_max_size_mb: u64,

        /// Log rotation: max number of old log files to keep (default: 5)
        #[arg(long, default_value_t = 5)]
        log_max_files: usize,
    },

    /// Run database migrations and exit
    Migrate {
        /// Database URL (sqlite://, postgres://, or mysql://)
        #[arg(long, default_value = "sqlite://./ironforge.db?mode=rwc")]
        db_url: String,
    },

    /// Rebuild or refresh full-text search indexes from main tables
    RebuildFts {
        /// Database URL (sqlite://, postgres://, or mysql://)
        #[arg(long, default_value = "sqlite://./ironforge.db?mode=rwc")]
        db_url: String,
    },

    /// Re-publish `refs/pull/<n>/head` where it is missing or stale.
    ///
    /// Pull requests opened before the ref existed have no anchor until their
    /// head branch is pushed again; for a fork pull request that may never
    /// happen. Writes nothing but the ref, and is safe to re-run.
    RepairPullRefs {
        /// Database URL (sqlite://, postgres://, or mysql://)
        #[arg(long, default_value = "sqlite://./ironforge.db?mode=rwc")]
        db_url: String,

        /// Root directory for git repositories
        #[arg(long, default_value = "./repos")]
        repo_root: String,

        /// Only this repository (owner/name); every repository otherwise
        #[arg(long)]
        repo: Option<String>,

        /// Also repair merged and closed pull requests
        #[arg(long, default_value_t = false)]
        include_closed: bool,

        /// Print what would change without writing anything
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Create a consistent SQLite database backup.
    BackupDb {
        /// SQLite database URL (e.g. sqlite://./ironforge.db?mode=rwc)
        #[arg(long, default_value = "sqlite://./ironforge.db?mode=rwc")]
        db_url: String,

        /// Output backup file path.
        output: String,

        /// Overwrite output if it already exists.
        #[arg(long, default_value_t = false)]
        force: bool,
    },

    /// Restore a SQLite database file from a backup.
    RestoreDb {
        /// SQLite database URL to restore into.
        #[arg(long, default_value = "sqlite://./ironforge.db?mode=rwc")]
        db_url: String,

        /// Backup file path to restore from.
        input: String,

        /// Overwrite existing target DB and sidecar WAL/SHM files.
        #[arg(long, default_value_t = false)]
        force: bool,

        /// Skip the post-restore quiesce step (see `restore --help`).
        #[arg(long, default_value_t = false)]
        keep_live_state: bool,
    },

    /// Create a full-instance backup (OPS-501): database, git repositories,
    /// blob storage (LFS/packages/OCI/artifacts/attachments), audit archive
    /// and optional config file, into a single directory.
    ///
    /// Stop the IronForge server before running this command: the database
    /// snapshot and the file copy are taken sequentially, not atomically.
    Backup {
        /// Database URL (SQLite only; use pg_dump/mysqldump for other backends)
        #[arg(long, default_value = "sqlite://./ironforge.db?mode=rwc")]
        db_url: String,

        /// Root directory holding git repositories and blob storage
        #[arg(long, default_value = "./repos")]
        repo_root: String,

        /// Audit archive directory (skipped when it does not exist)
        #[arg(long, default_value = "./data/audit-archive")]
        audit_archive_dir: String,

        /// Optional TOML config file to include in the backup
        #[arg(long)]
        config: Option<String>,

        /// Output backup directory (must not exist unless --force)
        output: String,

        /// Overwrite an existing output directory.
        #[arg(long, default_value_t = false)]
        force: bool,
    },

    /// Restore a full-instance backup created by `ironforge backup` (OPS-501).
    ///
    /// Restores the database, repository/blob data and audit archive to the
    /// given targets. Run `ironforge migrate` afterwards if the restoring
    /// binary is newer than the one that took the backup.
    Restore {
        /// Database URL to restore into (SQLite only)
        #[arg(long, default_value = "sqlite://./ironforge.db?mode=rwc")]
        db_url: String,

        /// Root directory to restore repositories and blob storage into
        #[arg(long, default_value = "./repos")]
        repo_root: String,

        /// Audit archive restore target
        #[arg(long, default_value = "./data/audit-archive")]
        audit_archive_dir: String,

        /// Backup directory to restore from.
        input: String,

        /// Overwrite existing files at the restore targets.
        #[arg(long, default_value_t = false)]
        force: bool,

        /// Skip the post-restore quiesce step. By default a restore ends all
        /// collaboration sessions, deletes password-reset tokens and abandons
        /// queued webhook/e-mail jobs, because the restored state is older
        /// than the world and those would act on it (or repeat effects that
        /// already happened).
        #[arg(long, default_value_t = false)]
        keep_live_state: bool,
    },

    /// Create a new bare repository (no DB record — for quick testing)
    CreateRepo {
        /// Owner username
        owner: String,

        /// Repository name (without .git suffix)
        name: String,

        /// Root directory for repositories
        #[arg(long, default_value = "./repos")]
        repo_root: String,
    },

    /// Create a user directly in the database (bootstrap / ops; works even
    /// when open registration is disabled)
    CreateUser {
        username: String,

        #[arg(long)]
        email: String,

        /// Password value, or `-` to read one line from stdin
        #[arg(long)]
        password: Option<String>,

        #[arg(long)]
        admin: bool,

        /// Database URL (same one the server uses)
        #[arg(long, default_value = "sqlite://ironforge.db?mode=rwc")]
        db_url: String,
    },

    /// Run as a CI Runner — polls jobs and executes them
    Runner {
        /// IronForge server URL (e.g. http://127.0.0.1:8080)
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        server: String,

        /// Runner name (used for registration if not already registered)
        #[arg(long)]
        name: Option<String>,

        /// Existing runner ID (skip registration)
        #[arg(long)]
        runner_id: Option<i64>,

        /// Existing runner token (skip registration)
        #[arg(long)]
        token: Option<String>,

        /// Admin user JWT used only when this command needs to register a runner
        #[arg(long)]
        auth_token: Option<String>,
    },

    /// Import a repository from GitHub or GitLab
    Import {
        /// Source platform: "github" or "gitlab"
        #[arg(value_parser = ["github", "gitlab"])]
        platform: String,

        /// Source repository URL (e.g., https://github.com/user/repo)
        source_url: String,

        /// Target owner in IronForge
        #[arg(long)]
        target_owner: String,

        /// Target repository name (defaults to source repo name)
        #[arg(long)]
        target_name: Option<String>,

        /// API access token for the source platform
        #[arg(long)]
        token: Option<String>,

        /// Root directory for repositories
        #[arg(long, default_value = "./repos")]
        repo_root: String,

        /// Database URL (sqlite://, postgres://, or mysql://)
        #[arg(long, default_value = "sqlite://./ironforge.db?mode=rwc")]
        db_url: String,

        /// Skip importing the repository itself
        #[arg(long)]
        skip_repo: bool,

        /// Skip importing issues
        #[arg(long)]
        skip_issues: bool,

        /// Skip importing pull/merge requests
        #[arg(long)]
        skip_prs: bool,

        /// Skip importing labels
        #[arg(long)]
        skip_labels: bool,

        /// Skip importing milestones
        #[arg(long)]
        skip_milestones: bool,

        /// Skip importing releases
        #[arg(long)]
        skip_releases: bool,

        /// Also import wiki pages
        #[arg(long)]
        import_wiki: bool,
    },

    /// Index a repository for code search
    IndexRepo {
        /// Repository to index, in the format "owner/name"
        repo_slug: String,

        /// Root directory for repositories
        #[arg(long, default_value = "./repos")]
        repo_root: String,

        /// Database URL (sqlite://, postgres://, or mysql://)
        #[arg(long, default_value = "sqlite://./ironforge.db?mode=rwc")]
        db_url: String,

        /// Git ref to index (default: repository's default branch)
        #[arg(long)]
        ref_name: Option<String>,
    },

    /// Manage package registry
    Package {
        #[command(subcommand)]
        cmd: PackageCmd,
    },
}

/// TOML configuration file structure.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ConfigFile {
    #[serde(default)]
    server: ServerConfig,
    #[serde(default)]
    database: DatabaseConfig,
    #[serde(default)]
    auth: AuthConfig,
    #[serde(default)]
    ci: CiConfig,
    #[serde(default)]
    rate_limit: RateLimitConfig,
    #[serde(default)]
    smtp: SmtpConfig,
    #[serde(default)]
    tls: TlsConfig,
    #[serde(default)]
    logging: LoggingConfig,
    #[serde(default)]
    audit: AuditConfig,
    #[serde(default)]
    timeouts: TimeoutConfig,
    /// Server external URL (e.g., "https://git.example.com"). Used for SSO callbacks.
    #[serde(default)]
    external_url: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
#[allow(dead_code)]
struct ServerConfig {
    repo_root: Option<String>,
    http_addr: Option<String>,
    ssh_addr: Option<String>,
    host_key: Option<String>,
    /// External-facing URL for SSO callbacks and links (e.g., "https://git.example.com")
    external_url: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
#[allow(dead_code)]
struct DatabaseConfig {
    url: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
#[allow(dead_code)]
struct AuthConfig {
    jwt_secret: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct CiConfig {
    #[serde(default)]
    docker: Option<bool>,
    #[serde(default)]
    external_runners: Option<bool>,
    /// Allow CI jobs without an `image` to run on the host. Off by default.
    #[serde(default)]
    allow_host_execution: Option<bool>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct RateLimitConfig {
    max: Option<u32>,
    window_secs: Option<u64>,
    #[serde(default)]
    trusted_proxies: Vec<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct SmtpConfig {
    host: Option<String>,
    port: Option<u16>,
    user: Option<String>,
    pass: Option<String>,
    from: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct TlsConfig {
    cert: Option<String>,
    key: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct LoggingConfig {
    file: Option<String>,
    max_size_mb: Option<u64>,
    max_files: Option<usize>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct AuditConfig {
    enabled: Option<bool>,
    archive_dir: Option<String>,
    archive_after_days: Option<i64>,
    interval_minutes: Option<u64>,
    batch_size: Option<u64>,
}

#[derive(Debug, serde::Deserialize)]
struct TimeoutConfig {
    /// CI job timeout in seconds (default: 3600 = 1 hour).
    #[serde(default = "default_job_timeout")]
    job_secs: u64,
    /// Git CLI command timeout in seconds (default: 120).
    #[serde(default = "default_git_timeout")]
    git_cmd_secs: u64,
    /// Database connect timeout in seconds (default: 10).
    #[serde(default = "default_db_connect_timeout")]
    db_connect_secs: u64,
    /// Database idle timeout in seconds (default: 600).
    #[serde(default = "default_db_idle_timeout")]
    db_idle_secs: u64,
}

// `#[serde(default)]` on a whole `[timeouts]` section falls back to `Default`,
// not to the per-field defaults above, so this must spell them out: a derived
// `Default` made a config file with no `[timeouts]` section mean 0-second
// database and job timeouts (the server could not even connect to its database).
impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            job_secs: default_job_timeout(),
            git_cmd_secs: default_git_timeout(),
            db_connect_secs: default_db_connect_timeout(),
            db_idle_secs: default_db_idle_timeout(),
        }
    }
}

fn default_job_timeout() -> u64 {
    3600
}
fn default_git_timeout() -> u64 {
    120
}
fn default_db_connect_timeout() -> u64 {
    10
}
fn default_db_idle_timeout() -> u64 {
    600
}

fn load_config_file(path: &str) -> anyhow::Result<ConfigFile> {
    let content = std::fs::read_to_string(path)?;
    let config: ConfigFile = toml::from_str(&content)?;
    tracing::info!(path = %path, "Loaded configuration file");
    Ok(config)
}

fn parse_rate_limit_trusted_proxies(values: &[String]) -> anyhow::Result<Vec<IpAddr>> {
    values
        .iter()
        .map(|value| {
            value
                .parse::<IpAddr>()
                .with_context(|| format!("invalid rate_limit trusted proxy IP: {value}"))
        })
        .collect()
}

/// First seven characters of a commit id, for human-readable output.
fn short_sha(sha: &str) -> &str {
    let end = sha.len().min(7);
    sha.get(..end).unwrap_or(sha)
}

async fn backup_sqlite_db(db_url: &str, output: &PathBuf, force: bool) -> anyhow::Result<()> {
    if !db_url.starts_with("sqlite:") {
        anyhow::bail!(
            "database backup is only supported for the SQLite backend in this version; \
             use your PostgreSQL/MySQL server's native dump tool (pg_dump / mysqldump) for other backends"
        );
    }
    if output.exists() && !force {
        anyhow::bail!(
            "backup output already exists: {} (use --force to overwrite)",
            output.display()
        );
    }
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create backup directory: {}", parent.display())
            })?;
        }
    }
    if output.exists() {
        std::fs::remove_file(output)
            .with_context(|| format!("failed to remove existing backup: {}", output.display()))?;
    }

    tracing::info!(db_url = %rg_db::redact_database_url(db_url), output = %output.display(), "Creating SQLite backup");
    let db = rg_db::connect(db_url).await?;
    let output_str = output
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("backup output path is not valid UTF-8"))?;

    use rg_db::sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "VACUUM INTO ?",
        [output_str.into()],
    ))
    .await
    .context("SQLite VACUUM INTO backup failed")?;

    tracing::info!(output = %output.display(), "SQLite backup complete");
    println!("Backup written to {}", output.display());
    Ok(())
}

fn restore_sqlite_db(db_url: &str, input: &PathBuf, force: bool) -> anyhow::Result<()> {
    if !input.exists() {
        anyhow::bail!("backup input does not exist: {}", input.display());
    }
    if !input.is_file() {
        anyhow::bail!("backup input is not a file: {}", input.display());
    }

    let target = sqlite_db_path_from_url(db_url)?;
    if target.exists() && !force {
        anyhow::bail!(
            "target database already exists: {} (stop IronForge and use --force to overwrite)",
            target.display()
        );
    }
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create database directory: {}", parent.display())
            })?;
        }
    }

    if force {
        remove_sqlite_sidecar_files(&target)?;
    }
    std::fs::copy(input, &target).with_context(|| {
        format!(
            "failed to restore backup {} to {}",
            input.display(),
            target.display()
        )
    })?;

    tracing::info!(input = %input.display(), target = %target.display(), "SQLite restore complete");
    println!("Restored {} to {}", input.display(), target.display());
    Ok(())
}

fn remove_sqlite_sidecar_files(db_path: &std::path::Path) -> anyhow::Result<()> {
    for suffix in ["", "-wal", "-shm"] {
        let path = PathBuf::from(format!("{}{}", db_path.display(), suffix));
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }
    Ok(())
}

// ── Full-instance backup / restore (OPS-501) ─────────────────────────────

/// Backup directory layout:
///
/// ```text
/// <output>/
///   manifest.json      # format version, timestamp, contents summary
///   ironforge.db       # SQLite snapshot (VACUUM INTO)
///   data/              # repo_root copy: git repos + LFS/packages/OCI/
///                       # artifacts/attachments blob storage
///   audit-archive/     # audit NDJSON.zst archives (when present)
///   config/            # optional TOML config file copy
/// ```
const BACKUP_FORMAT_VERSION: u64 = 1;

/// Per-file SHA-256 list written next to the manifest (`sha256sum` format, paths
/// relative to the backup directory, `/`-separated). Everything in the backup
/// except the manifest and this file is listed.
const CHECKSUMS_FILE: &str = "checksums.sha256";

fn sha256_hex(path: &std::path::Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Every regular file under `root` except the manifest and the checksum list,
/// as `/`-separated relative paths, sorted.
fn backup_payload_files(root: &std::path::Path) -> anyhow::Result<Vec<String>> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("failed to read directory {}", dir.display()))?
        {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                stack.push(entry.path());
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(root)?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if rel != "manifest.json" && rel != CHECKSUMS_FILE {
                found.push(rel);
            }
        }
    }
    found.sort();
    Ok(found)
}

fn write_backup_checksums(root: &std::path::Path) -> anyhow::Result<usize> {
    let files = backup_payload_files(root)?;
    let mut text = String::new();
    for rel in &files {
        text.push_str(&format!("{}  {rel}\n", sha256_hex(&root.join(rel))?));
    }
    std::fs::write(root.join(CHECKSUMS_FILE), text)?;
    Ok(files.len())
}

/// Run `PRAGMA integrity_check` on a copy of a SQLite file and make sure every
/// migration it recorded is known to this binary. Works on a temporary copy so
/// the backup itself is never opened for writing.
async fn verify_sqlite_file(path: &std::path::Path) -> anyhow::Result<()> {
    use rg_db::sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let tmp = std::env::temp_dir().join(format!(
        "ironforge-verify-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::copy(path, &tmp)
        .with_context(|| format!("failed to copy {} for verification", path.display()))?;
    let result = async {
        let url = format!("sqlite://{}?mode=rw", tmp.display());
        let db = rg_db::connect(&url)
            .await
            .context("the database file in the backup cannot be opened")?;
        let rows = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "PRAGMA integrity_check".to_string(),
            ))
            .await
            .context("the database file in the backup failed its integrity check")?;
        let verdicts: Vec<String> = rows
            .iter()
            .filter_map(|r| r.try_get_by_index::<String>(0).ok())
            .collect();
        if verdicts != ["ok"] {
            anyhow::bail!(
                "the database file in the backup is damaged (integrity_check: {})",
                verdicts.join("; ")
            );
        }
        let recorded = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT version FROM seaql_migrations".to_string(),
            ))
            .await
            .context("the backup database has no migration history")?;
        let known = rg_db::known_migration_names();
        let unknown: Vec<String> = recorded
            .iter()
            .filter_map(|r| r.try_get_by_index::<String>(0).ok())
            .filter(|name| !known.contains(name))
            .collect();
        if !unknown.is_empty() {
            anyhow::bail!(
                "the backup was written by a newer IronForge (unknown migrations: {}); \
                 upgrade this binary before restoring",
                unknown.join(", ")
            );
        }
        anyhow::Ok(())
    }
    .await;
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", tmp.display()));
    }
    result
}

/// Check a whole backup directory BEFORE any restore write: manifest, every
/// file's checksum (and that nothing unlisted is present), and the database's
/// integrity and schema compatibility. A backup from before checksums existed
/// is still accepted, with a warning that its files could not be verified.
async fn verify_backup(input: &std::path::Path) -> anyhow::Result<()> {
    let list_path = input.join(CHECKSUMS_FILE);
    if list_path.is_file() {
        let listed = std::fs::read_to_string(&list_path)?;
        let mut expected = std::collections::BTreeMap::new();
        for line in listed.lines().filter(|l| !l.trim().is_empty()) {
            let (digest, rel) = line
                .split_once("  ")
                .ok_or_else(|| anyhow::anyhow!("malformed line in {CHECKSUMS_FILE}: {line}"))?;
            expected.insert(rel.to_string(), digest.to_string());
        }
        let present = backup_payload_files(input)?;
        for rel in &present {
            if !expected.contains_key(rel) {
                anyhow::bail!("backup contains a file that is not in {CHECKSUMS_FILE}: {rel}");
            }
        }
        for (rel, digest) in &expected {
            let path = input.join(rel);
            if !path.is_file() {
                anyhow::bail!("backup is missing a file listed in {CHECKSUMS_FILE}: {rel}");
            }
            if &sha256_hex(&path)? != digest {
                anyhow::bail!("backup file is damaged (checksum mismatch): {rel}");
            }
        }
        println!("Verified {} files against {CHECKSUMS_FILE}", expected.len());
    } else {
        eprintln!(
            "warning: this backup has no {CHECKSUMS_FILE} (taken by an older version); \
             its files could not be verified for corruption"
        );
    }
    let db_file = input.join("ironforge.db");
    if !db_file.is_file() {
        anyhow::bail!("backup is missing the database file: {}", db_file.display());
    }
    verify_sqlite_file(&db_file).await
}

/// Create a full-instance backup. The database snapshot and file copy are
/// sequential — the server must be stopped for a consistent backup.
async fn backup_instance(
    db_url: &str,
    repo_root: &std::path::Path,
    audit_archive_dir: &std::path::Path,
    config: Option<&std::path::Path>,
    output: &std::path::Path,
    force: bool,
) -> anyhow::Result<()> {
    if !db_url.starts_with("sqlite:") {
        anyhow::bail!(
            "full-instance backup currently supports the SQLite backend only; \
             for PostgreSQL/MySQL dump the database with pg_dump / mysqldump and \
             back up the repository root alongside it"
        );
    }
    if output.exists() && !force {
        anyhow::bail!(
            "backup output already exists: {} (use --force to overwrite)",
            output.display()
        );
    }
    if !repo_root.is_dir() {
        anyhow::bail!("repo root not found: {}", repo_root.display());
    }

    if output.exists() {
        std::fs::remove_dir_all(output)
            .with_context(|| format!("failed to remove existing backup: {}", output.display()))?;
    }
    std::fs::create_dir_all(output)
        .with_context(|| format!("failed to create backup directory: {}", output.display()))?;

    // 1. Database snapshot
    let db_backup = output.join("ironforge.db");
    backup_sqlite_db(db_url, &db_backup, true).await?;

    // 2. Repository root (git repos + all blob storage)
    let files = copy_dir_recursive(repo_root, &output.join("data"))
        .with_context(|| format!("failed to copy repo root {}", repo_root.display()))?;
    tracing::info!(files, "repo root copied");

    // 3. Audit archive (optional — only when the directory exists)
    let mut includes_audit = false;
    if audit_archive_dir.is_dir() {
        copy_dir_recursive(audit_archive_dir, &output.join("audit-archive")).with_context(
            || {
                format!(
                    "failed to copy audit archive {}",
                    audit_archive_dir.display()
                )
            },
        )?;
        includes_audit = true;
    }

    // 4. Config file (optional)
    let mut config_name: Option<String> = None;
    if let Some(config_path) = config {
        if !config_path.is_file() {
            anyhow::bail!("config file not found: {}", config_path.display());
        }
        let name = config_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("config file name is not valid UTF-8"))?;
        let target_dir = output.join("config");
        std::fs::create_dir_all(&target_dir)?;
        std::fs::copy(config_path, target_dir.join(name))?;
        config_name = Some(name.to_string());
    }

    // 5. Checksums of everything copied, then the manifest.
    let checksummed = write_backup_checksums(output)?;
    tracing::info!(files = checksummed, "backup checksums written");

    let manifest = serde_json::json!({
        "format_version": BACKUP_FORMAT_VERSION,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "db_backend": "sqlite",
        "includes": {
            "database": true,
            "repo_root": true,
            "audit_archive": includes_audit,
            "config": config_name,
            "checksums": CHECKSUMS_FILE,
        },
    });
    std::fs::write(
        output.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;

    println!(
        "Full-instance backup written to {} (db + repo root{}{})",
        output.display(),
        if includes_audit {
            " + audit archive"
        } else {
            ""
        },
        if config_name.is_some() {
            " + config"
        } else {
            ""
        },
    );
    println!(
        "Restore with: ironforge restore --input {}",
        output.display()
    );
    Ok(())
}

/// Restore a full-instance backup created by [`backup_instance`].
fn restore_instance(
    input: &std::path::Path,
    db_url: &str,
    repo_root: &std::path::Path,
    audit_archive_dir: &std::path::Path,
    force: bool,
) -> anyhow::Result<()> {
    if !input.is_dir() {
        anyhow::bail!("backup directory does not exist: {}", input.display());
    }

    // Validate the manifest before touching any target
    let manifest_path = input.join("manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)
            .with_context(|| format!("invalid manifest: {}", manifest_path.display()))?;
    let format_version = manifest["format_version"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("backup manifest is missing format_version"))?;
    if format_version > BACKUP_FORMAT_VERSION {
        anyhow::bail!(
            "backup format version {format_version} is newer than this binary supports \
             ({}); upgrade IronForge before restoring",
            BACKUP_FORMAT_VERSION
        );
    }

    // 1. Database
    let db_backup = input.join("ironforge.db");
    if !db_backup.is_file() {
        anyhow::bail!(
            "backup is missing the database file: {}",
            db_backup.display()
        );
    }

    // 2. Repository root / blob storage
    let data_dir = input.join("data");
    if !data_dir.is_dir() {
        anyhow::bail!(
            "backup is missing the data directory: {}",
            data_dir.display()
        );
    }

    // Pre-flight: validate every target BEFORE the first destructive write so
    // a rejected restore leaves the instance untouched.
    if !force {
        let target_db = sqlite_db_path_from_url(db_url)?;
        if target_db.exists() {
            anyhow::bail!(
                "target database already exists: {} (stop IronForge and use --force to overwrite)",
                target_db.display()
            );
        }
        if repo_root.exists()
            && std::fs::read_dir(repo_root)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(false)
        {
            anyhow::bail!(
                "repo root is not empty: {} (use --force to overwrite existing files)",
                repo_root.display()
            );
        }
    }

    // Restore database first, then files.
    restore_sqlite_db(db_url, &db_backup, force)?;

    copy_dir_recursive(&data_dir, repo_root)
        .with_context(|| format!("failed to restore data to {}", repo_root.display()))?;

    // 3. Audit archive (only when the backup contains one)
    let audit_src = input.join("audit-archive");
    if audit_src.is_dir() {
        copy_dir_recursive(&audit_src, audit_archive_dir).with_context(|| {
            format!(
                "failed to restore audit archive to {}",
                audit_archive_dir.display()
            )
        })?;
    }

    println!("Restored full instance to:");
    println!("  database : {db_url}");
    println!("  repo root: {}", repo_root.display());
    println!("Next steps:");
    println!("  1. run `ironforge migrate --db-url {db_url}` (no-op when schema matches)");
    println!("  2. start the server against the restored paths");
    Ok(())
}

/// What the post-restore quiesce step changed.
#[derive(Debug, Default, PartialEq, Eq)]
struct QuiesceReport {
    sessions_ended: u64,
    reset_tokens_deleted: u64,
    jobs_abandoned: u64,
    /// Pending/running CI pipelines cancelled (their stages and jobs are failed).
    pipelines_cancelled: u64,
    /// Access tokens present in the restored database. They are NOT touched:
    /// the backup cannot tell which ones were revoked after it was taken.
    access_tokens_restored: u64,
}

impl QuiesceReport {
    fn print(&self) {
        println!("Post-restore quiesce (the restored state is older than the world):");
        println!(
            "  collaboration sessions ended : {} (clients must register new sessions)",
            self.sessions_ended
        );
        println!(
            "  password-reset tokens deleted: {}",
            self.reset_tokens_deleted
        );
        println!(
            "  queued webhook/e-mail jobs abandoned (marked dead, last_error says why): {}",
            self.jobs_abandoned
        );
        println!(
            "  pending/running CI pipelines cancelled: {}",
            self.pipelines_cancelled
        );
        println!(
            "  access tokens restored UNCHANGED: {} - any token revoked after this backup is \
             valid again; review or rotate them",
            self.access_tokens_restored
        );
    }
}

/// Make a freshly restored database safe to start: it is a snapshot of the
/// past, so anything that would act on stale state is stopped.
///
/// * collaboration sessions are ended (they are labels clients can re-create,
///   and a restored secret may belong to a session that was ended since);
/// * password-reset tokens are deleted (short-lived, cheap to re-request);
/// * queued `webhook.deliver` / `email.send` jobs are marked `dead` instead of
///   run: they may already have been delivered from the live instance, and a
///   second delivery cannot be taken back;
/// * pending/running CI pipelines are cancelled and their stages/jobs failed, so a
///   restored runner does not re-run work (deploys included) that already ran;
/// * access tokens are only counted - revoking all of them would break every
///   integration, and the backup cannot say which were revoked later.
///
/// Tables a (possibly older) backup does not have are skipped.
async fn quiesce_restored_database(db_url: &str) -> anyhow::Result<QuiesceReport> {
    use rg_db::sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let path = sqlite_db_path_from_url(db_url)?;
    let db = rg_db::connect(&format!("sqlite://{}?mode=rw", path.display()))
        .await
        .context("cannot open the restored database to quiesce it")?;
    let sql = |q: String| Statement::from_string(DatabaseBackend::Sqlite, q);
    let has_table = |name: &'static str| {
        let db = &db;
        async move {
            let rows = db
                .query_all(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    format!("SELECT 1 FROM sqlite_master WHERE type='table' AND name='{name}'"),
                ))
                .await?;
            anyhow::Ok(!rows.is_empty())
        }
    };
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut report = QuiesceReport::default();

    if has_table("agent_sessions").await? {
        report.sessions_ended = db
            .execute(sql(format!(
                "UPDATE agent_sessions SET ended_at = '{now}' WHERE ended_at IS NULL"
            )))
            .await
            .context("ending collaboration sessions")?
            .rows_affected();
    }
    if has_table("password_reset_tokens").await? {
        report.reset_tokens_deleted = db
            .execute(sql("DELETE FROM password_reset_tokens".to_string()))
            .await
            .context("deleting password-reset tokens")?
            .rows_affected();
    }
    if has_table("background_jobs").await? {
        report.jobs_abandoned = db
            .execute(sql(format!(
                "UPDATE background_jobs SET status = 'dead', locked_by = NULL, locked_at = NULL, \
                 last_error = 'abandoned at restore: may already have been delivered', \
                 updated_at = '{now}' \
                 WHERE status IN ('pending','running') \
                 AND task_type IN ('webhook.deliver','email.send')"
            )))
            .await
            .context("abandoning queued outbound jobs")?
            .rows_affected();
    }
    if has_table("pipelines").await? {
        // A restored runner would otherwise pick up (and re-run) work the live
        // instance already did, including deploy jobs. Same statements as
        // deploy/post-restore-quiesce.sql (a test keeps them in step).
        report.pipelines_cancelled = db
            .execute(sql(
                "UPDATE pipelines SET status = 'canceled', finished_at = CURRENT_TIMESTAMP \
                 WHERE status IN ('pending','running')"
                    .to_string(),
            ))
            .await
            .context("cancelling CI pipelines")?
            .rows_affected();
        for table in ["pipeline_stages", "pipeline_jobs"] {
            if has_table(table).await? {
                db.execute(sql(format!(
                    "UPDATE {table} SET status = 'failed', finished_at = CURRENT_TIMESTAMP \
                     WHERE status IN ('pending','running')"
                )))
                .await
                .with_context(|| format!("failing {table}"))?;
            }
        }
    }
    if has_table("access_tokens").await? {
        let rows = db
            .query_all(sql("SELECT COUNT(*) FROM access_tokens".to_string()))
            .await?;
        report.access_tokens_restored = rows
            .first()
            .and_then(|r| r.try_get_by_index::<i64>(0).ok())
            .unwrap_or(0)
            .max(0) as u64;
    }
    Ok(report)
}

/// Recursively copy a directory tree. Returns the number of files copied.
/// Symlinks are followed (content is copied).
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<u64> {
    std::fs::create_dir_all(dst)?;
    let mut count = 0u64;
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((source, target)) = stack.pop() {
        for entry in std::fs::read_dir(&source)
            .with_context(|| format!("failed to read directory {}", source.display()))?
        {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let entry_target = target.join(entry.file_name());
            if file_type.is_dir() {
                std::fs::create_dir_all(&entry_target)?;
                stack.push((entry.path(), entry_target));
            } else {
                std::fs::copy(entry.path(), &entry_target)?;
                count += 1;
            }
        }
    }
    Ok(count)
}

fn sqlite_db_path_from_url(db_url: &str) -> anyhow::Result<PathBuf> {
    let rest = db_url
        .strip_prefix("sqlite://")
        .ok_or_else(|| anyhow::anyhow!("only sqlite:// database URLs are supported"))?;
    let path_part = rest.split_once('?').map(|(path, _)| path).unwrap_or(rest);
    if path_part.is_empty() || path_part == ":memory:" || path_part == "/:memory:" {
        anyhow::bail!("restore requires a file-backed SQLite database URL");
    }
    Ok(PathBuf::from(path_part))
}

/// Secrets that must never be accepted, as SHA-256 digests.
///
/// These values are (or were) committed to the repository. Matching on the
/// digest rather than the literal keeps the repository from re-committing the
/// very secret it is trying to ban.
const DENYLISTED_JWT_SECRET_SHA256: &[(&str, &str)] = &[
    (
        // ironforge.example.toml placeholder.
        "92809ab604c9377285fbbcf82c4f30db65f107ecfd7217c7e62c17a2b3be7633",
        "the documented placeholder from ironforge.example.toml",
    ),
    (
        // Previously committed in ironforge.toml / ironforge-simple.toml.
        "b476e5c4d762e0860f117d8626c163068cc915d868a5b2220e03981fd951d341",
        "a secret previously committed to this repository (rotate it if it was ever used)",
    ),
];

/// Validate JWT secret strength.
/// Common function used for CLI arg, env var, and config file values.
fn validate_jwt_secret(jwt_secret: &str, source: &str) -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};

    let digest = hex::encode(Sha256::digest(jwt_secret.as_bytes()));
    if let Some((_, reason)) = DENYLISTED_JWT_SECRET_SHA256
        .iter()
        .find(|(hash, _)| *hash == digest)
    {
        tracing::error!(
            "FATAL: jwt_secret from {} is {} — refusing to start. \
             Generate a fresh secret, e.g. `openssl rand -base64 48`",
            source,
            reason
        );
        anyhow::bail!("refusing to start with a known-compromised jwt_secret");
    }
    if jwt_secret.len() < 32 {
        tracing::warn!(
            jwt_len = jwt_secret.len(),
            "jwt_secret from {} is shorter than 32 characters — consider using a stronger secret",
            source
        );
    }
    Ok(())
}

/// Validate critical configuration before starting servers.
/// Refuses to start with dangerous defaults or invalid settings.
fn validate_config(
    jwt_secret: &str,
    repo_root: &std::path::Path,
    tls_config: &Option<(PathBuf, PathBuf)>,
) -> anyhow::Result<()> {
    // 1. Validate JWT secret (refuse default, warn if too short)
    validate_jwt_secret(jwt_secret, "config")?;

    // 2. Verify repo_root is writable
    let test_file = repo_root.join(".write_test");
    std::fs::write(&test_file, "test")
        .with_context(|| format!("repo_root is not writable: {:?}", repo_root))?;
    std::fs::remove_file(&test_file)?;

    // 3. Verify TLS files exist if configured
    if let Some((ref cert, ref key)) = tls_config {
        if !cert.exists() {
            anyhow::bail!("TLS certificate not found: {:?}", cert);
        }
        if !key.exists() {
            anyhow::bail!("TLS private key not found: {:?}", key);
        }
    }

    tracing::info!("Configuration validation passed");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI args first (without initializing logging, to avoid early output)
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve {
            repo_root,
            http_addr,
            ssh_addr,
            host_key,
            db_url,
            jwt_secret,
            docker,
            external_runners,
            allow_host_execution,
            rate_limit_max,
            rate_limit_window,
            rate_limit_trusted_proxies,
            smtp_host,
            smtp_port,
            smtp_user,
            smtp_pass,
            smtp_from,
            tls_cert,
            tls_key,
            config,
            log_file,
            log_max_size_mb,
            log_max_files,
        } => {
            run_serve(
                repo_root,
                http_addr,
                ssh_addr,
                host_key,
                db_url,
                jwt_secret,
                docker,
                external_runners,
                allow_host_execution,
                rate_limit_max,
                rate_limit_window,
                rate_limit_trusted_proxies,
                smtp_host,
                smtp_port,
                smtp_user,
                smtp_pass,
                smtp_from,
                tls_cert,
                tls_key,
                config,
                log_file,
                log_max_size_mb,
                log_max_files,
            )
            .await?;
        }

        Commands::Migrate { db_url } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();

            tracing::info!(
                "Connecting to database: {}",
                rg_db::redact_database_url(&db_url)
            );
            let db = rg_db::connect(&db_url).await?;
            tracing::info!("Running database migrations...");
            rg_db::run_migrations(&db).await?;
            tracing::info!("Migrations complete ✅");
        }

        Commands::RebuildFts { db_url } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();

            tracing::info!(
                "Connecting to database: {}",
                rg_db::redact_database_url(&db_url)
            );
            let db = rg_db::connect(&db_url).await?;

            rg_db::rebuild_fts_indexes(&db).await?;

            tracing::info!("Full-text search indexes refreshed successfully ✅");
        }

        Commands::RepairPullRefs {
            db_url,
            repo_root,
            repo,
            include_closed,
            dry_run,
        } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();

            tracing::info!(
                "Connecting to database: {}",
                rg_db::redact_database_url(&db_url)
            );
            let db = rg_db::connect(&db_url).await?;
            let (owner, repo_name) = match repo.as_deref() {
                Some(spec) => match spec.split_once('/') {
                    Some((owner, name)) if !owner.is_empty() && !name.is_empty() => {
                        (Some(owner.to_string()), Some(name.to_string()))
                    }
                    _ => anyhow::bail!("--repo takes the form owner/name"),
                },
                None => (None, None),
            };
            let options = rg_core::pull_request::RepairOptions {
                owner,
                repo_name,
                include_closed,
                dry_run,
            };
            let report = rg_core::pull_request::repair_pull_refs(
                &db,
                std::path::Path::new(&repo_root),
                &options,
            )
            .await?;

            let verb = if dry_run { "would repair" } else { "repaired" };
            println!(
                "scanned {} pull request(s): {} already current, {} {}, {} skipped",
                report.scanned,
                report.current,
                report.actions().len(),
                verb,
                report.skipped.len()
            );
            for action in report.actions() {
                let previous = match action.previous.as_deref() {
                    Some(sha) => format!("was {}", short_sha(sha)),
                    None => "was missing".to_string(),
                };
                println!(
                    "  {} {}#{} refs/pull/{}/head -> {} ({}, {}{})",
                    if dry_run { "would repair" } else { "repaired" },
                    action.repo,
                    action.number,
                    action.number,
                    short_sha(&action.head_sha),
                    previous,
                    action.kind(),
                    if action.is_fork { ", fork" } else { "" }
                );
            }
            for skip in &report.skipped {
                match skip.number {
                    Some(number) => println!("  skipped {}#{}: {}", skip.repo, number, skip.reason),
                    None => println!("  skipped {}: {}", skip.repo, skip.reason),
                }
            }
            println!(
                "note: the base commit recorded at pull-request open \
                 (anchors.base_sha_when_opened) is not backfilled -- it was never \
                 stored for older pull requests, and guessing it would invent an \
                 anchor that was not there."
            );
        }

        Commands::BackupDb {
            db_url,
            output,
            force,
        } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();

            backup_sqlite_db(&db_url, &PathBuf::from(output), force).await?;
        }

        Commands::RestoreDb {
            db_url,
            input,
            force,
            keep_live_state,
        } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();

            let input = PathBuf::from(input);
            if input.is_file() {
                verify_sqlite_file(&input).await?;
            }
            restore_sqlite_db(&db_url, &input, force)?;
            if keep_live_state {
                println!("Skipped the post-restore quiesce step (--keep-live-state).");
            } else {
                quiesce_restored_database(&db_url).await?.print();
            }
        }

        Commands::Backup {
            db_url,
            repo_root,
            audit_archive_dir,
            config,
            output,
            force,
        } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();

            backup_instance(
                &db_url,
                std::path::Path::new(&repo_root),
                std::path::Path::new(&audit_archive_dir),
                config.as_deref().map(std::path::Path::new),
                std::path::Path::new(&output),
                force,
            )
            .await?;
        }

        Commands::Restore {
            db_url,
            repo_root,
            audit_archive_dir,
            input,
            force,
            keep_live_state,
        } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();

            // Everything is checked before the first write, so a damaged or
            // incompatible backup never replaces a working instance.
            let input_path = std::path::Path::new(&input);
            if !input_path.is_dir() {
                anyhow::bail!("backup directory does not exist: {}", input_path.display());
            }
            verify_backup(input_path).await?;
            restore_instance(
                input_path,
                &db_url,
                std::path::Path::new(&repo_root),
                std::path::Path::new(&audit_archive_dir),
                force,
            )?;
            if keep_live_state {
                println!("Skipped the post-restore quiesce step (--keep-live-state).");
            } else {
                let report = quiesce_restored_database(&db_url).await?;
                report.print();
            }
        }

        Commands::CreateRepo {
            owner,
            name,
            repo_root,
        } => {
            // Simple logging for create-repo command
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();

            let repo_root = PathBuf::from(&repo_root);
            let repo_dir = repo_root.join(format!("{}/{}.git", owner, name));
            std::fs::create_dir_all(&repo_dir)?;

            // Replace git init --bare with gix API
            gix::create::into(
                &repo_dir,
                gix::create::Kind::Bare,
                gix::create::Options::default(),
            )
            .with_context(|| "failed to create bare repository")?;

            println!("Created repository: {}/{}.git", owner, name);
        }

        Commands::Runner {
            server,
            name,
            runner_id,
            token,
            auth_token,
        } => {
            use reqwest::header;

            let client = reqwest::Client::new();

            // ── Register or use existing credentials ─────────
            let (runner_id, token) = match (runner_id, token) {
                (Some(rid), Some(tok)) => (rid, tok),
                _ => {
                    // Register new runner
                    let name = name.as_deref().unwrap_or("default-runner");
                    let auth_token = auth_token
                        .or_else(|| std::env::var("IRONFORGE_AUTH_TOKEN").ok())
                        .context(
                            "runner auto-registration requires --auth-token or \
                             IRONFORGE_AUTH_TOKEN; alternatively pass --runner-id and --token",
                        )?;
                    let resp: serde_json::Value = client
                        .post(format!("{}/api/v1/runners/register", server))
                        .bearer_auth(auth_token)
                        .json(&serde_json::json!({"name": name}))
                        .send()
                        .await
                        .context("failed to register runner")?
                        .json()
                        .await?;
                    let rid = resp["id"].as_i64().ok_or_else(|| {
                        anyhow::anyhow!("invalid register response: missing 'id'")
                    })?;
                    let tok = resp["token"]
                        .as_str()
                        .ok_or_else(|| {
                            anyhow::anyhow!("invalid register response: missing 'token'")
                        })?
                        .to_string();
                    eprintln!("Runner registered: id={}, token={}", rid, tok);
                    eprintln!("Save these credentials for future runs!");
                    (rid, tok)
                }
            };

            eprintln!("Runner started: server={}, id={}", server, runner_id);

            // ── Main poll loop ─────────────────────────────
            let auth_header = format!("Bearer {}", token);
            loop {
                // 1. Poll for job
                let poll_resp = client
                    .get(format!(
                        "{}/api/v1/runners/{}/jobs/poll?timeout=30",
                        server, runner_id
                    ))
                    .header(header::AUTHORIZATION, &auth_header)
                    .send()
                    .await;

                let job: serde_json::Value = match poll_resp {
                    Ok(r) if r.status() == reqwest::StatusCode::NO_CONTENT => {
                        continue;
                    }
                    Ok(r) => r.json().await?,
                    Err(e) => {
                        eprintln!("Poll error: {}, retrying in 5s", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let job_id = job["job_id"]
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("invalid poll response"))?;
                let script: Vec<&str> = job["script"]
                    .as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                let image = job["image"].as_str();

                eprintln!("Got job {}: {}", job_id, job["name"].as_str().unwrap_or(""));

                // 2. Start job
                let _ = client
                    .post(format!(
                        "{}/api/v1/runners/{}/jobs/{}/start",
                        server, runner_id, job_id
                    ))
                    .header(header::AUTHORIZATION, &auth_header)
                    .send()
                    .await;

                // 3. Execute job
                let script_str = script.join("\n");
                let (exit_code, log) = if let Some(img) = image {
                    run_job_docker(img, &script_str).await
                } else if host_runner_allowed() {
                    run_job_local(&script_str).await
                } else {
                    (
                        1,
                        "job requests no container image and host execution is disabled; \
                         set IRONFORGE_ALLOW_HOST_RUNNER=1 to run jobs directly on the \
                         host shell (no isolation)"
                            .to_string(),
                    )
                };

                // 4. Upload log
                let _ = client
                    .post(format!(
                        "{}/api/v1/runners/{}/jobs/{}/log",
                        server, runner_id, job_id
                    ))
                    .header(header::AUTHORIZATION, &auth_header)
                    .body(log.clone())
                    .send()
                    .await;

                // 5. Finish job
                let status = if exit_code == 0 { "success" } else { "failure" };
                let _ = client
                    .post(format!(
                        "{}/api/v1/runners/{}/jobs/{}/finish",
                        server, runner_id, job_id
                    ))
                    .header(header::AUTHORIZATION, &auth_header)
                    .json(&serde_json::json!({"status": status, "exit_code": exit_code}))
                    .send()
                    .await;

                eprintln!(
                    "Job {} finished: status={}, exit_code={}",
                    job_id, status, exit_code
                );
            }
        }

        Commands::Import {
            platform,
            source_url,
            target_owner,
            target_name,
            token,
            repo_root,
            db_url,
            skip_repo,
            skip_issues,
            skip_prs,
            skip_labels,
            skip_milestones,
            skip_releases,
            import_wiki,
        } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();

            // Resolve target name from source URL if not provided
            let target_repo_name = match target_name {
                Some(n) => n,
                None => {
                    let url = source_url.trim_end_matches('/').trim_end_matches(".git");
                    url.split('/')
                        .next_back()
                        .unwrap_or("imported-repo")
                        .to_string()
                }
            };

            println!("╔══════════════════════════════════════════════════╗");
            println!(
                "║  IronForge Import — {} → IronForge",
                platform.to_uppercase()
            );
            println!("╠══════════════════════════════════════════════════╣");
            println!("║  Source:     {}", source_url);
            println!("║  Target:     {}/{}", target_owner, target_repo_name);
            println!(
                "║  Token:      {}",
                if token.is_some() {
                    "provided"
                } else {
                    "not provided"
                }
            );
            println!(
                "║  Import:     {} {} {} {} {} {}",
                if !skip_repo { "📦repo" } else { "" },
                if !skip_labels { "🏷️labels" } else { "" },
                if !skip_milestones {
                    "🎯milestones"
                } else {
                    ""
                },
                if !skip_issues { "📝issues" } else { "" },
                if !skip_prs { "🔄PRs" } else { "" },
                if !skip_releases { "🚀releases" } else { "" },
            );
            if import_wiki {
                println!("║             📚wiki");
            }
            println!("╚══════════════════════════════════════════════════╝");

            tracing::info!(
                "Connecting to database: {}",
                rg_db::redact_database_url(&db_url)
            );
            let db = rg_db::connect(&db_url).await?;
            rg_db::run_migrations(&db).await?;

            // Verify platform is valid
            if platform != "github" && platform != "gitlab" {
                anyhow::bail!(
                    "unsupported platform: {}. Use 'github' or 'gitlab'.",
                    platform
                );
            }

            let repo_root = PathBuf::from(&repo_root);
            std::fs::create_dir_all(&repo_root)?;

            // Start import
            println!("\n⏳ Starting import...");
            let task = rg_core::import::service::start_import(
                &db,
                1, // user_id — in CLI mode, default to admin (ID 1)
                platform,
                source_url,
                target_owner.clone(),
                target_repo_name,
                token,
                !skip_repo,
                !skip_issues,
                !skip_prs,
                import_wiki,
                !skip_releases,
                !skip_labels,
                !skip_milestones,
                &repo_root,
            )
            .await?;

            println!("Import task created: id={}", task.id);
            println!("Polling for completion...");

            // Poll until complete
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let current = rg_db::ops::import_task_ops::find_by_id(&db, task.id)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("import task disappeared"))?;

                match current.status.as_str() {
                    "pending" | "cloning" | "importing" => {
                        let stage = current.stage.as_deref().unwrap_or("...");
                        println!("  [{}%] {}", current.progress, stage);
                    }
                    "completed" => {
                        println!("\n✅ Import completed successfully!");
                        if let Some(ref stats) = current.stats {
                            if let Ok(s) = serde_json::from_str::<serde_json::Value>(stats) {
                                println!(
                                    "   Stats: {}",
                                    serde_json::to_string_pretty(&s).unwrap_or_default()
                                );
                            }
                        }
                        break;
                    }
                    "failed" => {
                        let err = current.error.as_deref().unwrap_or("unknown error");
                        anyhow::bail!("Import failed: {err}");
                    }
                    _ => {
                        tracing::warn!("Unknown import status: {}", current.status);
                    }
                }
            }
        }

        Commands::Package { cmd } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();

            match cmd {
                PackageCmd::Publish {
                    pkg_type,
                    name,
                    version,
                    file,
                    owner,
                    repo,
                    token,
                    server_url,
                } => {
                    if !rg_core::package_registry::package_types::is_valid(&pkg_type) {
                        anyhow::bail!(
                            "Unsupported package type: {}. Supported: {}",
                            pkg_type,
                            rg_core::package_registry::package_types::ALL.join(", ")
                        );
                    }

                    let file_data = tokio::fs::read(&file)
                        .await
                        .context(format!("Failed to read file: {}", file))?;
                    let filename = std::path::Path::new(&file)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("package")
                        .to_string();

                    // Use HTTP API via token if provided, otherwise direct DB
                    if let Some(bearer) = token {
                        let client = reqwest::Client::new();
                        let url = format!(
                            "{}/api/v1/repos/{}/{}/packages/{}/publish?name={}&version={}",
                            server_url.trim_end_matches('/'),
                            owner,
                            repo,
                            pkg_type,
                            name,
                            version
                        );
                        let resp = client
                            .post(&url)
                            .header("Authorization", format!("Bearer {}", bearer))
                            .header(
                                "Content-Disposition",
                                format!("attachment; filename=\"{}\"", filename),
                            )
                            .body(file_data)
                            .send()
                            .await?;

                        let status = resp.status();
                        let body = resp.text().await?;
                        if status.is_success() {
                            println!("✅ Package published: {}/{}@{}", name, name, version);
                            println!("   {}", body);
                        } else {
                            anyhow::bail!("Publish failed ({}): {}", status, body);
                        }
                    } else {
                        // Direct DB access (requires --db-url)
                        // For direct DB: need DB access — but this command doesn't have db_url
                        anyhow::bail!(
                            "Direct DB publish requires a running server. Please use --token to authenticate via HTTP API."
                        );
                    }
                }

                PackageCmd::List {
                    owner,
                    repo,
                    pkg_type,
                    db_url,
                } => {
                    if !rg_core::package_registry::package_types::is_valid(&pkg_type) {
                        anyhow::bail!(
                            "Unsupported package type: {}. Supported: {}",
                            pkg_type,
                            rg_core::package_registry::package_types::ALL.join(", ")
                        );
                    }

                    tracing::info!(
                        "Connecting to database: {}",
                        rg_db::redact_database_url(&db_url)
                    );
                    let db = rg_db::connect(&db_url).await?;
                    rg_db::run_migrations(&db).await?;

                    match rg_core::package_registry::service::list_packages(
                        &db, &owner, &repo, &pkg_type,
                    )
                    .await
                    {
                        Ok(packages) => {
                            println!("Packages ({}) in {}/{}:", pkg_type, owner, repo);
                            for pkg in &packages {
                                println!(
                                    "  {} ({} versions, {} downloads)",
                                    pkg.name, pkg.version_count, pkg.download_count
                                );
                                if let Some(ref desc) = pkg.description {
                                    println!("    {}", desc);
                                }
                                if let Some(ref ver) = pkg.latest_version {
                                    println!("    latest: {}", ver);
                                }
                            }
                            if packages.is_empty() {
                                println!("  (none)");
                            }
                        }
                        Err(e) => anyhow::bail!("Failed to list packages: {e:#}"),
                    }
                }
            }
        }

        Commands::IndexRepo {
            repo_slug,
            repo_root,
            db_url,
            ref_name,
        } => {
            // Simple logging for index-repo command
            tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();

            // Parse owner/name from repo_slug
            let parts: Vec<&str> = repo_slug.splitn(2, '/').collect();
            if parts.len() != 2 {
                anyhow::bail!("Invalid repo slug format. Expected: owner/name");
            }
            let owner_username = parts[0];
            let repo_name = parts[1];

            tracing::info!(
                "Connecting to database: {}",
                rg_db::redact_database_url(&db_url)
            );
            let db = rg_db::connect(&db_url).await?;

            // Resolve `<owner>/<repo>` where owner is a user or an organization.
            let repo =
                rg_core::repo::service::find_repo_by_owner_name(&db, owner_username, repo_name)
                    .await
                    .context("Failed to find repository")?
                    .ok_or_else(|| {
                        anyhow::anyhow!("Repository not found: {}/{}", owner_username, repo_name)
                    })?;

            tracing::info!(
                repo_id = repo.id,
                repo_name = %repo.name,
                default_branch = %repo.default_branch,
                "Found repository"
            );

            // Determine ref to index
            let ref_to_index = ref_name.as_deref().unwrap_or(&repo.default_branch);

            // Construct repo path
            let repo_path = std::path::Path::new(&repo_root)
                .join(format!("{}/{}.git", owner_username, repo_name));

            if !repo_path.exists() {
                anyhow::bail!("Repository path does not exist: {}", repo_path.display());
            }

            tracing::info!(
                repo_path = %repo_path.display(),
                ref_name = %ref_to_index,
                "Indexing repository"
            );

            // Create indexer and index repository
            let indexer = rg_core::search::code_indexer::CodeIndexer::new(db.clone());
            let start_time = std::time::Instant::now();
            let indexed_count = indexer
                .index_repository(repo.id, &repo_path, ref_to_index)
                .await
                .context("Failed to index repository")?;
            let elapsed = start_time.elapsed();

            println!(
                "✅ Indexed {} files in {:.2}s",
                indexed_count,
                elapsed.as_secs_f64()
            );
            tracing::info!(
                indexed_count = indexed_count,
                elapsed_ms = elapsed.as_millis(),
                "Repository indexing complete"
            );
        }

        Commands::CreateUser {
            username,
            email,
            password,
            admin,
            db_url,
        } => {
            let db = rg_db::connect(&db_url).await?;
            // A fresh database needs the schema before the insert.
            rg_db::run_migrations(&db).await?;
            let password = match password.as_deref() {
                Some("-") => {
                    use std::io::BufRead;
                    let mut line = String::new();
                    std::io::stdin().lock().read_line(&mut line)?;
                    line.trim().to_string()
                }
                Some(value) => value.to_string(),
                None => anyhow::bail!(
                    "pass --password <VALUE> or --password - (read one line from stdin)"
                ),
            };
            if password.len() < 8 {
                anyhow::bail!("password must be at least 8 characters");
            }
            let hash = rg_core::auth::password::hash_password(&password)
                .map_err(|e| anyhow::anyhow!("could not hash the password: {e}"))?;
            let user = rg_db::ops::user_ops::create_user(&db, &username, &email, &hash, "").await?;
            // create_user exists for SSO provisioning and marks the account
            // "oauth2", which password login refuses; ops-created users log
            // in with the password just set.
            rg_db::ops::user_ops::set_auth_provider(&db, user.id, "local").await?;
            if admin {
                rg_db::ops::user_ops::update_by_id(&db, user.id, None, None, Some(true), None)
                    .await?;
            }
            println!(
                "created user {username} (id {}){}",
                user.id,
                if admin { " [admin]" } else { "" }
            );
        }
    }

    Ok(())
}

/// Initialise and run the IronForge server (HTTP + SSH).
#[allow(clippy::too_many_arguments)]
async fn run_serve(
    repo_root: String,
    http_addr: String,
    ssh_addr: String,
    host_key: Option<String>,
    db_url: String,
    jwt_secret: Option<String>,
    docker: bool,
    external_runners: bool,
    allow_host_execution: bool,
    rate_limit_max: u32,
    rate_limit_window: u64,
    rate_limit_trusted_proxies: Vec<String>,
    smtp_host: Option<String>,
    smtp_port: u16,
    smtp_user: Option<String>,
    smtp_pass: Option<String>,
    smtp_from: Option<String>,
    tls_cert: Option<String>,
    tls_key: Option<String>,
    config: Option<String>,
    log_file: Option<String>,
    log_max_size_mb: u64,
    log_max_files: usize,
) -> anyhow::Result<()> {
    // ── Load config file (if specified) ────────────────────────
    let cfg = if let Some(config_path) = &config {
        Some(load_config_file(config_path.as_str())?)
    } else {
        None
    };

    // Resolve JWT secret: env var > CLI args > config file > error
    let resolved_jwt_secret = if let Ok(env_secret) = std::env::var("IRONFORGE_JWT_SECRET") {
        validate_jwt_secret(&env_secret, "environment variable IRONFORGE_JWT_SECRET")?;
        tracing::info!("Using JWT secret from environment variable IRONFORGE_JWT_SECRET");
        env_secret
    } else if let Some(cli_secret) = jwt_secret {
        validate_jwt_secret(&cli_secret, "--jwt-secret CLI argument")?;
        cli_secret
    } else if let Some(cfg_secret) = cfg.as_ref().and_then(|c| c.auth.jwt_secret.clone()) {
        validate_jwt_secret(&cfg_secret, "config file [auth].jwt_secret")?;
        cfg_secret
    } else {
        anyhow::bail!(
            "No JWT secret provided. Set IRONFORGE_JWT_SECRET, use --jwt-secret, or configure [auth].jwt_secret in config file"
        );
    };

    // Resolve other values: CLI args > config file
    let resolved_repo_root = repo_root;
    let resolved_http_addr = http_addr;
    let resolved_ssh_addr = ssh_addr;
    let resolved_host_key =
        host_key.or_else(|| cfg.as_ref().and_then(|c| c.server.host_key.clone()));
    let resolved_db_url = db_url;
    // SQLite errors name the database file; keep that directory out of error
    // responses (the HTTP layer is not handed the database location).
    rg_http::redact::register_database_url(&resolved_db_url);
    let resolved_docker = docker || cfg.as_ref().and_then(|c| c.ci.docker).unwrap_or(false);
    let resolved_external_runners = external_runners
        || cfg
            .as_ref()
            .and_then(|c| c.ci.external_runners)
            .unwrap_or(false);
    let resolved_allow_host_execution = allow_host_execution
        || cfg
            .as_ref()
            .and_then(|c| c.ci.allow_host_execution)
            .unwrap_or(false);
    let resolved_rate_limit_max = if rate_limit_max > 0 {
        rate_limit_max
    } else {
        cfg.as_ref().and_then(|c| c.rate_limit.max).unwrap_or(0_u32)
    };
    let resolved_rate_limit_window = if rate_limit_window != 60 {
        rate_limit_window
    } else {
        cfg.as_ref()
            .and_then(|c| c.rate_limit.window_secs)
            .unwrap_or(60)
    };
    let resolved_rate_limit_trusted_proxy_values = if !rate_limit_trusted_proxies.is_empty() {
        rate_limit_trusted_proxies
    } else {
        cfg.as_ref()
            .map(|c| c.rate_limit.trusted_proxies.clone())
            .unwrap_or_default()
    };
    let resolved_rate_limit_trusted_proxies =
        parse_rate_limit_trusted_proxies(&resolved_rate_limit_trusted_proxy_values)?;

    // SMTP: CLI takes precedence, fallback to config
    let (
        resolved_smtp_host,
        resolved_smtp_port,
        resolved_smtp_user,
        resolved_smtp_pass,
        resolved_smtp_from,
    ) = {
        let h = smtp_host.or_else(|| cfg.as_ref().and_then(|c| c.smtp.host.clone()));
        let p = cfg.as_ref().and_then(|c| c.smtp.port).unwrap_or(smtp_port);
        let u = smtp_user.or_else(|| cfg.as_ref().and_then(|c| c.smtp.user.clone()));
        let pw = smtp_pass.or_else(|| cfg.as_ref().and_then(|c| c.smtp.pass.clone()));
        let f = smtp_from.or_else(|| cfg.as_ref().and_then(|c| c.smtp.from.clone()));
        (h, p, u, pw, f)
    };

    // TLS: CLI takes precedence, fallback to config
    let resolved_tls_cert = tls_cert.or_else(|| cfg.as_ref().and_then(|c| c.tls.cert.clone()));
    let resolved_tls_key = tls_key.or_else(|| cfg.as_ref().and_then(|c| c.tls.key.clone()));

    // Logging: CLI takes precedence, fallback to config
    let resolved_log_file = log_file.or_else(|| cfg.as_ref().and_then(|c| c.logging.file.clone()));
    let resolved_log_max_files = if log_max_files != 5 {
        log_max_files
    } else {
        cfg.as_ref().and_then(|c| c.logging.max_files).unwrap_or(5)
    };
    let resolved_log_max_size_mb = if log_max_size_mb != 10 {
        log_max_size_mb
    } else {
        cfg.as_ref()
            .and_then(|c| c.logging.max_size_mb)
            .unwrap_or(10)
    };

    // External URL: CLI takes precedence, fallback to config
    let resolved_external_url = cfg
        .as_ref()
        .and_then(|c| c.external_url.clone())
        .or_else(|| cfg.as_ref().and_then(|c| c.server.external_url.clone()));

    // Timeouts from config (with defaults)
    let resolved_job_timeout = cfg.as_ref().map(|c| c.timeouts.job_secs).unwrap_or(3600);
    let resolved_git_timeout = cfg
        .as_ref()
        .map(|c| c.timeouts.git_cmd_secs)
        .unwrap_or_else(default_git_timeout);
    let resolved_db_connect_timeout = cfg
        .as_ref()
        .map(|c| c.timeouts.db_connect_secs)
        .unwrap_or_else(default_db_connect_timeout);
    let resolved_db_idle_timeout = cfg
        .as_ref()
        .map(|c| c.timeouts.db_idle_secs)
        .unwrap_or_else(default_db_idle_timeout);

    // ── Initialize logging ─────────────────────────────────────
    if let Some(ref log_path) = resolved_log_file {
        let log_dir = std::path::Path::new(log_path)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let log_prefix = std::path::Path::new(log_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("ironforge");
        let log_suffix = std::path::Path::new(log_path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("log");

        let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix(log_prefix)
            .filename_suffix(log_suffix)
            .max_log_files(resolved_log_max_files)
            .build(log_dir)
            .map_err(|e| anyhow::anyhow!("failed to create log appender: {}", e))?;

        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .with_target(false)
            .with_writer(non_blocking)
            .init();

        std::mem::forget(_guard);

        tracing::info!(file = %log_path, "Logging to file with rotation");
        if resolved_log_max_size_mb != 10 {
            tracing::warn!(
                max_size_mb = resolved_log_max_size_mb,
                "log_max_size_mb is not enforced: the file appender rotates daily (not by size). Use log_max_files to cap the number of retained files."
            );
        }
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .with_target(false)
            .init();
    }

    let repo_root = PathBuf::from(&resolved_repo_root);
    std::fs::create_dir_all(&repo_root)?;

    // ── Git CLI gateway (seed configured command timeout) ─────────
    if let Err(e) = rg_git::cli_gateway::init_global_gateway(std::time::Duration::from_secs(
        resolved_git_timeout,
    )) {
        tracing::warn!(%e, "git gateway init failed — git-dependent features may be unavailable");
    } else {
        tracing::info!(git_cmd_secs = resolved_git_timeout, "Git CLI gateway ready");
    }

    // ── Database ──────────────────────────────────────────────────
    tracing::info!(
        "Connecting to database: {}",
        rg_db::redact_database_url(&resolved_db_url)
    );
    let db = rg_db::connect_with_timeouts(
        &resolved_db_url,
        resolved_db_connect_timeout,
        resolved_db_idle_timeout,
    )
    .await?;
    rg_db::run_migrations(&db).await?;
    tracing::info!("Database ready");

    let audit_config = cfg.as_ref().map(|config| &config.audit);
    let _audit_archiver_handle = if audit_config
        .and_then(|config| config.enabled)
        .unwrap_or(true)
    {
        let archive_config = rg_core::audit::archiver::AuditArchiveConfig {
            archive_dir: PathBuf::from(
                audit_config
                    .and_then(|config| config.archive_dir.as_deref())
                    .unwrap_or("./data/audit-archive"),
            ),
            archive_after_days: audit_config
                .and_then(|config| config.archive_after_days)
                .unwrap_or(90),
            interval_minutes: audit_config
                .and_then(|config| config.interval_minutes)
                .unwrap_or(60),
            batch_size: audit_config
                .and_then(|config| config.batch_size)
                .unwrap_or(1_000),
        };
        Some(rg_core::audit::archiver::spawn_archiver_with_config(
            db.clone(),
            archive_config,
        )?)
    } else {
        tracing::info!("Audit log archival disabled by configuration");
        None
    };

    // ── HTTP server ───────────────────────────────────────────────
    let smtp_config =
        match (
            resolved_smtp_host,
            resolved_smtp_user,
            resolved_smtp_pass,
            resolved_smtp_from,
        ) {
            (Some(host), Some(user), Some(pass), Some(from)) => Some(
                rg_core::email::SmtpConfig::new(&host, resolved_smtp_port, &user, &pass, &from),
            ),
            _ => None,
        };

    let tls_config = match (resolved_tls_cert, resolved_tls_key) {
        (Some(cert), Some(key)) => {
            tracing::info!("TLS enabled: cert={}, key={}", cert, key);
            Some((PathBuf::from(cert), PathBuf::from(key)))
        }
        (Some(_), None) => {
            tracing::warn!("TLS cert specified but no key — running HTTP only");
            None
        }
        (None, Some(_)) => {
            tracing::warn!("TLS key specified but no cert — running HTTP only");
            None
        }
        _ => None,
    };

    validate_config(&resolved_jwt_secret, &repo_root, &tls_config)?;

    let http_config = rg_http::HttpServerConfig {
        listen_addr: resolved_http_addr,
        repo_root: repo_root.clone(),
        db: db.clone(),
        jwt_secret: resolved_jwt_secret.clone(),
        docker_enabled: resolved_docker,
        external_runners: resolved_external_runners,
        rate_limit_max: resolved_rate_limit_max,
        rate_limit_window_secs: resolved_rate_limit_window,
        rate_limit_trusted_proxies: resolved_rate_limit_trusted_proxies,
        smtp_config,
        tls_config,
        oci_storage_path: None,
        external_url: resolved_external_url,
        job_timeout_secs: resolved_job_timeout,
        allow_host_execution: resolved_allow_host_execution,
        // M-14: Inject CiEngine via trait object, decoupling rg-http from rg-ci.
        ci_engine: std::sync::Arc::new(rg_ci::CiEngine),
    };

    // ── SSH server ────────────────────────────────────────────────
    let host_key_path = resolved_host_key.unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        format!("{}/.ssh/id_ed25519", home)
    });

    let ssh_config = rg_ssh::SshServerConfig {
        host_key_path: PathBuf::from(&host_key_path),
        listen_addr: resolved_ssh_addr,
        repo_root: repo_root.clone(),
        db: Some(db.clone()),
    };

    let http_handle = tokio::spawn(async move {
        if let Err(e) = rg_http::run(http_config).await {
            tracing::error!("HTTP server error: {:#}", e);
        }
    });

    let _ssh_handle = tokio::spawn(async move {
        if let Err(e) = rg_ssh::start_ssh_server(ssh_config).await {
            tracing::error!("SSH server error (HTTP unaffected): {:#}", e);
        }
    });

    tracing::info!("IronForge server started (Phase 20)");

    if let Err(e) = http_handle.await {
        tracing::error!("HTTP server task terminated: {:#}", e);
    }

    Ok(())
}

/// Execute a job script locally via platform-appropriate shell.
async fn run_job_local(script: &str) -> (i32, String) {
    #[cfg(unix)]
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .output()
        .await;

    #[cfg(windows)]
    let output = tokio::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .await;

    match output {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let mut log = String::new();
            if !o.stdout.is_empty() {
                log.push_str(&String::from_utf8_lossy(&o.stdout));
            }
            if !o.stderr.is_empty() {
                if !log.is_empty() {
                    log.push('\n');
                }
                log.push_str(&String::from_utf8_lossy(&o.stderr));
            }
            (code, log)
        }
        Err(e) => (-1, format!("Failed to spawn job: {}", e)),
    }
}

/// Whether CI jobs may be executed directly on the host shell.
///
/// A host shell has **no isolation whatsoever**, so it must be an explicit
/// operator decision rather than a silent default. Enable it with
/// `IRONFORGE_ALLOW_HOST_RUNNER=1` (or `true`/`yes`).
fn host_runner_allowed() -> bool {
    std::env::var("IRONFORGE_ALLOW_HOST_RUNNER")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

/// Execute a job script inside a sandboxed Docker container.
///
/// Fails **closed**: a job that asked for a container must never silently run
/// on the host shell just because the Docker daemon is down. The other two
/// runners (the `rg-ci` PipelineRunner and the `rg-runner` daemon) already
/// behave this way — this inline runner was the odd one out.
async fn run_job_docker(image: &str, script: &str) -> (i32, String) {
    // Check if Docker is available
    let docker_check = tokio::process::Command::new("docker")
        .arg("info")
        .output()
        .await;

    match docker_check {
        Ok(check) if !check.status.success() => {
            return (
                1,
                "job requested a container but the Docker daemon is unavailable; \
                 refusing to fall back to the host shell"
                    .to_string(),
            );
        }
        Err(e) => {
            return (
                1,
                format!(
                    "job requested a container but docker could not be invoked ({e}); \
                     refusing to fall back to the host shell"
                ),
            );
        }
        _ => {}
    }

    // The image reference comes from repository content; an unvalidated value
    // such as `--privileged` would be parsed by `docker run` as an option.
    if let Err(e) = rg_ci_isolation::validate_container_image(image) {
        return (1, format!("invalid container image: {e}"));
    }

    // Same resource/capability limits as the server-side runners: drop all
    // capabilities, no-new-privileges, memory/cpu/pid caps.
    let mut args: Vec<String> = vec!["run".to_string(), "--rm".to_string()];
    args.extend(rg_ci_isolation::DockerLimits::default().hardening_args());
    // `--` ends option parsing so the validated image cannot be mistaken for a
    // flag even if a future Docker version accepts new options.
    args.push("--".to_string());
    args.extend([
        image.to_string(),
        "sh".to_string(),
        "-c".to_string(),
        script.to_string(),
    ]);

    let output = tokio::process::Command::new("docker")
        .args(&args)
        .output()
        .await;

    match output {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let mut log = String::new();
            if !o.stdout.is_empty() {
                log.push_str(&String::from_utf8_lossy(&o.stdout));
            }
            if !o.stderr.is_empty() {
                if !log.is_empty() {
                    log.push('\n');
                }
                log.push_str(&String::from_utf8_lossy(&o.stderr));
            }
            if code != 0 && log.is_empty() {
                log = format!("Docker exited with code {}", code);
            }
            (code, log)
        }
        Err(e) => (-1, format!("Failed to run docker: {}", e)),
    }
}

#[cfg(test)]
mod config_tests {
    use super::ConfigFile;

    #[test]
    fn example_config_includes_valid_audit_archive_settings() {
        let config: ConfigFile =
            toml::from_str(include_str!("../../../ironforge.example.toml")).unwrap();
        assert_eq!(config.audit.enabled, Some(true));
        assert_eq!(config.audit.archive_after_days, Some(90));
        assert_eq!(config.audit.interval_minutes, Some(60));
        assert_eq!(config.audit.batch_size, Some(1_000));
    }

    /// A config file that says nothing about timeouts must not zero them: the
    /// database connect timeout of 0 seconds made the server fail to start.
    #[test]
    fn a_config_without_a_timeouts_section_keeps_the_default_timeouts() {
        let config: ConfigFile =
            toml::from_str("external_url = \"https://git.example\"\n").unwrap();
        assert_eq!(config.timeouts.job_secs, 3600);
        assert_eq!(config.timeouts.git_cmd_secs, 120);
        assert_eq!(config.timeouts.db_connect_secs, 10);
        assert_eq!(config.timeouts.db_idle_secs, 600);

        let partial: ConfigFile = toml::from_str("[timeouts]\njob_secs = 5\n").unwrap();
        assert_eq!(partial.timeouts.job_secs, 5);
        assert_eq!(partial.timeouts.db_connect_secs, 10);
    }
}

#[cfg(test)]
mod backup_verify_tests {
    use super::*;

    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "ironforge-backup-test-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Dir(path)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A backup directory with a real migrated database and one data file.
    async fn sample_backup(dir: &Dir) -> PathBuf {
        let backup = dir.0.join("backup");
        std::fs::create_dir_all(backup.join("data")).unwrap();
        std::fs::write(backup.join("data/repo.txt"), b"object bytes").unwrap();
        let url = format!(
            "sqlite://{}?mode=rwc",
            backup.join("ironforge.db").display()
        );
        let db = rg_db::connect(&url).await.unwrap();
        rg_db::run_migrations(&db).await.unwrap();
        drop(db);
        std::fs::write(backup.join("manifest.json"), br#"{"format_version":1}"#).unwrap();
        write_backup_checksums(&backup).unwrap();
        backup
    }

    #[tokio::test]
    async fn an_intact_backup_verifies() {
        let dir = Dir::new("ok");
        let backup = sample_backup(&dir).await;
        verify_backup(&backup).await.unwrap();
    }

    #[tokio::test]
    async fn a_changed_missing_or_extra_file_is_refused() {
        let dir = Dir::new("tamper");
        let backup = sample_backup(&dir).await;
        std::fs::write(backup.join("data/repo.txt"), b"object bytez").unwrap();
        let err = verify_backup(&backup).await.unwrap_err().to_string();
        assert!(err.contains("checksum mismatch"), "{err}");

        write_backup_checksums(&backup).unwrap();
        std::fs::write(backup.join("data/extra.txt"), b"x").unwrap();
        let err = verify_backup(&backup).await.unwrap_err().to_string();
        assert!(err.contains("not in checksums"), "{err}");

        std::fs::remove_file(backup.join("data/extra.txt")).unwrap();
        std::fs::remove_file(backup.join("data/repo.txt")).unwrap();
        let err = verify_backup(&backup).await.unwrap_err().to_string();
        assert!(err.contains("missing a file"), "{err}");
    }

    #[tokio::test]
    async fn a_backup_without_checksums_is_accepted_but_a_damaged_database_is_not() {
        let dir = Dir::new("legacy");
        let backup = sample_backup(&dir).await;
        std::fs::remove_file(backup.join(CHECKSUMS_FILE)).unwrap();
        verify_backup(&backup).await.unwrap();

        let db = backup.join("ironforge.db");
        let mut bytes = std::fs::read(&db).unwrap();
        let mid = bytes.len() / 2;
        let end = mid + 4096.min(bytes.len() - mid);
        for b in &mut bytes[mid..end] {
            *b = 0xA5;
        }
        std::fs::write(&db, bytes).unwrap();
        assert!(verify_backup(&backup).await.is_err());
    }

    #[tokio::test]
    async fn a_database_from_a_newer_schema_is_refused() {
        let dir = Dir::new("newer");
        let backup = sample_backup(&dir).await;
        let url = format!("sqlite://{}?mode=rw", backup.join("ironforge.db").display());
        let db = rg_db::connect(&url).await.unwrap();
        {
            use rg_db::sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
            db.execute(Statement::from_string(
                DatabaseBackend::Sqlite,
                "INSERT INTO seaql_migrations (version, applied_at) VALUES ('m29990101_000001_future', 0)"
                    .to_string(),
            ))
            .await
            .unwrap();
            // Fold the write-ahead log into the file: a backup is one plain file.
            db.execute(Statement::from_string(
                DatabaseBackend::Sqlite,
                "PRAGMA wal_checkpoint(TRUNCATE)".to_string(),
            ))
            .await
            .unwrap();
        }
        drop(db);
        write_backup_checksums(&backup).unwrap();
        let err = verify_backup(&backup).await.unwrap_err().to_string();
        assert!(err.contains("newer IronForge"), "{err}");
    }

    /// A migrated SQLite database full of state that a restore must quiesce.
    /// Returns the temp dir (keep it alive), the URL and the seeding time.
    async fn seed_stale_state(tag: &str) -> (Dir, String, chrono::DateTime<chrono::Utc>) {
        use chrono::{Duration, Utc};
        use rg_db::entities::{
            access_token, agent_session, background_job, password_reset_token, pipeline,
            pipeline_job, pipeline_stage, repository,
        };
        use rg_db::ops::{repo_ops, user_ops};
        use rg_db::sea_orm::{ActiveModelTrait, NotSet, Set};

        let dir = Dir::new(tag);
        let path = dir.0.join("restored.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let db = rg_db::connect(&url).await.unwrap();
        rg_db::run_migrations(&db).await.unwrap();
        let now = Utc::now();
        let user = user_ops::create_user(&db, "q1", "q1@example.invalid", "x", "Q")
            .await
            .unwrap();
        let repo = repo_ops::create(
            &db,
            repository::ActiveModel {
                id: NotSet,
                owner_id: Set(user.id),
                name: Set("r".into()),
                description: Set(None),
                is_private: Set(false),
                default_branch: Set("main".into()),
                fork_id: Set(None),
                stars_count: Set(0),
                forks_count: Set(0),
                org_id: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
                origin_repo_id: Set(None),
            },
        )
        .await
        .unwrap();
        let ended_before = now - Duration::hours(1);
        for (id, ended) in [("live", None), ("done", Some(ended_before))] {
            agent_session::ActiveModel {
                id: Set(id.into()),
                repo_id: Set(repo.id),
                user_id: Set(user.id),
                secret_hash: Set("h".into()),
                display_name: Set(id.into()),
                client_kind: Set("test".into()),
                created_at: Set(now),
                last_seen_at: Set(now),
                expires_at: Set(now + Duration::hours(8)),
                ended_at: Set(ended),
            }
            .insert(&db)
            .await
            .unwrap();
        }
        password_reset_token::ActiveModel {
            id: NotSet,
            user_id: Set(user.id),
            token_hash: Set("t".into()),
            expires_at: Set(now + Duration::hours(1)),
            used: Set(false),
            created_at: Set(now),
        }
        .insert(&db)
        .await
        .unwrap();
        for name in ["a", "b"] {
            access_token::ActiveModel {
                id: NotSet,
                user_id: Set(user.id),
                name: Set(name.into()),
                token_hash: Set(format!("h-{name}")),
                scopes: Set("repo".into()),
                expires_at: Set(None),
                repo_allowlist: Set(None),
                ref_allowlist: Set(None),
                last_used_at: Set(None),
                created_at: Set(now),
            }
            .insert(&db)
            .await
            .unwrap();
        }
        for (kind, status) in [
            ("webhook.deliver", "pending"),
            ("email.send", "running"),
            ("email.send", "succeeded"),
            ("mirror.sync", "pending"),
        ] {
            background_job::ActiveModel {
                id: NotSet,
                task_type: Set(kind.into()),
                payload: Set("{}".into()),
                status: Set(status.into()),
                attempts: Set(0),
                max_attempts: Set(3),
                run_at: Set(now),
                locked_by: Set(None),
                locked_at: Set(None),
                last_error: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let naive = now.naive_utc();
        for (pstatus, label) in [("running", "live"), ("success", "done")] {
            let p = pipeline::ActiveModel {
                id: NotSet,
                repo_id: Set(repo.id),
                commit_sha: Set(format!("{label:0<40}")),
                ref_name: Set("refs/heads/main".into()),
                status: Set(pstatus.into()),
                trigger_type: Set("push".into()),
                triggered_by: Set(None),
                started_at: Set(Some(naive)),
                finished_at: Set(None),
                created_at: Set(naive),
            }
            .insert(&db)
            .await
            .unwrap();
            let stage = pipeline_stage::ActiveModel {
                id: NotSet,
                pipeline_id: Set(p.id),
                name: Set("build".into()),
                stage_order: Set(0),
                status: Set(pstatus.into()),
                started_at: Set(Some(naive)),
                finished_at: Set(None),
            }
            .insert(&db)
            .await
            .unwrap();
            pipeline_job::ActiveModel {
                id: NotSet,
                stage_id: Set(stage.id),
                runner_id: Set(None),
                name: Set("deploy".into()),
                image: Set(None),
                script: Set("true".into()),
                variables: Set(None),
                cache_key: Set(None),
                cache_paths: Set(None),
                allow_failure: Set(false),
                timeout_seconds: Set(None),
                when_condition: Set("on_success".into()),
                if_condition: Set(None),
                environment_id: Set(None),
                environment_name: Set(None),
                status: Set(pstatus.into()),
                exit_code: Set(None),
                log: Set(None),
                started_at: Set(Some(naive)),
                finished_at: Set(None),
                updated_at: Set(None),
                tags: Set(None),
            }
            .insert(&db)
            .await
            .unwrap();
        }
        drop(db);
        (dir, url, now)
    }

    #[tokio::test]
    async fn quiesce_ends_sessions_and_abandons_only_queued_outbound_jobs() {
        use rg_db::entities::{
            agent_session, background_job, pipeline, pipeline_job, pipeline_stage,
        };
        use rg_db::sea_orm::EntityTrait;
        let (_dir, url, now) = seed_stale_state("quiesce").await;

        let report = quiesce_restored_database(&url).await.unwrap();
        assert_eq!(
            report,
            QuiesceReport {
                sessions_ended: 1,
                reset_tokens_deleted: 1,
                jobs_abandoned: 2,
                pipelines_cancelled: 1,
                access_tokens_restored: 2,
            }
        );
        // A second run finds nothing left to change.
        let again = quiesce_restored_database(&url).await.unwrap();
        assert_eq!(again.sessions_ended, 0);
        assert_eq!(again.jobs_abandoned, 0);
        assert_eq!(again.pipelines_cancelled, 0);

        // The rows still read back through the entities (timestamps stay parseable).
        let db = rg_db::connect(&url).await.unwrap();
        let sessions = agent_session::Entity::find().all(&db).await.unwrap();
        assert!(sessions.iter().all(|s| s.ended_at.is_some()));
        let done = sessions.iter().find(|s| s.id == "done").unwrap();
        assert!(
            done.ended_at.unwrap() < now,
            "already-ended session kept its time"
        );
        let jobs = background_job::Entity::find().all(&db).await.unwrap();
        let by = |kind: &str, st: &str| {
            jobs.iter()
                .filter(|j| j.task_type == kind && j.status == st)
                .count()
        };
        assert_eq!(by("webhook.deliver", "dead"), 1);
        assert_eq!(by("email.send", "dead"), 1);
        assert_eq!(by("email.send", "succeeded"), 1);
        assert_eq!(
            by("mirror.sync", "pending"),
            1,
            "other task types are untouched"
        );
        assert!(jobs.iter().filter(|j| j.status == "dead").all(|j| j
            .last_error
            .as_deref()
            .is_some_and(|e| e.contains("abandoned at restore"))));
        // The live pipeline is cancelled with its stage and job failed; a finished
        // one is left alone.
        let pipes = pipeline::Entity::find().all(&db).await.unwrap();
        assert_eq!(pipes.iter().filter(|p| p.status == "canceled").count(), 1);
        assert_eq!(pipes.iter().filter(|p| p.status == "success").count(), 1);
        assert!(pipes
            .iter()
            .filter(|p| p.status == "canceled")
            .all(|p| p.finished_at.is_some()));
        let stages = pipeline_stage::Entity::find().all(&db).await.unwrap();
        assert_eq!(stages.iter().filter(|x| x.status == "failed").count(), 1);
        assert_eq!(stages.iter().filter(|x| x.status == "success").count(), 1);
        let pjobs = pipeline_job::Entity::find().all(&db).await.unwrap();
        assert_eq!(pjobs.iter().filter(|x| x.status == "failed").count(), 1);
        assert_eq!(pjobs.iter().filter(|x| x.status == "success").count(), 1);
    }

    /// `deploy/post-restore-quiesce.sql` (what PostgreSQL/MySQL operators run) must
    /// leave the same state as the SQLite CLI path.
    #[tokio::test]
    async fn the_quiesce_sql_file_matches_the_cli_path() {
        use rg_db::sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
        let snapshot = |url: String| async move {
            let db = rg_db::connect(&url).await.unwrap();
            let mut out = Vec::new();
            for q in [
                "SELECT status FROM pipelines ORDER BY id",
                "SELECT status FROM pipeline_stages ORDER BY id",
                "SELECT status FROM pipeline_jobs ORDER BY id",
                "SELECT task_type || ':' || status FROM background_jobs ORDER BY id",
                "SELECT id || ':' || (ended_at IS NOT NULL) FROM agent_sessions ORDER BY id",
                "SELECT COUNT(*) FROM password_reset_tokens",
            ] {
                let rows = db
                    .query_all(Statement::from_string(
                        DatabaseBackend::Sqlite,
                        q.to_string(),
                    ))
                    .await
                    .unwrap();
                out.push(
                    rows.iter()
                        .map(|r| {
                            r.try_get_by_index::<String>(0).unwrap_or_else(|_| {
                                r.try_get_by_index::<i64>(0).unwrap().to_string()
                            })
                        })
                        .collect::<Vec<_>>(),
                );
            }
            out
        };
        let (_d1, via_cli, _) = seed_stale_state("quiesce-sql-cli").await;
        quiesce_restored_database(&via_cli).await.unwrap();
        let (_d2, via_sql, _) = seed_stale_state("quiesce-sql-file").await;
        let db = rg_db::connect(&via_sql).await.unwrap();
        db.execute_unprepared(include_str!("../../../deploy/post-restore-quiesce.sql"))
            .await
            .unwrap();
        drop(db);
        assert_eq!(snapshot(via_cli).await, snapshot(via_sql).await);
    }

    #[tokio::test]
    async fn quiesce_tolerates_a_database_without_the_newer_tables() {
        let dir = Dir::new("quiesce-old");
        let path = dir.0.join("old.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let db = rg_db::connect(&url).await.unwrap();
        drop(db);
        let report = quiesce_restored_database(&url).await.unwrap();
        assert_eq!(report, QuiesceReport::default());
    }
}
