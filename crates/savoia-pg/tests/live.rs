//! Live tests against a real server. Skipped unless `SAVOIA_PG_URL` is set, e.g.
//! `SAVOIA_PG_URL=postgres://savoia:savoia@127.0.0.1:54317/savoia` (docker compose).

use savoia_core::{AppError, Driver, SslMode, parse_url};
use savoia_pg::PgDriver;

fn target() -> Option<(savoia_core::ConnectionConfig, savoia_core::Secrets)> {
    let url = std::env::var("SAVOIA_PG_URL").ok()?;
    Some(parse_url(&url).expect("SAVOIA_PG_URL must be a valid postgres URL"))
}

async fn seed(url: &str) {
    let (client, conn) = tokio_postgres::connect(url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(conn);
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS it_catalog;
             CREATE TABLE IF NOT EXISTS it_catalog.widgets (id serial PRIMARY KEY, name text);
             CREATE OR REPLACE VIEW it_catalog.widget_names AS SELECT name FROM it_catalog.widgets;
             CREATE OR REPLACE FUNCTION it_catalog.answer() RETURNS int LANGUAGE sql AS 'SELECT 42';",
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn connects_and_reads_catalog() {
    let Some((mut config, secrets)) = target() else {
        return;
    };
    seed(&std::env::var("SAVOIA_PG_URL").unwrap()).await;
    config.ssl = SslMode::Disable;

    let conn = PgDriver
        .connect(&config.endpoint(), &secrets)
        .await
        .expect("connect");
    let catalog = conn.catalog().await.expect("catalog");

    assert!(
        catalog.server.version.starts_with("PostgreSQL "),
        "{}",
        catalog.server.version
    );
    let current = catalog
        .databases
        .iter()
        .find(|d| d.is_current)
        .expect("current db");
    assert_eq!(Some(current.name.as_str()), config.database.as_deref());
    let schema = current
        .schemas
        .iter()
        .find(|s| s.name == "it_catalog")
        .expect("seeded schema");
    assert!(schema.objects.tables.contains(&"widgets".to_string()));
    assert!(schema.objects.views.contains(&"widget_names".to_string()));
    assert!(schema.objects.functions.contains(&"answer".to_string()));
    assert!(
        schema
            .objects
            .sequences
            .contains(&"widgets_id_seq".to_string())
    );
    assert!(current.schemas.iter().all(|s| !s.name.starts_with("pg_")));
    conn.close().await;
}

#[tokio::test]
async fn prefer_and_require_accept_a_self_signed_cert() {
    let Some((mut config, secrets)) = target() else {
        return;
    };
    for mode in [SslMode::Prefer, SslMode::Require] {
        config.ssl = mode;
        let conn = PgDriver
            .connect(&config.endpoint(), &secrets)
            .await
            .expect("connect");
        conn.server_info().await.expect("server info");
    }
}

#[tokio::test]
async fn verify_full_rejects_a_self_signed_cert() {
    let Some((mut config, secrets)) = target() else {
        return;
    };
    config.ssl = SslMode::VerifyFull;
    let err = PgDriver
        .connect(&config.endpoint(), &secrets)
        .await
        .err()
        .expect("must fail");
    assert!(matches!(err, AppError::Connect { .. }), "{err:?}");
    assert!(
        err.to_string().to_lowercase().contains("certificate"),
        "{err}"
    );
}

#[tokio::test]
async fn require_uses_tls() {
    let Some((mut config, secrets)) = target() else {
        return;
    };
    config.ssl = SslMode::Require;
    let url = std::env::var("SAVOIA_PG_URL").unwrap();
    let (client, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(conn);
    let ours = PgDriver
        .connect(&config.endpoint(), &secrets)
        .await
        .expect("connect");
    let used_tls: bool = client
        .query_one(
            "SELECT bool_or(s.ssl) FROM pg_stat_ssl s JOIN pg_stat_activity a USING (pid) \
             WHERE a.application_name = 'Savoia DB'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert!(used_tls);
    ours.close().await;
}

#[tokio::test]
async fn wrong_password_is_an_auth_error() {
    let Some((config, mut secrets)) = target() else {
        return;
    };
    secrets.password = Some("definitely-wrong".into());
    let err = PgDriver
        .connect(&config.endpoint(), &secrets)
        .await
        .err()
        .expect("must fail");
    assert!(matches!(err, AppError::Auth { .. }), "{err:?}");
}

#[tokio::test]
async fn unreachable_port_is_a_connect_error() {
    let Some((mut config, secrets)) = target() else {
        return;
    };
    config.port = 1;
    let err = PgDriver
        .connect(&config.endpoint(), &secrets)
        .await
        .err()
        .expect("must fail");
    assert!(matches!(err, AppError::Connect { .. }), "{err:?}");
}
