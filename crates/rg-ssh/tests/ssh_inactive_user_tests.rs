//! A deactivated account must not authenticate over SSH, by password or by
//! public key (issue #43). Each case first proves the credential works while
//! the account is active, so a rejection afterwards can only come from the
//! deactivation.

use std::sync::Arc;

use russh::client;
use russh::keys::{Algorithm, PrivateKey};
use sea_orm::{ConnectOptions, Database, DatabaseConnection, Set};

struct AcceptAnyHostKey;

impl client::Handler for AcceptAnyHostKey {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    db: DatabaseConnection,
    addr: String,
}

async fn start_server() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let mut options = ConnectOptions::new(format!("sqlite://{}?mode=rwc", db_path.display()));
    options.max_connections(2).min_connections(1);
    let db = Database::connect(options).await.unwrap();
    rg_db::run_migrations(&db).await.unwrap();

    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap().to_string();
    drop(probe);
    let config = rg_ssh::SshServerConfig {
        host_key_path: dir.path().join("host_ed25519"),
        listen_addr: addr.clone(),
        repo_root: dir.path().join("repos"),
        db: Some(db.clone()),
    };
    tokio::spawn(async move {
        rg_ssh::start_ssh_server(config).await.unwrap();
    });
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(&addr).await.is_ok() {
            return Harness {
                _dir: dir,
                db,
                addr,
            };
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("SSH listener did not start on {addr}");
}

async fn connect(addr: &str) -> client::Handle<AcceptAnyHostKey> {
    client::connect(Arc::new(client::Config::default()), addr, AcceptAnyHostKey)
        .await
        .unwrap()
}

async fn set_active(db: &DatabaseConnection, user_id: i64, active: bool) {
    rg_db::ops::user_ops::update_by_id(db, user_id, None, None, None, Some(active))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deactivated_account_cannot_authenticate_with_a_password() {
    let h = start_server().await;
    let hash = rg_core::auth::password::hash_password("correct horse battery staple").unwrap();
    let user = rg_db::ops::user_ops::create_user(&h.db, "pw-user", "pw@example.com", &hash, "Pw")
        .await
        .unwrap();

    let mut session = connect(&h.addr).await;
    let result = session
        .authenticate_password("pw-user", "correct horse battery staple")
        .await
        .unwrap();
    assert!(
        result.success(),
        "an active account must be able to sign in"
    );

    set_active(&h.db, user.id, false).await;

    let mut session = connect(&h.addr).await;
    let result = session
        .authenticate_password("pw-user", "correct horse battery staple")
        .await
        .unwrap();
    assert!(
        !result.success(),
        "a deactivated account must not authenticate with its password"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deactivated_account_cannot_authenticate_with_a_public_key() {
    let h = start_server().await;
    let user = rg_db::ops::user_ops::create_user(&h.db, "key-user", "key@example.com", "", "Key")
        .await
        .unwrap();

    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let public_key = key.public_key().to_openssh().unwrap();
    let fingerprint = rg_core::auth::ssh_key::fingerprint_from_openssh(&public_key).unwrap();
    rg_db::ops::ssh_key_ops::create(
        &h.db,
        rg_db::entities::ssh_key::ActiveModel {
            id: sea_orm::NotSet,
            user_id: Set(user.id),
            title: Set("inactive-user test".to_string()),
            public_key: Set(public_key),
            fingerprint: Set(fingerprint),
            created_at: Set(chrono::Utc::now()),
            last_used_at: Set(None),
        },
    )
    .await
    .unwrap();
    let key = Arc::new(key);

    let mut session = connect(&h.addr).await;
    let result = session
        .authenticate_publickey(
            "git",
            russh::keys::PrivateKeyWithHashAlg::new(key.clone(), None),
        )
        .await
        .unwrap();
    assert!(result.success(), "an active account's key must be accepted");

    set_active(&h.db, user.id, false).await;

    let mut session = connect(&h.addr).await;
    let result = session
        .authenticate_publickey("git", russh::keys::PrivateKeyWithHashAlg::new(key, None))
        .await
        .unwrap();
    assert!(
        !result.success(),
        "a deactivated account's key must not authenticate"
    );
}
