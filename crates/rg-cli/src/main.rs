//! IronForge CLI — main entry point.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "ironforge", about = "A Git hosting platform written in Rust")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
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
    },

    /// Create a new bare repository
    CreateRepo {
        /// Owner username
        owner: String,

        /// Repository name (without .git suffix)
        name: String,

        /// Root directory for repositories
        #[arg(long, default_value = "./repos")]
        repo_root: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve {
            repo_root,
            http_addr,
            ssh_addr,
            host_key,
        } => {
            let repo_root = PathBuf::from(&repo_root);
            std::fs::create_dir_all(&repo_root)?;

            // Start HTTP and SSH servers concurrently
            let http_config = rg_http::HttpServerConfig {
                listen_addr: http_addr,
                repo_root: repo_root.clone(),
            };

            let host_key_path = host_key.unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                format!("{}/.ssh/id_ed25519", home)
            });

            let ssh_config = rg_ssh::SshServerConfig {
                host_key_path: PathBuf::from(&host_key_path),
                listen_addr: ssh_addr,
                repo_root: repo_root.clone(),
            };

            let http_handle = tokio::spawn(async move {
                if let Err(e) = rg_http::run(http_config).await {
                    tracing::error!("HTTP server error: {:#}", e);
                }
            });

            let ssh_handle = tokio::spawn(async move {
                if let Err(e) = rg_ssh::start_ssh_server(ssh_config).await {
                    tracing::error!("SSH server error: {:#}", e);
                }
            });

            tracing::info!("IronForge server started");

            // Wait for either server to fail
            tokio::select! {
                _ = http_handle => {},
                _ = ssh_handle => {},
            }
        }

        Commands::CreateRepo {
            owner,
            name,
            repo_root,
        } => {
            let repo_root = PathBuf::from(&repo_root);
            let repo_dir = repo_root.join(format!("{}/{}.git", owner, name));
            std::fs::create_dir_all(&repo_dir)?;

            let output = tokio::process::Command::new("git")
                .arg("init")
                .arg("--bare")
                .arg(&repo_dir)
                .output()
                .await?;

            if output.status.success() {
                println!("Created repository: {}/{}.git", owner, name);
            } else {
                anyhow::bail!(
                    "failed to create repository: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }

    Ok(())
}
