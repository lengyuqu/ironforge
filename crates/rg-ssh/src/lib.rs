//! IronForge SSH server implementation using russh.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use russh::keys::load_secret_key;
use russh::server::{Auth, Config, Handler, Msg, Server as _, Session};
use russh::{Channel, ChannelId, ChannelStream};
use tokio::io::AsyncWriteExt;

use rg_git::protocol::receive_pack::handle_receive_pack_stream;
use rg_git::protocol::upload_pack::handle_upload_pack_stream;

/// Error type for SSH handler.
#[derive(Debug)]
struct HandlerError(String);

impl std::fmt::Display for HandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for HandlerError {}

impl From<russh::Error> for HandlerError {
    fn from(e: russh::Error) -> Self {
        HandlerError(e.to_string())
    }
}

impl From<anyhow::Error> for HandlerError {
    fn from(e: anyhow::Error) -> Self {
        HandlerError(format!("{:#}", e))
    }
}

/// SSH server configuration.
pub struct SshServerConfig {
    /// Path to the SSH host key file (e.g., ed25519).
    pub host_key_path: PathBuf,
    /// Address to listen on (e.g., "0.0.0.0:2222").
    pub listen_addr: String,
    /// Root directory for git repositories.
    pub repo_root: PathBuf,
}

/// The IronForge SSH server — implements `russh::server::Server`.
struct SshServer {
    config: Arc<Config>,
    repo_root: Arc<PathBuf>,
    id: usize,
}

impl SshServer {
    pub fn new(ssh_config: SshServerConfig) -> Result<Self> {
        let host_key = load_secret_key(&ssh_config.host_key_path, None)
            .with_context(|| format!("failed to load host key: {:?}", ssh_config.host_key_path))?;

        let config = Config {
            auth_rejection_time: std::time::Duration::from_secs(1),
            auth_rejection_time_initial: Some(std::time::Duration::from_secs(0)),
            keys: vec![host_key],
            ..Default::default()
        };

        Ok(Self {
            config: Arc::new(config),
            repo_root: Arc::new(ssh_config.repo_root),
            id: 0,
        })
    }

    pub async fn run(&mut self, listen_addr: &str) -> Result<()> {
        let addr: std::net::SocketAddr = listen_addr
            .parse()
            .with_context(|| format!("invalid listen address: {}", listen_addr))?;

        tracing::info!(%listen_addr, "Starting SSH server");
        self.run_on_address(self.config.clone(), addr)
            .await
            .context("SSH server error")?;

        Ok(())
    }
}

impl russh::server::Server for SshServer {
    type Handler = SshHandler;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self::Handler {
        let handler = SshHandler {
            repo_root: self.repo_root.clone(),
            id: self.id,
            channel: None,
        };
        self.id += 1;
        handler
    }

    fn handle_session_error(&mut self, error: <Self::Handler as Handler>::Error) {
        tracing::error!("Session error: {:?}", error);
    }
}

/// russh Handler implementation for IronForge.
/// One SshHandler per client connection.
struct SshHandler {
    repo_root: Arc<PathBuf>,
    id: usize,
    /// The channel opened by the client for this session.
    channel: Option<Channel<Msg>>,
}

impl Handler for SshHandler {
    type Error = HandlerError;

    async fn auth_publickey(
        &mut self,
        _user: &str,
        _public_key: &russh::keys::PublicKey,
    ) -> Result<Auth, Self::Error> {
        // Phase 1: accept any auth
        Ok(Auth::Accept)
    }

    async fn auth_password(&mut self, _user: &str, _password: &str) -> Result<Auth, Self::Error> {
        // Phase 1: accept any auth
        Ok(Auth::Accept)
    }

    async fn auth_keyboard_interactive(
        &mut self,
        _user: &str,
        _submethods: &str,
        _response: Option<russh::server::Response<'_>>,
    ) -> Result<Auth, Self::Error> {
        // Phase 1: accept any auth
        Ok(Auth::Accept)
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        tracing::debug!(id = self.id, channel_id = ?channel.id(), "channel_open_session");
        self.channel = Some(channel);
        Ok(true)
    }

