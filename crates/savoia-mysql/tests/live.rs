//! Live tests against a real server. Skipped unless `SAVOIA_MYSQL_URL` is set, e.g.
//! `SAVOIA_MYSQL_URL=mysql://savoia:savoia@127.0.0.1:33084/savoia` (docker compose).

use mysql_async::prelude::Queryable;
use savoia_core::{AppError, Driver, SslMode, TableKind, parse_url};
use savoia_mysql::MysqlDriver;

fn target() -> Option<(savoia_core::ConnectionConfig, savoia_core::Secrets)> {
    let url = std::env::var("SAVOIA_MYSQL_URL").ok()?;
    Some(parse_url(&url).expect("SAVOIA_MYSQL_URL must be a valid mysql URL"))
}

/// Seeds once per test binary; concurrent DDL on the same objects can race.
async fn seed(url: &str) {
    static SEEDED: tokio::sync::Mutex<bool> = tokio::sync::Mutex::const_new(false);
    let mut seeded = SEEDED.lock().await;
    if *seeded {
        return;
    }
    *seeded = true;
    let pool = mysql_async::Pool::new(url);
    let mut conn = pool.get_conn().await.unwrap();
    conn.query_drop(
        "CREATE TABLE IF NOT EXISTS it_widgets (id INT PRIMARY KEY AUTO_INCREMENT, name TEXT);
         CREATE OR REPLACE VIEW it_widget_names AS SELECT name FROM it_widgets;
         DROP FUNCTION IF EXISTS it_answer;
         CREATE FUNCTION it_answer() RETURNS INT DETERMINISTIC RETURN 42;
         CREATE TABLE IF NOT EXISTS it_parts (
           widget_id INT NOT NULL,
           part_no INT NOT NULL,
           label VARCHAR(40) DEFAULT 'none',
           PRIMARY KEY (widget_id, part_no),
           KEY it_parts_label (label),
           CONSTRAINT it_parts_widget FOREIGN KEY (widget_id) REFERENCES it_widgets (id));",
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
    assert_eq!(schema.objects, None, "names load on demand");
    assert!(schema.counts.tables >= 2 && schema.counts.views >= 1 && schema.counts.functions >= 1);
    // Every database is browsable from the one connection.
    let system = catalog
        .databases
        .iter()
        .find(|d| d.name == "information_schema")
        .expect("information_schema");
    assert!(system.schemas[0].counts.views > 0);

    let objects = conn
        .list_objects(&current.name, &current.name)
        .await
        .expect("objects");
    assert!(objects.tables.contains(&"it_widgets".to_string()));
    assert!(objects.tables.contains(&"it_parts".to_string()));
    assert!(objects.views.contains(&"it_widget_names".to_string()));
    assert!(objects.functions.contains(&"it_answer".to_string()));
    conn.close().await;
}

#[tokio::test]
async fn describes_tables_and_their_keys() {
    let Some((config, secrets)) = target() else {
        return;
    };
    seed(&std::env::var("SAVOIA_MYSQL_URL").unwrap()).await;
    let db = config.database.clone().unwrap();
    let conn = MysqlDriver
        .connect(&config.endpoint(), &secrets)
        .await
        .expect("connect");

    let parts = conn
        .describe_table(&db, &db, "it_parts")
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
            ("widget_id", "int", false),
            ("part_no", "int", false),
            ("label", "varchar(40)", true),
        ]
    );
    assert_eq!(parts.columns[2].default.as_deref(), Some("none"));
    assert_eq!(parts.primary_key, ["widget_id", "part_no"]);
    let [fk] = parts.foreign_keys.as_slice() else {
        panic!("one foreign key: {:?}", parts.foreign_keys)
    };
    assert_eq!(fk.name, "it_parts_widget");
    assert_eq!(
        (fk.ref_schema.as_str(), fk.ref_table.as_str()),
        (db.as_str(), "it_widgets")
    );
    assert_eq!(
        (fk.columns.as_slice(), fk.ref_columns.as_slice()),
        (&["widget_id".to_string()][..], &["id".to_string()][..])
    );
    let label = parts
        .indexes
        .iter()
        .find(|i| i.name == "it_parts_label")
        .expect("label index");
    assert!(!label.unique && label.columns == ["label"]);

    let all = conn.describe_schema(&db, &db).await.expect("schema");
    let view = all
        .iter()
        .find(|t| t.name == "it_widget_names")
        .expect("view");
    assert_eq!(view.kind, TableKind::View);
    assert!(
        all.iter()
            .any(|t| t.name == "it_widgets" && t.primary_key == ["id"])
    );

    let missing = conn.describe_table(&db, &db, "nope").await;
    assert!(
        matches!(missing, Err(AppError::Query { .. })),
        "{missing:?}"
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
