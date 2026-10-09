//! Live tests against a real server. Skipped unless `SAVOIA_MYSQL_URL` is set, e.g.
//! `SAVOIA_MYSQL_URL=mysql://savoia:savoia@127.0.0.1:33084/savoia` (docker compose).

use mysql_async::prelude::Queryable;
use savoia_core::{AppError, Driver, SslMode, parse_url};
use savoia_mysql::MysqlDriver;

fn target() -> Option<(savoia_core::ConnectionConfig, savoia_core::Secrets)> {
    let url = std::env::var("SAVOIA_MYSQL_URL").ok()?;
    Some(parse_url(&url).expect("SAVOIA_MYSQL_URL must be a valid mysql URL"))
}

async fn seed(url: &str) {
    let pool = mysql_async::Pool::new(url);
    let mut conn = pool.get_conn().await.unwrap();
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS it_widgets (id INT PRIMARY KEY AUTO_INCREMENT, name TEXT);
         CREATE OR REPLACE VIEW it_widget_names AS SELECT name FROM it_widgets;
         DROP FUNCTION IF EXISTS it_answer;
         CREATE FUNCTION it_answer() RETURNS INT DETERMINISTIC RETURN 42;",
    )
    .await
    .unwrap();
    drop(conn);
    pool.disconnect().await.unwrap();
}

#[tokio::test]
async fn connects_and_reads_catalog() {
    let Some((config, secrets)) = target() else {
        return;
    };
    seed(&std::env::var("SAVOIA_MYSQL_URL").unwrap()).await;

    let conn = MysqlDriver
        .connect(&config.endpoint(), &secrets)
        .await
        .expect("connect");
    let catalog = conn.catalog().await.expect("catalog");

    assert!(
        catalog.server.version.starts_with("MySQL "),
        "{}",
        catalog.server.version
    );
    let current = catalog
        .databases
        .iter()
        .find(|d| d.is_current)
        .expect("current db");
    assert_eq!(Some(current.name.as_str()), config.database.as_deref());
    let [schema] = current.schemas.as_slice() else {
        panic!("one schema per MySQL database")
    };
    assert!(schema.objects.tables.contains(&"it_widgets".to_string()));
    assert!(
        schema
            .objects
            .views
            .contains(&"it_widget_names".to_string())
    );
    assert!(schema.objects.functions.contains(&"it_answer".to_string()));
    assert!(
        catalog
            .databases
            .iter()
            .filter(|d| !d.is_current)
            .all(|d| d.schemas.is_empty())
    );
    conn.close().await;
}

#[tokio::test]
async fn tls_modes_against_self_signed_server() {
    let Some((mut config, secrets)) = target() else {
        return;
    };
    // MySQL 8 generates a self-signed certificate at first start.
    for mode in [SslMode::Disable, SslMode::Prefer, SslMode::Require] {
        config.ssl = mode;
        let conn = MysqlDriver
            .connect(&config.endpoint(), &secrets)
            .await
            .unwrap_or_else(|e| panic!("{mode:?}: {e}"));
        conn.server_info().await.expect("server info");
        conn.close().await;
    }
    config.ssl = SslMode::VerifyFull;
    let err = MysqlDriver
        .connect(&config.endpoint(), &secrets)
        .await
        .err()
        .expect("self-signed must fail");
    assert!(matches!(err, AppError::Connect { .. }), "{err:?}");
}

#[tokio::test]
async fn wrong_password_is_an_auth_error() {
    let Some((config, mut secrets)) = target() else {
        return;
    };
    secrets.password = Some("definitely-wrong".into());
    let err = MysqlDriver
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
    let err = MysqlDriver
        .connect(&config.endpoint(), &secrets)
        .await
        .err()
        .expect("must fail");
    assert!(matches!(err, AppError::Connect { .. }), "{err:?}");
}
