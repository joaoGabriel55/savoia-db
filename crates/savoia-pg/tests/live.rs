//! Live tests against a real server. Skipped unless `SAVOIA_PG_URL` is set, e.g.
//! `SAVOIA_PG_URL=postgres://savoia:savoia@127.0.0.1:54317/savoia` (docker compose).

use savoia_core::{AppError, Driver, SslMode, TableKind, parse_url};
use savoia_pg::PgDriver;

fn target() -> Option<(savoia_core::ConnectionConfig, savoia_core::Secrets)> {
    let url = std::env::var("SAVOIA_PG_URL").ok()?;
    Some(parse_url(&url).expect("SAVOIA_PG_URL must be a valid postgres URL"))
}

/// Seeds once per test binary; concurrent `CREATE ... IF NOT EXISTS` can race.
async fn seed(url: &str) {
    static SEEDED: tokio::sync::Mutex<bool> = tokio::sync::Mutex::const_new(false);
    let mut seeded = SEEDED.lock().await;
    if *seeded {
        return;
    }
    *seeded = true;
    let (client, conn) = tokio_postgres::connect(url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(conn);
    client
        .batch_execute(
            "CREATE SCHEMA IF NOT EXISTS it_catalog;
             CREATE TABLE IF NOT EXISTS it_catalog.widgets (id serial PRIMARY KEY, name text);
             CREATE OR REPLACE VIEW it_catalog.widget_names AS SELECT name FROM it_catalog.widgets;
             CREATE OR REPLACE FUNCTION it_catalog.answer() RETURNS int LANGUAGE sql AS 'SELECT 42';
             CREATE TABLE IF NOT EXISTS it_catalog.parts (
               widget_id int NOT NULL REFERENCES it_catalog.widgets (id),
               part_no int NOT NULL,
               label varchar(40) DEFAULT 'none',
               PRIMARY KEY (widget_id, part_no));
             CREATE INDEX IF NOT EXISTS parts_label_lower ON it_catalog.parts (lower(label));",
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
    assert_eq!(schema.objects, None, "names load on demand");
    assert_eq!(
        (
            schema.counts.tables,
            schema.counts.views,
            schema.counts.functions
        ),
        (2, 1, 1)
    );
    assert_eq!(schema.counts.sequences, 1);
    assert!(current.schemas.iter().all(|s| !s.name.starts_with("pg_")));

    let db = current.name.clone();
    let objects = conn.list_objects(&db, "it_catalog").await.expect("objects");
    assert_eq!(objects.tables, ["parts", "widgets"]);
    assert_eq!(objects.views, ["widget_names"]);
    assert_eq!(objects.functions, ["answer"]);
    assert_eq!(objects.sequences, ["widgets_id_seq"]);
    conn.close().await;
}

#[tokio::test]
async fn describes_tables_and_their_keys() {
    let Some((mut config, secrets)) = target() else {
        return;
    };
    seed(&std::env::var("SAVOIA_PG_URL").unwrap()).await;
    config.ssl = SslMode::Disable;
    let db = config.database.clone().unwrap();
    let conn = PgDriver
        .connect(&config.endpoint(), &secrets)
        .await
        .expect("connect");

    let parts = conn
        .describe_table(&db, "it_catalog", "parts")
        .await
        .expect("describe");
    assert_eq!(parts.kind, TableKind::Table);
    let columns: Vec<_> = parts
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c.data_type.as_str(), c.nullable))
        .collect();
    assert_eq!(
        columns,
        [
            ("widget_id", "integer", false),
            ("part_no", "integer", false),
            ("label", "character varying(40)", true),
        ]
    );
    assert_eq!(
        parts.columns[2].default.as_deref(),
        Some("'none'::character varying")
    );
    assert_eq!(parts.primary_key, ["widget_id", "part_no"]);
    let fk = &parts.foreign_keys[0];
    assert_eq!(
        (
            fk.columns.as_slice(),
            fk.ref_schema.as_str(),
            fk.ref_table.as_str()
        ),
        (&["widget_id".to_string()][..], "it_catalog", "widgets")
    );
    assert_eq!(fk.ref_columns, ["id"]);
    let lower = parts
        .indexes
        .iter()
        .find(|i| i.name == "parts_label_lower")
        .expect("expression index");
    assert_eq!(lower.columns, ["lower(label::text)"]);
    assert!(parts.indexes.iter().any(|i| i.primary && i.unique));

    let all = conn
        .describe_schema(&db, "it_catalog")
        .await
        .expect("schema");
    let names: Vec<_> = all.iter().map(|t| (t.name.as_str(), t.kind)).collect();
    assert_eq!(
        names,
        [
            ("parts", TableKind::Table),
            ("widget_names", TableKind::View),
            ("widgets", TableKind::Table),
        ]
    );

    let missing = conn.describe_table(&db, "it_catalog", "nope").await;
    assert!(
        matches!(missing, Err(AppError::Query { .. })),
        "{missing:?}"
    );
    let other_db = conn.list_objects("template1", "public").await;
    assert!(
        matches!(other_db, Err(AppError::Query { .. })),
        "{other_db:?}"
    );
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
             WHERE a.application_name = 'Savoia Studio'",
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