    async fn exec_request(
        &mut self,
        channel_id: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let command = String::from_utf8_lossy(data).to_string();
        tracing::info!(%command, id = self.id, "SSH exec request");

        let (service, repo_path) = parse_git_command(&command)?;

        let repo_full_path = {
            // Try path as-is first, then with .git suffix.
            let p = self.repo_root.join(&repo_path);
            if p.exists() {
                p
            } else {
                let with_git = self.repo_root.join(format!("{}.git", repo_path));
                if with_git.exists() {
                    with_git
                } else {
                    let err_msg = format!("repository not found: {}", repo_path);
                    tracing::error!(%err_msg);
                    session.channel_failure(channel_id)?;
                    return Err(HandlerError(err_msg));
                }
            }
        };

        // Take the stored Channel so we can call into_stream() on it.
        let ch = match self.channel.take() {
            Some(ch) => ch,
            None => {
                let msg = "no channel available for exec_request";
                tracing::error!(msg);
                session.channel_failure(channel_id)?;
                return Err(HandlerError(msg.into()));
            }
        };

        // Signal the client that the exec request was accepted.
        session.channel_success(channel_id)?;

        // Obtain a Handle so we can send exit-status from the spawned task.
        // Handle::exit_status_request(channel_id, code) is the server-side API.
        let handle = session.handle();
        let service_name = service.clone();

        tokio::spawn(async move {
            tracing::info!(%service_name, path = %repo_full_path.display(), "Starting git SSH session");

            // Keep `stream` owned here so we control when EOF is sent.
            // We run the git handler with &mut stream, then:
            //   1. Send exit-status (channel must still be open)
            //   2. Shutdown stream → sends SSH EOF to client (channel close follows)
            let mut stream: ChannelStream<Msg> = ch.into_stream();

            let result = match service_name.as_str() {
                "git-upload-pack" => handle_upload_pack_stream(&repo_full_path, &mut stream).await,
                "git-receive-pack" => handle_receive_pack_stream(&repo_full_path, &mut stream).await,
                _ => Err(anyhow::anyhow!("Unknown git service: {}", service_name)),
            };

            let exit_code: u32 = if result.is_ok() { 0 } else { 1 };

            match &result {
                Ok(_) => tracing::info!(%service_name, "Git SSH session complete"),
                Err(e) => tracing::error!(error = %e, %service_name, "Git SSH session failed"),
            }

            // Step 1: Send exit-status while channel is still alive.
            // Must happen BEFORE EOF / channel close.
            if let Err(e) = handle.exit_status_request(channel_id, exit_code).await {
                tracing::warn!(error = ?e, "failed to send exit_status to client");
            }

            // Step 2: Shutdown the stream (sends SSH channel EOF).
            // This is critical: it signals the client that we have finished sending
            // ALL data (including the report-status pkt-lines). Without this explicit
            // shutdown, data still buffered in russh may be lost when stream is dropped.
            if let Err(e) = stream.shutdown().await {
                tracing::warn!(error = ?e, "failed to shutdown SSH stream");
            }

            // Step 3: stream drops here → channel close.
        });

        Ok(())
    }
}

/// Parse a git SSH command string like:
///   `git-upload-pack '/owner/repo'`
///   `git-receive-pack '/owner/repo.git'`
fn parse_git_command(command: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = command.splitn(2, ' ').collect();
    if parts.len() < 2 {
        anyhow::bail!("invalid git command: {}", command);
    }

    let service = parts[0].trim().to_string();
    if service != "git-upload-pack" && service != "git-receive-pack" {
        anyhow::bail!("unsupported git command: {}", service);
    }

    // Strip surrounding quotes and leading slashes from the repo path.
    let raw_path = parts[1].trim();
    let repo_path = raw_path
        .trim_start_matches('\'')
        .trim_end_matches('\'')
        .trim_start_matches('"')
        .trim_end_matches('"')
        .trim_start_matches('/')
        .to_string();

    Ok((service, repo_path))
}

/// Public entry point to start the SSH server.
pub async fn start_ssh_server(config: SshServerConfig) -> Result<()> {
    let addr = config.listen_addr.clone();
    let mut server = SshServer::new(config)?;
    server.run(&addr).await
}
