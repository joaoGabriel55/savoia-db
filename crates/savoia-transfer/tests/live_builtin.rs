//! Round trips through the built-in exporter: seed, export, restore with the
//! real client, and compare. Skipped unless `SAVOIA_PG_URL` /
//! `SAVOIA_MYSQL_URL` is set and `psql` / `mysql` is installed.

use std::io::Read;
use std::path::PathBuf;

use savoia_core::{Connection, Driver, Engine, QueryEvent, SslMode, parse_url};
use savoia_transfer::builtin::{ExportEvent, ExportRequest, OutputFormat, export};
use savoia_transfer::runner::{RunEnd, ToolCommand};
use savoia_transfer::tools::{Tool, ToolSearch, detect};

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("savoia-builtin-{}-{name}", uuid::Uuid::new_v4()))
}

/// Every row of every result, as text, joined into one string per row.
async fn fetch(conn: &dyn Connection, sql: &str) -> Vec<String> {
    let mut handle = conn.execute(sql.to_owned()).await.unwrap();
    let mut out = Vec::new();
    while let Some(event) = handle.next().await {
        if let QueryEvent::Rows(rows) = event.unwrap() {
            for row in rows {
                let cells: Vec<&str> = row.iter().map(|c| c.as_deref().unwrap_or("NULL")).collect();
                out.push(cells.join(" | "));
            }
        }
    }
    out
}

async fn restore(tool: Tool, url_var: &str, dump: &PathBuf) {
    let (mut config, secrets) = parse_url(&std::env::var(url_var).unwrap()).unwrap();
    config.ssl = SslMode::Disable;
    let found = detect(tool, &ToolSearch::system(None)).unwrap();
    let mut command =
        ToolCommand::connect(&found, &config.endpoint(), secrets.password.as_deref()).unwrap();
    match tool {
        Tool::Psql => {
            command
                .arg("--set=ON_ERROR_STOP=1")
                .arg("--quiet")
                .arg("--file")
                .arg(dump);
        }
        _ => {
            command.stdin_file(dump);
        }
    }
    let (end, log) = command.spawn().unwrap().wait().await;
    assert_eq!(end, RunEnd::Succeeded, "{}", log.join("\n"));
}

const PG_SEED: &str = "
DROP SCHEMA IF EXISTS it_export CASCADE;
CREATE SCHEMA it_export;
CREATE TYPE it_export.mood AS ENUM ('ok', 'meh', 'it''s bad');
CREATE TABLE it_export.customers (
  id serial PRIMARY KEY,
  name text NOT NULL UNIQUE CHECK (name <> ''),
  mood it_export.mood,
  note text,
  data bytea);
CREATE TABLE it_export.orders (
  id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  customer_id int NOT NULL REFERENCES it_export.customers (id) ON DELETE CASCADE,
  total numeric(10,2) DEFAULT 0,
  doubled numeric GENERATED ALWAYS AS (total * 2) STORED);
CREATE INDEX orders_total_text ON it_export.orders (lower(total::text));
CREATE SEQUENCE it_export.tickets START 1000 INCREMENT 5;
SELECT nextval('it_export.tickets'), nextval('it_export.tickets');
CREATE VIEW it_export.big AS SELECT * FROM it_export.orders WHERE total > 10;
CREATE VIEW it_export.bigger AS SELECT * FROM it_export.big WHERE total > 100;
INSERT INTO it_export.customers (name, mood, note, data)
  SELECT 'c' || i || CASE WHEN i % 7 = 0 THEN ' O''Brien, \"q\"' ELSE '' END,
         (ARRAY['ok', 'meh', 'it''s bad', NULL])[i % 4 + 1]::it_export.mood,
         CASE i % 3 WHEN 0 THEN NULL WHEN 1 THEN '' ELSE E'tab\\there\\nnewline \\\\ back' END,
         CASE WHEN i % 5 = 0 THEN NULL ELSE decode(lpad(to_hex(i), 4, '0'), 'hex') END
  FROM generate_series(1, 250) i;
