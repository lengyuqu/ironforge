//! Business logic for the remote credential dictionary.

use crate::auth::encryption::{decrypt, encrypt};
use crate::error::{CoreError, CoreResult};
use crate::net_guard;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use chrono::Utc;
use rg_db::entities::remote_credential::{
    ActiveModel, DEFAULT_BASIC_USERNAME, Model, AUTH_SCHEME_BASIC, AUTH_SCHEME_BEARER,
    OWNER_KIND_ORG, OWNER_KIND_USER,
};
use rg_db::entities::repository;
use sea_orm::ActiveValue::Set;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};

/// Shortest token accepted. Real platform tokens are 20+ characters; this only
/// rejects empty and obviously truncated input.
const MIN_TOKEN_LEN: usize = 4;
/// Longest token accepted, so a pasted private key cannot grow a row unbounded.
const MAX_TOKEN_LEN: usize = 8_192;
/// Longest host key accepted (`host:port` of a DNS name is far shorter).
const MAX_HOST_LEN: usize = 255;

/// Who a credential belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    User(i64),
    Org(i64),
}

impl Scope {
    /// Discriminator stored in `remote_credentials.owner_kind`.
    pub fn kind(&self) -> &'static str {
        match self {
            Scope::User(_) => OWNER_KIND_USER,
            Scope::Org(_) => OWNER_KIND_ORG,
        }
    }

    /// `users.id` or `organizations.id`.
    pub fn id(&self) -> i64 {
        match self {
            Scope::User(id) | Scope::Org(id) => *id,
        }
    }

    /// The scope a repository's outbound credentials come from.
    ///
    /// Org repositories use the org's dictionary; everything else uses the
    /// owner's. Deliberately not a fallback chain: a shared repository must not
    /// authenticate as whichever member triggered the sync.
    pub fn for_repository(repo: &repository::Model) -> Self {
        match repo.org_id {
            Some(org_id) => Scope::Org(org_id),
            None => Scope::User(repo.owner_id),
        }
    }
}

/// How the token is presented to the remote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthScheme {
    /// `Authorization: Basic base64(username:token)`.
    Basic,
    /// `Authorization: Bearer <token>`.
    Bearer,
}

impl AuthScheme {
    fn parse(raw: &str) -> CoreResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | AUTH_SCHEME_BASIC => Ok(AuthScheme::Basic),
            AUTH_SCHEME_BEARER => Ok(AuthScheme::Bearer),
            other => Err(CoreError::invalid_input(format!(
                "unknown auth scheme {other:?}: expected 'basic' or 'bearer'"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AuthScheme::Basic => AUTH_SCHEME_BASIC,
            AuthScheme::Bearer => AUTH_SCHEME_BEARER,
        }
    }
}

/// A decrypted credential, valid for the lifetime of one git invocation.
///
/// `token` is plaintext: it must never be logged, serialised, or persisted. It
/// has no `Serialize` on purpose, and `Debug` is hand-written to print `***`
/// instead of the token — `tracing::debug!(?credential)` is a realistic
/// mistake, and this is the layer that has to survive it.
pub struct ResolvedCredential {
    /// Row id, so the caller can record `last_used_at` after a successful call.
    pub credential_id: i64,
    pub username: String,
    pub token: String,
    pub scheme: AuthScheme,
}

impl std::fmt::Debug for ResolvedCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedCredential")
            .field("credential_id", &self.credential_id)
            .field("username", &self.username)
            .field("token", &"***")
            .field("scheme", &self.scheme)
            .finish()
    }
}

