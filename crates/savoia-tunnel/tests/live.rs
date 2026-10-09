//! Live tests through the docker-compose `ssh` bastion. Skipped unless
//! `SAVOIA_SSH_TEST=1`. Uses a temp known_hosts file, never `~/.ssh`.

use savoia_core::{
    AppError, ConnectionConfig, Driver, Engine, Secrets, SshAuth, SshConfig, SslMode,
};
use savoia_pg::PgDriver;
use savoia_tunnel::{HostKeyPolicy, KnownHosts, open};

fn enabled() -> bool {
    std::env::var("SAVOIA_SSH_TEST").is_ok_and(|v| v == "1")
}

fn bastion() -> SshConfig {
    SshConfig {
        host: "127.0.0.1".into(),
        port: 2222,
        user: "savoia".into(),
        auth: SshAuth::Password,
    }
}

fn secrets() -> Secrets {
    Secrets {
        password: Some("savoia".into()),
        ssh_password: Some("savoia".into()),
        ..Default::default()
    }
}

fn temp_known_hosts() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "savoia-known-hosts-{}",
        savoia_core::ConnectionId::new()
    ))
}

#[tokio::test]
async fn unknown_host_then_trust_then_known() {
    if !enabled() {
        return;
    }
    let file = temp_known_hosts();
    let target = ("postgres-17".to_string(), 5432);

    let err = open(
        &bastion(),
        &secrets(),
        target.clone(),
        HostKeyPolicy::KnownOnly,
        KnownHosts::File(file.clone()),
    )
    .await
    .err()
    .expect("unknown host must be refused");
    let AppError::UnknownHostKey { fingerprint, .. } = err else {
        panic!("{err:?}")
    };
    assert!(fingerprint.starts_with("SHA256:"), "{fingerprint}");

    let tunnel = open(
        &bastion(),
        &secrets(),
        target.clone(),
        HostKeyPolicy::TrustUnknown,
        KnownHosts::File(file.clone()),
    )
    .await
    .expect("trusted");
    tunnel.close().await;
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("[127.0.0.1]:2222")
    );

    let tunnel = open(
        &bastion(),
        &secrets(),
        target,
        HostKeyPolicy::KnownOnly,
        KnownHosts::File(file.clone()),
    )
    .await
    .expect("now known");
    tunnel.close().await;
    std::fs::remove_file(file).unwrap();
}

#[tokio::test]
async fn postgres_through_the_tunnel() {
    if !enabled() {
        return;
    }
    let file = temp_known_hosts();
    let tunnel = open(
        &bastion(),
        &secrets(),
        ("postgres-17".into(), 5432),
        HostKeyPolicy::TrustUnknown,
        KnownHosts::File(file.clone()),
    )
    .await
    .expect("tunnel");

    let mut config = ConnectionConfig::new(Engine::Postgres);
    config.host = "postgres-17".into(); // only resolvable inside the docker network
    config.user = "savoia".into();
    config.database = Some("savoia".into());
    config.ssl = SslMode::Require;

    // Two sessions over one tunnel, to check concurrent channels.
    let endpoint = config.endpoint_via("127.0.0.1", tunnel.local_port());
    let a = PgDriver
        .connect(&endpoint, &secrets())
        .await
        .expect("first session");
    let b = PgDriver
        .connect(&endpoint, &secrets())
        .await
        .expect("second session");
    assert!(
        a.server_info()
            .await
            .unwrap()
            .version
            .starts_with("PostgreSQL")
    );
    assert!(
        b.catalog()
            .await
            .unwrap()
            .databases
            .iter()
            .any(|d| d.is_current)
    );

    drop((a, b));
    tunnel.close().await;
    std::fs::remove_file(file).unwrap();
}

#[tokio::test]
async fn wrong_ssh_password_fails_cleanly() {
    if !enabled() {
        return;
    }
    let file = temp_known_hosts();
    let bad = Secrets {
        ssh_password: Some("nope".into()),
        ..secrets()
    };
    let err = open(
        &bastion(),
        &bad,
        ("postgres-17".into(), 5432),
        HostKeyPolicy::TrustUnknown,
        KnownHosts::File(file.clone()),
    )
    .await
    .err()
    .expect("must fail");
    assert!(matches!(err, AppError::Ssh { .. }), "{err:?}");
    assert!(err.to_string().contains("authentication failed"), "{err}");
    let _ = std::fs::remove_file(file);
}
