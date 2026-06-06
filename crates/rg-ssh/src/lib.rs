//! IronForge SSH server implementation using russh.
//!
//! Phase 2: auth_publickey queries the database for matching SSH keys.
//! auth_password queries the database and verifies via Argon2.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use russh::keys::load_secret_key;
use russh::server::{Auth, Config, Handler, Msg, Server as _, Session};
use russh::{Channel, ChannelId, ChannelStream};
use sea_orm::DatabaseConnection;
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
    /// Database connection (None = open access, Phase 1 compat).
    pub db: Option<DatabaseConnection>,
}

/// Shared state passed to every SshHandler.
struct SharedState {
    repo_root: Arc<PathBuf>,
    db: Option<Arc<DatabaseConnection>>,
}

/// The IronForge SSH server — implements `russh::server::Server`.
struct SshServer {
    config: Arc<Config>,
    shared: Arc<SharedState>,
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

        let shared = Arc::new(SharedState {
            repo_root: Arc::new(ssh_config.repo_root),
            db: ssh_config.db.map(Arc::new),
        });

        Ok(Self {
            config: Arc::new(config),
            shared,
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

    fn new_client(&mut self, peer: Option<std::net::SocketAddr>) -> Self::Handler {
        let handler = SshHandler {
            shared: self.shared.clone(),
            id: self.id,
            _peer: peer,
            channel: None,
            authenticated_user_id: None,
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
    shared: Arc<SharedState>,
    id: usize,
    _peer: Option<std::net::SocketAddr>,
    /// The channel opened by the client for this session.
    channel: Option<Channel<Msg>>,
    /// User id resolved during authentication (None if auth not DB-backed).
    authenticated_user_id: Option<i64>,
}

impl Handler for SshHandler {
    type Error = HandlerError;

    // CRITICAL: Auth::Reject must include `partial_success: false` (踩坑经验 #5)
    //
    // russh's `Auth::Reject` has a field `partial_success: bool`.
    // If this is `true`, the server tells the client "you partially succeeded,
    // try other methods". This can cause:
    //   - Infinite auth loops
    //   - Clients reporting "partial success" errors
    //   - Unexpected behavior where auth should have been a clear reject
    //
    // Always use `partial_success: false` unless you specifically implement
    // multi-method partial auth (which we don't).
    //
    // Also: `fingerprint()` REQUIRES `HashAlg::Sha256` argument.
    // Without it, the method signature doesn't match (it requires the alg).
    // The returned Fingerprint displays as "SHA256:<base64>" which is
    // the standard format for authorized_keys.

    async fn auth_publickey(
        &mut self,
        _user: &str,
        public_key: &russh::keys::PublicKey,
    ) -> Result<Auth, Self::Error> {
        let Some(db) = &self.shared.db else {
            // Phase 1 compat: no DB, accept all
            return Ok(Auth::Accept);
        };

        // Compute SHA-256 fingerprint. ssh_key is a transitive dep of russh via
        // internal-russh-forked-ssh-key. The fingerprint() method returns a
        // Fingerprint that implements Display as "SHA256:<base64>".
        use russh::keys::ssh_key;
        let fp = public_key.fingerprint(ssh_key::HashAlg::Sha256);
        let fp_str = fp.to_string();
        tracing::debug!(fingerprint = %fp_str, "SSH pubkey auth attempt");

        match rg_db::ops::ssh_key_ops::find_by_fingerprint(db, &fp_str).await {
            Ok(Some(key)) => {
                self.authenticated_user_id = Some(key.user_id);
                tracing::info!(user_id = key.user_id, "SSH pubkey auth accepted");
                Ok(Auth::Accept)
            }
            Ok(None) => {
                tracing::warn!(fingerprint = %fp_str, "SSH pubkey not found");
                Ok(Auth::Reject { proceed_with_methods: None, partial_success: false })
            }
            Err(e) => {
                tracing::error!(error = %e, "DB error during pubkey lookup");
                Ok(Auth::Reject { proceed_with_methods: None, partial_success: false })
            }
        }
    }

    async fn auth_password(&mut self, username: &str, password: &str) -> Result<Auth, Self::Error> {
        let Some(db) = &self.shared.db else {
            // Phase 1 compat
            return Ok(Auth::Accept);
        };

        match rg_db::ops::user_ops::find_by_username(db, username).await {
            Ok(Some(user)) => {
                match rg_core::auth::password::verify_password(password, &user.password_hash) {
                    Ok(true) => {
                        self.authenticated_user_id = Some(user.id);
                        tracing::info!(username, "SSH password auth accepted");
                        Ok(Auth::Accept)
                    }
                    Ok(false) => {
                        tracing::warn!(username, "SSH password auth rejected");
                        Ok(Auth::Reject { proceed_with_methods: None, partial_success: false })
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "password verify error");
                        Ok(Auth::Reject { proceed_with_methods: None, partial_success: false })
                    }
                }
            }
            Ok(None) => {
                tracing::warn!(username, "SSH password auth: user not found");
                Ok(Auth::Reject { proceed_with_methods: None, partial_success: false })
            }
            Err(e) => {
                tracing::error!(error = %e, "DB error during password auth");
                Ok(Auth::Reject { proceed_with_methods: None, partial_success: false })
            }
        }
    }

    async fn auth_keyboard_interactive(
        &mut self,
        _user: &str,
        _submethods: &str,
        _response: Option<russh::server::Response<'_>>,
    ) -> Result<Auth, Self::Error> {
        Ok(Auth::Reject { proceed_with_methods: None, partial_success: false })
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

        // H-02: Validate repo_path before joining with repo_root
        rg_core::platform::validate_repo_path(&repo_path)
            .with_context(|| format!("invalid repository path: {}", repo_path))?;

        let repo_full_path = {
            let p = self.shared.repo_root.join(&repo_path);
            if p.exists() {
                p
            } else {
                let with_git = self.shared.repo_root.join(format!("{}.git", repo_path));
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

        let ch = match self.channel.take() {
            Some(ch) => ch,
            None => {
                let msg = "no channel available for exec_request";
                tracing::error!(msg);
                session.channel_failure(channel_id)?;
                return Err(HandlerError(msg.into()));
            }
        };

        session.channel_success(channel_id)?;

        let handle = session.handle();
        let service_name = service.clone();

        tokio::spawn(async move {
            tracing::info!(%service_name, path = %repo_full_path.display(), "Starting git SSH session");

            let mut stream: ChannelStream<Msg> = ch.into_stream();

            let result: Result<(), anyhow::Error> = match service_name.as_str() {
                "git-upload-pack" => handle_upload_pack_stream(&repo_full_path, &mut stream).await.map(|_| ()),
                "git-receive-pack" => handle_receive_pack_stream(&repo_full_path, &mut stream).await.map(|_| ()),
                _ => Err(anyhow::anyhow!("Unknown git service: {}", service_name)),
            };

            let exit_code: u32 = if result.is_ok() { 0 } else { 1 };

            match &result {
                Ok(_) => tracing::info!(%service_name, "Git SSH session complete"),
                Err(e) => tracing::error!(error = %e, %service_name, "Git SSH session failed"),
            }

            // CRITICAL: SSH stream shutdown order (踩坑经验)
            // 
            // Must send exit_status BEFORE shutting down the stream.
            // The russh client expects to receive the exit-status message before
            // the channel is closed. If we shutdown the stream first, the
            // exit_status message may be lost, causing the client to report
            // "connection closed unexpectedly" or exit code 255.
            //
            // Correct order:
            //   1. Send exit_status to client
            //   2. Shutdown the stream (which sends SSH_MSG_CHANNEL_CLOSE)
            //   3. Drop the channel (happens automatically when tokio::spawn future completes)
            if let Err(e) = handle.exit_status_request(channel_id, exit_code).await {
                tracing::warn!(error = ?e, "failed to send exit_status to client");
            }

            // Now safe to shutdown the stream - client has received exit_status
            if let Err(e) = stream.shutdown().await {
                tracing::warn!(error = ?e, "failed to shutdown SSH stream");
            }
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