INSERT INTO it_export.orders (customer_id, total)
  SELECT (i % 250) + 1, i * 1.5 FROM generate_series(1, 420) i;
";

const PG_FINGERPRINT: &str = "
SELECT 'col', c.relname, a.attname, format_type(a.atttypid, a.atttypmod), a.attnotnull::text,
       coalesce(pg_get_expr(d.adbin, d.adrelid), ''), a.attidentity::text, a.attgenerated::text
  FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid
  LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
  WHERE c.relnamespace = 'it_export'::regnamespace AND a.attnum > 0 AND NOT a.attisdropped
  ORDER BY 2, 3;
SELECT 'con', conrelid::regclass::text, conname, pg_get_constraintdef(oid)
  FROM pg_constraint WHERE connamespace = 'it_export'::regnamespace AND contype <> 'n' ORDER BY 2, 3;
SELECT 'idx', indexname, indexdef FROM pg_indexes WHERE schemaname = 'it_export' ORDER BY 2;
SELECT 'view', viewname, definition FROM pg_views WHERE schemaname = 'it_export' ORDER BY 2;
SELECT 'seq', sequencename, start_value::text, increment_by::text, last_value::text
  FROM pg_sequences WHERE schemaname = 'it_export' ORDER BY 2;
SELECT 'owned', pg_get_serial_sequence('it_export.customers', 'id');
SELECT 'rows', md5(string_agg(c::text, '|' ORDER BY id)) FROM it_export.customers c;
SELECT 'rows', md5(string_agg(o::text, '|' ORDER BY id)) FROM it_export.orders o;
";

#[tokio::test]
async fn postgres_round_trip() {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    if detect(Tool::Psql, &ToolSearch::system(None)).is_none() {
        return;
    }
    let (mut config, secrets) = parse_url(&url).unwrap();
    config.ssl = SslMode::Disable;
    let conn = savoia_pg::PgDriver
        .connect(&config.endpoint(), &secrets)
        .await
        .unwrap();

    fetch(&*conn, PG_SEED).await;
    let before = fetch(&*conn, PG_FINGERPRINT).await;

    let sql = temp("pg.sql");
    let mut request = ExportRequest {
        schema: "it_export".into(),
        tables: Vec::new(),
        ddl: true,
        data: true,
        drop_existing: true,
        format: OutputFormat::Sql,
        destination: sql.clone(),
    };
    let mut events = Vec::new();
    let summary = export(&*conn, Engine::Postgres, &request, |e| events.push(e))
        .await
        .unwrap();
    assert_eq!(summary.tables, 2);
    assert_eq!(summary.rows, 670);
    assert!(events.contains(&ExportEvent::Table("customers".into())));
    assert!(events.contains(&ExportEvent::Rows(420)));

    // The gzip output decompresses to the same dump.
    request.format = OutputFormat::SqlGz;
    request.destination = temp("pg.sql.gz");
    export(&*conn, Engine::Postgres, &request, |_| {})
        .await
        .unwrap();
    let mut unzipped = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(&request.destination).unwrap())
        .read_to_string(&mut unzipped)
        .unwrap();
    assert_eq!(unzipped, std::fs::read_to_string(&sql).unwrap());

    // Restore over the original (the dump drops it first) and compare.
    restore(Tool::Psql, "SAVOIA_PG_URL", &sql).await;
    let after = fetch(&*conn, PG_FINGERPRINT).await;
    assert_eq!(before, after);

    // CSV: one file per table, NULL and "" kept apart.
    request.format = OutputFormat::Csv;
    request.destination = temp("pg-csv");
    request.tables = vec!["customers".into(), "big".into()];
    export(&*conn, Engine::Postgres, &request, |_| {})
        .await
        .unwrap();
    let customers = std::fs::read_to_string(request.destination.join("customers.csv")).unwrap();
    assert_eq!(customers.lines().next(), Some("id,name,mood,note,data"));
    assert!(customers.contains(",\"\","), "an empty note is quoted");
    assert!(request.destination.join("big.csv").exists());
    assert!(!request.destination.join("orders.csv").exists());
}