/// Fields accepted when creating or reconfiguring a host's credential.
#[derive(Debug, Deserialize)]
pub struct CredentialInput {
    /// Host or URL as typed by the user; normalised before storage.
    pub host: String,
    /// Basic-auth user. Ignored for `bearer`; defaults to `x-access-token`.
    #[serde(default)]
    pub username: Option<String>,
    pub token: String,
    /// `basic` (default) or `bearer`.
    #[serde(default)]
    pub auth_scheme: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

/// Fields that may be changed after creation. The host is not among them: it is
/// the lookup key, so changing it is a different credential (delete and add).
#[derive(Debug, Default, Deserialize)]
pub struct CredentialPatch {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub auth_scheme: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

/// API-safe projection of a credential: no ciphertext, only the last four
/// characters of the token so a user can tell two tokens apart.
#[derive(Debug, Serialize)]
pub struct CredentialView {
    pub id: i64,
    pub owner_kind: String,
    pub owner_id: i64,
    pub host: String,
    pub username: Option<String>,
    pub auth_scheme: String,
    pub label: Option<String>,
    /// `****abcd`, or `None` when the stored token cannot be decrypted (the
    /// instance secret changed) — the row then needs to be reconfigured.
    pub token_hint: Option<String>,
    pub last_used_at: Option<chrono::DateTime<Utc>>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

/// Every credential the scope configured, host-ordered.
pub async fn list(db: &DatabaseConnection, scope: Scope) -> CoreResult<Vec<Model>> {
    Ok(rg_db::ops::remote_credential_ops::list_for_owner(db, scope.kind(), scope.id()).await?)
}

/// List as API projections.
pub async fn list_views(
    db: &DatabaseConnection,
    scope: Scope,
    enc_key: &[u8; 32],
) -> CoreResult<Vec<CredentialView>> {
    Ok(list(db, scope)
        .await?
        .iter()
        .map(|model| view(model, enc_key))
        .collect())
}

/// One credential, verified to belong to `scope`.
pub async fn get(db: &DatabaseConnection, scope: Scope, id: i64) -> CoreResult<Model> {
    let model = rg_db::ops::remote_credential_ops::find_by_id(db, id)
        .await?
        .ok_or_else(|| CoreError::not_found("remote credential not found"))?;
    if model.owner_kind != scope.kind() || model.owner_id != scope.id() {
        // Same answer as a missing row: whether another owner has configured a
        // host is none of this caller's business.
        return Err(CoreError::not_found("remote credential not found"));
    }
    Ok(model)
}

/// Create the credential for a host, or replace the token of the existing one.
///
/// Upsert rather than create-or-conflict because the host *is* the identity:
/// re-submitting the same host is how a user rotates an expired token, and
/// making them delete the row first would leave a window where the mirror has
/// no credential at all.
pub async fn upsert(
    db: &DatabaseConnection,
    scope: Scope,
    input: CredentialInput,
    enc_key: &[u8; 32],
) -> CoreResult<Model> {
    let host = normalize_host(&input.host)?;
    validate_token(&input.token)?;
    let scheme = AuthScheme::parse(input.auth_scheme.as_deref().unwrap_or(""))?;
    let username = normalize_username(input.username, scheme);
    let label = normalize_label(input.label);
    let encrypted = encrypt(&input.token, enc_key)?;
    let now = Utc::now();

    if let Some(existing) =
        rg_db::ops::remote_credential_ops::find_for_owner_host(db, scope.kind(), scope.id(), &host)
            .await?
    {
        let mut model: ActiveModel = existing.into();
        model.token_encrypted = Set(encrypted);
        model.username = Set(username);
        model.auth_scheme = Set(scheme.as_str().to_string());
        if label.is_some() {
            model.label = Set(label);
        }
        model.updated_at = Set(now);
        return Ok(rg_db::ops::remote_credential_ops::update(db, model).await?);
    }

    let model = ActiveModel {
        owner_kind: Set(scope.kind().to_string()),
        owner_id: Set(scope.id()),
        host: Set(host),
        username: Set(username),
        token_encrypted: Set(encrypted),
        auth_scheme: Set(scheme.as_str().to_string()),
        label: Set(label),
        last_used_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    match rg_db::ops::remote_credential_ops::create(db, model).await {
        Ok(model) => Ok(model),
        // Two concurrent upserts: the loser hits the unique index and must end
        // up with the same "already configured" outcome as the winner.
        Err(error) if crate::user::service::is_unique_violation(&error) => Err(CoreError::conflict(
            "a credential for this host already exists for this owner",
        )),
        Err(error) => Err(error.into()),
    }
}

/// Change fields of an existing credential. The host cannot change.
pub async fn update(
    db: &DatabaseConnection,
    scope: Scope,
    id: i64,
    patch: CredentialPatch,
    enc_key: &[u8; 32],
) -> CoreResult<Model> {
    let existing = get(db, scope, id).await?;
    let mut model: ActiveModel = existing.into();

    if let Some(token) = patch.token.as_deref() {
        validate_token(token)?;
        model.token_encrypted = Set(encrypt(token, enc_key)?);
    }
    if let Some(raw) = patch.auth_scheme.as_deref() {
        model.auth_scheme = Set(AuthScheme::parse(raw)?.as_str().to_string());
    }
    if patch.username.is_some() {
        model.username = Set(patch.username);
    }
    if let Some(label) = patch.label {
        model.label = Set(normalize_label(Some(label)));
    }
    model.updated_at = Set(Utc::now());
    Ok(rg_db::ops::remote_credential_ops::update(db, model).await?)
}

/// Delete a credential.
pub async fn delete(db: &DatabaseConnection, scope: Scope, id: i64) -> CoreResult<()> {
    get(db, scope, id).await?;
    Ok(rg_db::ops::remote_credential_ops::delete_by_id(db, id).await?)
}

/// The lookup the feature exists for: the scope's token for whatever host `url`
/// points at.
///
/// `Ok(None)` means "no credential configured for this host", which is a normal
/// state (public repository). An undecryptable row is an error rather than a
/// silent `None`: the sync would otherwise fail with a 401 from the remote and
/// the operator would go looking in the wrong place.
pub async fn resolve(
    db: &DatabaseConnection,
    scope: Scope,
    url: &str,
    enc_key: &[u8; 32],
) -> CoreResult<Option<ResolvedCredential>> {
    let host = net_guard::url_host_key(url)
        .map_err(|e| CoreError::invalid_input(format!("invalid remote URL: {e}")))?;
    let Some(model) =
        rg_db::ops::remote_credential_ops::find_for_owner_host(db, scope.kind(), scope.id(), &host)
            .await?
    else {
        return Ok(None);
    };

    let token = decrypt(&model.token_encrypted, enc_key).map_err(|e| {
        CoreError::internal(format!(
            "stored token for host {host} cannot be decrypted (instance secret changed?) — \
             reconfigure the remote credential: {e}"
        ))
    })?;
    let scheme = AuthScheme::parse(&model.auth_scheme)?;
    let username = model
        .username
        .clone()
        .unwrap_or_else(|| DEFAULT_BASIC_USERNAME.to_string());

    Ok(Some(ResolvedCredential {
        credential_id: model.id,
        username,
        token,
        scheme,
    }))
}

/// Environment variables that make one git invocation authenticate with `cred`.
///
/// # Why the environment and not the URL or `-c`
///
/// `GIT_CONFIG_COUNT` / `GIT_CONFIG_KEY_<n>` / `GIT_CONFIG_VALUE_<n>` (git 2.31+)
/// inject a config entry for one process without writing it anywhere. The two
/// obvious alternatives leak:
///
/// * `https://user:token@host/...` is persisted into `.git/config` as
///   `remote.origin.url` and stays there.
/// * `git -c http.extraheader=...` puts the token in the process argument list,
///   readable by any local user through `ps`.
///
/// The config key is scoped to `<scheme>://<host>/`, so a redirect to another
/// host does not receive the header.
///
/// Returns an empty vector for transports that cannot carry an HTTP header
/// (`git://`): the token is simply not applicable there.
pub fn git_auth_env(url: &str, cred: &ResolvedCredential) -> CoreResult<Vec<(String, String)>> {
    let authority = net_guard::parse_remote_authority(url)
        .map_err(|e| CoreError::invalid_input(format!("invalid remote URL: {e}")))?;
    if !authority.is_http_transport() {
        return Ok(Vec::new());
    }

    let header = match cred.scheme {
        AuthScheme::Basic => {
            let raw = format!("{}:{}", cred.username, cred.token);
            format!("Authorization: Basic {}", BASE64.encode(raw))
        }
        AuthScheme::Bearer => format!("Authorization: Bearer {}", cred.token),
    };

    Ok(vec![
        ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
        (
            "GIT_CONFIG_KEY_0".to_string(),
            format!("http.{}.extraheader", authority.scoped_url()),
        ),
        ("GIT_CONFIG_VALUE_0".to_string(), header),
    ])
}

/// Project a row for the API, decrypting just enough to show a hint.
pub fn view(model: &Model, enc_key: &[u8; 32]) -> CredentialView {
    CredentialView {
        id: model.id,
        owner_kind: model.owner_kind.clone(),
        owner_id: model.owner_id,
        host: model.host.clone(),
        username: model.username.clone(),
        auth_scheme: model.auth_scheme.clone(),
        label: model.label.clone(),
        token_hint: token_hint(&model.token_encrypted, enc_key),
        last_used_at: model.last_used_at,
        created_at: model.created_at,
        updated_at: model.updated_at,
    }
}

/// Last four characters of the stored token, prefixed with `****`.
fn token_hint(encrypted: &str, enc_key: &[u8; 32]) -> Option<String> {
    let token = decrypt(encrypted, enc_key).ok()?;
    let tail: String = token
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Some(format!("****{tail}"))
}

fn normalize_host(input: &str) -> CoreResult<String> {
    let host = net_guard::normalize_host_input(input)
        .map_err(|e| CoreError::invalid_input(format!("invalid host: {e}")))?;
    if host.len() > MAX_HOST_LEN {
        return Err(CoreError::invalid_input(format!(
            "host is longer than {MAX_HOST_LEN} characters"
        )));
    }
    Ok(host)
}

fn validate_token(token: &str) -> CoreResult<()> {
    let len = token.chars().count();
    if len < MIN_TOKEN_LEN {
        return Err(CoreError::invalid_input(format!(
            "token must be at least {MIN_TOKEN_LEN} characters"
        )));
    }
    if len > MAX_TOKEN_LEN {
        return Err(CoreError::invalid_input(format!(
            "token must be at most {MAX_TOKEN_LEN} characters"
        )));
    }
    // A token that contains whitespace is almost always a copy-paste accident
    // (a wrapped header, a whole `Authorization:` line) and would produce an
    // unparsable header value further down.
    if token.chars().any(|c| c.is_whitespace()) {
        return Err(CoreError::invalid_input(
            "token must not contain whitespace".to_string(),
        ));
    }
    Ok(())
}

/// Basic needs a user; bearer has no use for one.
fn normalize_username(username: Option<String>, scheme: AuthScheme) -> Option<String> {
    let trimmed = username
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    match scheme {
        AuthScheme::Basic => Some(trimmed.unwrap_or_else(|| DEFAULT_BASIC_USERNAME.to_string())),
        AuthScheme::Bearer => None,
    }
}

fn normalize_label(label: Option<String>) -> Option<String> {
    label
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(128).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cred(username: &str, token: &str, scheme: AuthScheme) -> ResolvedCredential {
        ResolvedCredential {
            credential_id: 1,
            username: username.to_string(),
            token: token.to_string(),
            scheme,
        }
    }

    #[test]
    fn env_scopes_the_header_to_the_exact_host() {
        let env = git_auth_env(
            "https://github.com/acme/private.git",
            &cred("x-access-token", "ghp_example", AuthScheme::Basic),
        )
        .unwrap();
        let map: std::collections::HashMap<_, _> = env.into_iter().collect();
        assert_eq!(map["GIT_CONFIG_COUNT"], "1");
        assert_eq!(
            map["GIT_CONFIG_KEY_0"],
            "http.https://github.com/.extraheader"
        );
        assert!(map["GIT_CONFIG_VALUE_0"].starts_with("Authorization: Basic "));
        // base64("x-access-token:ghp_example")
        assert_eq!(
            map["GIT_CONFIG_VALUE_0"],
            "Authorization: Basic eC1hY2Nlc3MtdG9rZW46Z2hwX2V4YW1wbGU="
        );
    }

    #[test]
    fn env_keeps_a_non_default_port_in_the_scope() {
        let env = git_auth_env(
            "http://gitea.local:3000/team/repo.git",
            &cred("bot", "secret-token", AuthScheme::Basic),
        )
        .unwrap();
        let map: std::collections::HashMap<_, _> = env.into_iter().collect();
        assert_eq!(
            map["GIT_CONFIG_KEY_0"],
            "http.http://gitea.local:3000/.extraheader"
        );
    }

    #[test]
    fn env_uses_bearer_when_configured() {
        let env = git_auth_env(
            "https://gitlab.com/group/project.git",
            &cred("", "glpat-token", AuthScheme::Bearer),
        )
        .unwrap();
        let map: std::collections::HashMap<_, _> = env.into_iter().collect();
        assert_eq!(
            map["GIT_CONFIG_VALUE_0"],
            "Authorization: Bearer glpat-token"
        );
    }

    #[test]
    fn env_is_empty_for_git_protocol() {
        // git:// is anonymous; no header can be attached, and inventing one
        // would be silently ineffective.
        let env = git_auth_env(
            "git://github.com/acme/private.git",
            &cred("x", "token-value", AuthScheme::Basic),
        )
        .unwrap();
        assert!(env.is_empty());
    }

    #[test]
    fn a_token_never_reaches_the_url_it_authenticates() {
        // The whole point of the env mechanism: the URL handed to git is the
        // configured remote verbatim, with no credentials spliced in.
        let url = "https://github.com/acme/private.git";
        let env = git_auth_env(url, &cred("bot", "super-secret-token", AuthScheme::Basic)).unwrap();
        for (key, value) in env {
            assert!(!key.contains("super-secret-token"));
            assert!(!value.contains(url), "value must not carry the URL");
        }
    }

    #[test]
    fn token_validation_rejects_empty_and_whitespace() {
        assert!(validate_token("").is_err());
        assert!(validate_token("abc").is_err());
        assert!(validate_token("abc defgh").is_err());
        assert!(validate_token("abcd").is_ok());
    }

    #[test]
    fn host_input_accepts_bare_hosts_and_urls() {
        assert_eq!(normalize_host("GitHub.com").unwrap(), "github.com");
        assert_eq!(
            normalize_host("https://github.com/acme/repo").unwrap(),
            "github.com"
        );
        assert_eq!(
            normalize_host("gitea.local:3000").unwrap(),
            "gitea.local:3000"
        );
        assert_eq!(normalize_host("  gitlab.com  ").unwrap(), "gitlab.com");
        assert!(normalize_host("").is_err());
    }

    #[test]
    fn bearer_rows_drop_the_username() {
        assert_eq!(
            normalize_username(Some("someone".to_string()), AuthScheme::Bearer),
            None
        );
        assert_eq!(
            normalize_username(None, AuthScheme::Basic),
            Some(DEFAULT_BASIC_USERNAME.to_string())
        );
    }

    #[test]
    fn a_repository_with_an_org_resolves_to_the_org_scope() {
        let mut repo = repository::Model {
            id: 1,
            owner_id: 42,
            name: "repo".to_string(),
            description: None,
            is_private: false,
            default_branch: "main".to_string(),
            fork_id: None,
            stars_count: 0,
            forks_count: 0,
            org_id: Some(7),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
            origin_repo_id: None,
        };
        assert_eq!(Scope::for_repository(&repo), Scope::Org(7));
        repo.org_id = None;
        assert_eq!(Scope::for_repository(&repo), Scope::User(42));
    }

    async fn migrated_db() -> (DatabaseConnection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let url = format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("remote_credentials.db").display()
        );
        let db = sea_orm::Database::connect(sea_orm::ConnectOptions::new(url))
            .await
            .unwrap();
        rg_db::run_migrations(&db).await.unwrap();
        (db, dir)
    }

    fn input(host: &str, token: &str) -> CredentialInput {
        CredentialInput {
            host: host.to_string(),
            username: None,
            token: token.to_string(),
            auth_scheme: None,
            label: None,
        }
    }

    /// The whole chain, against a real database: normalise the typed host →
    /// encrypt → store → look up by a *different* URL on the same host →
    /// decrypt → build the git environment. Every step is where a "configure it
    /// once" feature usually breaks.
    #[tokio::test]
    async fn a_host_token_round_trips_from_the_database_to_the_git_environment() {
        let (db, _dir) = migrated_db().await;
        let key = crate::auth::encryption::derive_key("test-instance-secret");

        let created = upsert(
            &db,
            Scope::User(1),
            input("https://github.com/acme/repo", "ghp_example_token"),
            &key,
        )
        .await
        .unwrap();
        assert_eq!(created.host, "github.com", "a full URL normalises to its host");
        assert_eq!(created.auth_scheme, AUTH_SCHEME_BASIC);
        assert!(
            !created.token_encrypted.contains("ghp_example_token"),
            "the token must never be stored in the clear"
        );

        // A *different* repository on the same host is the point of the
        // dictionary: same token, no second configuration.
        let resolved = resolve(
            &db,
            Scope::User(1),
            "https://github.com/acme/other-repo.git",
            &key,
        )
        .await
        .unwrap()
        .expect("the host is configured");
        assert_eq!(resolved.token, "ghp_example_token");
        assert_eq!(resolved.username, DEFAULT_BASIC_USERNAME);

        let env: std::collections::HashMap<String, String> =
            git_auth_env("https://github.com/acme/other-repo.git", &resolved)
                .unwrap()
                .into_iter()
                .collect();
        assert_eq!(env["GIT_CONFIG_KEY_0"], "http.https://github.com/.extraheader");
        assert!(env["GIT_CONFIG_VALUE_0"].starts_with("Authorization: Basic "));

        // Re-submitting the same host rotates in place instead of adding a row.
        let rotated = upsert(&db, Scope::User(1), input("GitHub.com", "ghp_rotated"), &key)
            .await
            .unwrap();
        assert_eq!(rotated.id, created.id, "upsert by host, not a new row");
        assert_eq!(list(&db, Scope::User(1)).await.unwrap().len(), 1);

        // Scopes are isolated: neither another user nor an org sees this token.
        assert!(resolve(&db, Scope::User(2), "https://github.com/acme/repo", &key)
            .await
            .unwrap()
            .is_none());
        assert!(resolve(&db, Scope::Org(1), "https://github.com/acme/repo", &key)
            .await
            .unwrap()
            .is_none());
    }

    /// A row encrypted under a different instance secret must be an error, not
    /// a silent "no credential" — otherwise the sync fails with a 401 from the
    /// remote and the operator looks in the wrong place.
    #[tokio::test]
    async fn an_undecryptable_row_is_reported_not_ignored() {
        let (db, _dir) = migrated_db().await;
        let writer_key = crate::auth::encryption::derive_key("old-secret");
        let reader_key = crate::auth::encryption::derive_key("new-secret");

        upsert(&db, Scope::User(1), input("gitlab.com", "glpat-example"), &writer_key)
            .await
            .unwrap();

        let error = resolve(&db, Scope::User(1), "https://gitlab.com/a/b.git", &reader_key)
            .await
            .expect_err("a ciphertext that cannot be opened is an error");
        assert!(
            format!("{error}").contains("cannot be decrypted"),
            "the message must name the actual problem: {error}"
        );
    }
}