const MYSQL_SEED: &str = "
DROP VIEW IF EXISTS it_export_bigger, it_export_big;
DROP TABLE IF EXISTS it_export_orders, it_export_customers;
CREATE TABLE it_export_customers (
  id int AUTO_INCREMENT PRIMARY KEY,
  name varchar(80) NOT NULL UNIQUE,
  note text,
  data blob,
  created datetime DEFAULT CURRENT_TIMESTAMP,
  CHECK (name <> ''));
CREATE TABLE it_export_orders (
  id int AUTO_INCREMENT PRIMARY KEY,
  customer_id int NOT NULL,
  total decimal(10,2) DEFAULT 0,
  doubled decimal(12,2) GENERATED ALWAYS AS (total * 2) STORED,
  KEY total_idx (total),
  CONSTRAINT it_export_orders_fk FOREIGN KEY (customer_id) REFERENCES it_export_customers (id));
CREATE VIEW it_export_bigger AS SELECT 1 AS one;
CREATE OR REPLACE VIEW it_export_big AS SELECT * FROM it_export_orders WHERE total > 10;
CREATE OR REPLACE VIEW it_export_bigger AS SELECT * FROM it_export_big WHERE total > 100;
INSERT INTO it_export_customers (name, note, data, created) VALUES
  ('plain', NULL, NULL, '2026-01-02 03:04:05'),
  ('empty', '', 0x00FF10, '2026-01-02 03:04:05'),
  ('O''Brien \\\\ \"q\"', 'tab\\there\\nnewline', X'', '2026-01-02 03:04:05');
INSERT INTO it_export_orders (customer_id, total) VALUES (1, 5), (2, 50), (3, 500), (1, 12.5);
";

const MYSQL_FINGERPRINT: &str = "
SHOW CREATE TABLE it_export_customers;
SHOW CREATE TABLE it_export_orders;
SHOW CREATE VIEW it_export_big;
SHOW CREATE VIEW it_export_bigger;
CHECKSUM TABLE it_export_customers, it_export_orders;
SELECT id, name, note, hex(data), created FROM it_export_customers ORDER BY id;
";

#[tokio::test]
async fn mysql_round_trip() {
    let Ok(url) = std::env::var("SAVOIA_MYSQL_URL") else {
        return;
    };
    if detect(Tool::Mysql, &ToolSearch::system(None)).is_none() {
        return;
    }
    let (mut config, secrets) = parse_url(&url).unwrap();
    config.ssl = SslMode::Disable;
    let database = config.database.clone().unwrap();
    let conn = savoia_mysql::MysqlDriver
        .connect(&config.endpoint(), &secrets)
        .await
        .unwrap();

    fetch(&*conn, MYSQL_SEED).await;
    let before = fetch(&*conn, MYSQL_FINGERPRINT).await;

    let sql = temp("my.sql");
    let request = ExportRequest {
        schema: database,
        tables: [
            "it_export_customers",
            "it_export_orders",
            "it_export_big",
            "it_export_bigger",
        ]
        .map(String::from)
        .to_vec(),
        ddl: true,
        data: true,
        drop_existing: true,
        format: OutputFormat::Sql,
        destination: sql.clone(),
    };
    let summary = export(&*conn, Engine::Mysql, &request, |_| {})
        .await
        .unwrap();
    assert_eq!((summary.tables, summary.rows), (2, 7));
    let dump = std::fs::read_to_string(&sql).unwrap();
    assert!(!dump.contains("DEFINER="), "definers are stripped");
    assert!(
        dump.find("VIEW `it_export_big`").unwrap() < dump.find("VIEW `it_export_bigger`").unwrap(),
        "views are ordered by dependency"
    );

    restore(Tool::Mysql, "SAVOIA_MYSQL_URL", &sql).await;
    let after = fetch(&*conn, MYSQL_FINGERPRINT).await;
    assert_eq!(before, after);
}
