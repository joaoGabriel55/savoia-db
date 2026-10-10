//! Dump → restore round trips (backlog 4.7): each way of dumping and
//! restoring, external tools and built-in engine alike, must give back the
//! same schema and rows. Every method seeds a schema, fingerprints it, dumps
//! it, drops it, restores it and fingerprints it again.
//!
//! Runs against each server in `SAVOIA_ROUND_TRIP_URLS` (comma-separated,
//! e.g. every version from `docker compose --profile all up -d`), else
//! `SAVOIA_PG_URL` and `SAVOIA_MYSQL_URL`. Methods needing a tool that isn't
//! installed are skipped.

use std::path::PathBuf;
use std::sync::Arc;

use savoia_core::{
    Connection, ConnectionConfig, Driver, Engine, QueryEvent, Secrets, SslMode, parse_url,
};
use savoia_transfer::import::{ImportOptions, OnError};
use savoia_transfer::job::{
    self, Content, DumpFormat, DumpPlan, JobEnd, RestorePlan, RestoreSource, Runner,
};
use savoia_transfer::tools::{
    Compatibility, Tool, ToolSearch, check, detect, parse_server_version,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    Tool(Tool),
    BuiltIn,
}

/// One way of dumping and one of restoring.
struct Method {
    name: &'static str,
    dump: Way,
    format: DumpFormat,
    restore: Way,
}

const PG_METHODS: &[Method] = &[
    Method {
        name: "pg_dump → psql",
        dump: Way::Tool(Tool::PgDump),
        format: DumpFormat::Sql,
        restore: Way::Tool(Tool::Psql),
    },
    Method {
        name: "pg_dump archive → pg_restore",
        dump: Way::Tool(Tool::PgDump),
        format: DumpFormat::PgArchive,
        restore: Way::Tool(Tool::PgRestore),
    },
    Method {
        name: "built-in → built-in",
        dump: Way::BuiltIn,
        format: DumpFormat::Sql,
        restore: Way::BuiltIn,
    },
    Method {
        name: "built-in .gz → built-in",
        dump: Way::BuiltIn,
        format: DumpFormat::SqlGz,
        restore: Way::BuiltIn,
    },
    Method {
        name: "built-in → psql",
        dump: Way::BuiltIn,
        format: DumpFormat::Sql,
        restore: Way::Tool(Tool::Psql),
    },
];

const MYSQL_METHODS: &[Method] = &[
    Method {
        name: "mysqldump → mysql",
        dump: Way::Tool(Tool::Mysqldump),
        format: DumpFormat::Sql,
        restore: Way::Tool(Tool::Mysql),
    },
    Method {
        name: "mysqldump → built-in",
        dump: Way::Tool(Tool::Mysqldump),
        format: DumpFormat::Sql,
        restore: Way::BuiltIn,
    },
    Method {
        name: "built-in → built-in",
        dump: Way::BuiltIn,
        format: DumpFormat::Sql,
        restore: Way::BuiltIn,
    },
    Method {
        name: "built-in .gz → built-in",
        dump: Way::BuiltIn,
        format: DumpFormat::SqlGz,
        restore: Way::BuiltIn,
    },
    Method {
        name: "built-in → mysql",
        dump: Way::BuiltIn,
        format: DumpFormat::Sql,
        restore: Way::Tool(Tool::Mysql),
    },
];

fn urls() -> Vec<String> {
    match std::env::var("SAVOIA_ROUND_TRIP_URLS") {
        Ok(list) => list
            .split(',')
            .map(|u| u.trim().to_owned())
            .filter(|u| !u.is_empty())
            .collect(),
        Err(_) => ["SAVOIA_PG_URL", "SAVOIA_MYSQL_URL"]
            .iter()
            .filter_map(|var| std::env::var(var).ok())
            .collect(),
    }
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("savoia-rt-{}-{name}", uuid::Uuid::new_v4()))
}

/// Every row of every result set, one string per row.
async fn fetch(conn: &dyn Connection, sql: &str) -> Vec<String> {
    let mut handle = conn.execute(sql.to_owned()).await.unwrap();
    let mut out = Vec::new();
    while let Some(event) = handle.next().await {
        match event {
            Ok(QueryEvent::Rows(rows)) => {
                for row in rows {
                    out.push(
                        row.iter()
                            .map(|c| c.as_deref().unwrap_or("NULL"))
                            .collect::<Vec<_>>()
                            .join(" | "),
                    );
                }
            }
            Ok(_) => {}
            Err(err) => panic!("{err}\n{sql}"),
        }
    }
    out
}

const PG_SEED: &str = r#"
DROP SCHEMA IF EXISTS it_rt CASCADE;
CREATE SCHEMA it_rt;
CREATE TYPE it_rt.status AS ENUM ('new', 'paid', 'shipped');
CREATE SEQUENCE it_rt.invoice_no START 1000 INCREMENT 10;
SELECT nextval('it_rt.invoice_no');
CREATE TABLE it_rt.customers (
  id serial PRIMARY KEY,
  email text NOT NULL UNIQUE CHECK (position('@' in email) > 1),
  name text,
  created timestamptz NOT NULL DEFAULT now(),
  tags text[],
  profile jsonb,
  avatar bytea);
CREATE TABLE it_rt.orders (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  customer_id int NOT NULL REFERENCES it_rt.customers (id) ON DELETE CASCADE,
  status it_rt.status NOT NULL DEFAULT 'new',
  total numeric(12,2) NOT NULL,
  tax numeric GENERATED ALWAYS AS (total * 0.2) STORED,
  invoice int DEFAULT nextval('it_rt.invoice_no'),
  note text);
CREATE INDEX orders_open ON it_rt.orders (status) WHERE status <> 'shipped';
CREATE UNIQUE INDEX customers_lower_email ON it_rt.customers (lower(email));
CREATE VIEW it_rt.open_orders AS
  SELECT o.id, c.email, o.total FROM it_rt.orders o JOIN it_rt.customers c ON c.id = o.customer_id
  WHERE o.status <> 'shipped';
CREATE FUNCTION it_rt.touch() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN NEW.note := coalesce(NEW.note, 'n/a'); RETURN NEW; END $$;
CREATE TRIGGER orders_touch BEFORE INSERT ON it_rt.orders FOR EACH ROW EXECUTE FUNCTION it_rt.touch();
INSERT INTO it_rt.customers (email, name, created, tags, profile, avatar)
SELECT 'c' || i || '@example.com',
       CASE i % 5 WHEN 0 THEN NULL WHEN 1 THEN '' WHEN 2 THEN 'O''Brien "q" \ ' || i ELSE 'Zoë ☕ ' || i END,
       timestamptz '2026-01-01 00:00:00+00' + i * interval '1 hour',
       CASE WHEN i % 4 = 0 THEN NULL ELSE ARRAY['a' || i, NULL, 'b,c'] END,
       CASE WHEN i % 3 = 0 THEN NULL ELSE jsonb_build_object('n', i, 's', 'x"y', 'l', jsonb_build_array(1, 2)) END,
       CASE WHEN i % 6 = 0 THEN NULL WHEN i % 6 = 1 THEN '\x'::bytea ELSE decode(md5(i::text), 'hex') END
FROM generate_series(1, 300) i;
INSERT INTO it_rt.orders (customer_id, status, total, note)
SELECT (i % 300) + 1, (ARRAY['new', 'paid', 'shipped'])[i % 3 + 1]::it_rt.status, i * 1.25,
       CASE WHEN i % 2 = 0 THEN NULL ELSE 'note ' || i END
FROM generate_series(1, 1000) i;
"#;

/// Structure and rows of `it_rt`. Routines and triggers only when the
/// method carries them (the built-in engine doesn't).
fn pg_fingerprint(with_routines: bool) -> String {
    let mut sql = String::from(
        "SELECT 'col', c.relname, a.attname, format_type(a.atttypid, a.atttypmod), a.attnotnull::text,
                coalesce(pg_get_expr(d.adbin, d.adrelid), ''), a.attidentity::text, a.attgenerated::text
           FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid
           LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
          WHERE c.relnamespace = 'it_rt'::regnamespace AND c.relkind IN ('r', 'v')
            AND a.attnum > 0 AND NOT a.attisdropped
          ORDER BY 2, 3;
         SELECT 'con', conrelid::regclass::text, conname, pg_get_constraintdef(oid)
           FROM pg_constraint WHERE connamespace = 'it_rt'::regnamespace AND contype <> 'n' ORDER BY 2, 3;
         SELECT 'idx', indexname, indexdef FROM pg_indexes WHERE schemaname = 'it_rt' ORDER BY 2;
         SELECT 'view', viewname, definition FROM pg_views WHERE schemaname = 'it_rt' ORDER BY 2;
         SELECT 'seq', sequencename, data_type::text, start_value::text, increment_by::text, coalesce(last_value::text, '')
           FROM pg_sequences WHERE schemaname = 'it_rt' ORDER BY 2;
         SELECT 'enum', t.typname, string_agg(e.enumlabel, ',' ORDER BY e.enumsortorder)
           FROM pg_type t JOIN pg_enum e ON e.enumtypid = t.oid
          WHERE t.typnamespace = 'it_rt'::regnamespace GROUP BY t.typname;
         SELECT 'rows', 'customers', count(*)::text, md5(string_agg(t::text, '|' ORDER BY t.id)) FROM it_rt.customers t;
         SELECT 'rows', 'orders', count(*)::text, md5(string_agg(t::text, '|' ORDER BY t.id)) FROM it_rt.orders t;",
    );
    if with_routines {
        sql.push_str(
            "SELECT 'fn', p.proname, pg_get_functiondef(p.oid) FROM pg_proc p
              WHERE p.pronamespace = 'it_rt'::regnamespace ORDER BY 2;
             SELECT 'trigger', tgname, pg_get_triggerdef(oid) FROM pg_trigger
              WHERE tgrelid = 'it_rt.orders'::regclass AND NOT tgisinternal ORDER BY 2;",
        );
    }
    sql
}

const MYSQL_SEED: &str = r#"
DROP VIEW IF EXISTS it_rt_open_orders;
DROP TABLE IF EXISTS it_rt_orders, it_rt_customers;
CREATE TABLE it_rt_customers (
  id int AUTO_INCREMENT PRIMARY KEY,
  email varchar(120) NOT NULL UNIQUE,
  name varchar(80),
  created datetime(3) DEFAULT CURRENT_TIMESTAMP(3),
  profile json,
  avatar blob,
  score double,
  flags set('a', 'b', 'c'),
  kind enum('x', 'y'),
  CHECK (email LIKE '%@%'));
CREATE TABLE it_rt_orders (
  id bigint AUTO_INCREMENT PRIMARY KEY,
  customer_id int NOT NULL,
  total decimal(12,2) NOT NULL,
  tax decimal(12,2) GENERATED ALWAYS AS (total * 0.2) STORED,
  note text,
  KEY total_idx (total),
  CONSTRAINT it_rt_orders_customer FOREIGN KEY (customer_id) REFERENCES it_rt_customers (id) ON DELETE CASCADE);
CREATE VIEW it_rt_open_orders AS
  SELECT o.id, c.email, o.total FROM it_rt_orders o JOIN it_rt_customers c ON c.id = o.customer_id WHERE o.total > 100;
INSERT INTO it_rt_customers (email, name, created, profile, avatar, score, flags, kind) VALUES
  ('a@example.com', NULL, '2026-01-01 00:00:00.123', NULL, NULL, NULL, NULL, NULL),
  ('b@example.com', '', '2026-01-02 03:04:05.000', '{}', X'', 0, '', 'x'),
  ('c@example.com', 'O''Brien \\ \"q\" ☕', '2026-01-03 23:59:59.999', '{\"n\": 1, \"s\": \"x\\\"y\", \"l\": [1, 2]}',
     X'00FF10', -1.5e-10, 'a,c', 'y');
INSERT INTO it_rt_orders (customer_id, total, note) VALUES
  (1, 5, NULL), (2, 150.5, 'big'), (3, 999.99, ''), (1, 120, 'tab	here');
"#;

fn mysql_fingerprint() -> &'static str {
    "SHOW CREATE TABLE it_rt_customers;
     SHOW CREATE TABLE it_rt_orders;
     SHOW CREATE VIEW it_rt_open_orders;
     CHECKSUM TABLE it_rt_customers, it_rt_orders;
     SELECT id, email, name, created, profile, hex(avatar), score, flags, kind FROM it_rt_customers ORDER BY id;"
}

const MYSQL_DROP: &str =
    "DROP VIEW IF EXISTS it_rt_open_orders; DROP TABLE IF EXISTS it_rt_orders, it_rt_customers;";
const MYSQL_TABLES: [&str; 3] = ["it_rt_customers", "it_rt_orders", "it_rt_open_orders"];

struct Server {
    config: ConnectionConfig,
    secrets: Secrets,
    conn: Arc<dyn Connection>,
}

async fn connect(url: &str) -> Server {
    let (mut config, secrets) = parse_url(url).expect("a valid URL");
    config.ssl = SslMode::Disable;
    let endpoint = config.endpoint();
    let conn = match config.engine {
        Engine::Postgres => savoia_pg::PgDriver.connect(&endpoint, &secrets).await,
        Engine::Mysql => savoia_mysql::MysqlDriver.connect(&endpoint, &secrets).await,
    }
    .unwrap_or_else(|err| panic!("{url}: {err}"));
    Server {
        config,
        secrets,
        conn: Arc::from(conn),
    }
}

impl Server {
    /// A runner for `way`, or `None` if the tool is missing or can't work
    /// with this server.
    async fn runner(&self, way: Way) -> Option<Runner> {
        match way {
            Way::BuiltIn => Some(Runner::BuiltIn(self.conn.clone())),
            Way::Tool(tool) => {
                let found = detect(tool, &ToolSearch::system(None))?;
                let server = parse_server_version(&self.conn.server_info().await.ok()?)?;
                if matches!(
                    check(tool, found.version, server),
                    Compatibility::Incompatible(_)
                ) {
                    return None;
                }
                Some(Runner::Tool {
                    tool: found,
                    endpoint: self.config.endpoint(),
                    password: self.secrets.password.clone(),
                })
            }
        }
    }
}

async fn round_trip(server: &Server, method: &Method) -> Option<()> {
    let engine = server.config.engine;
    let database = server
        .config
        .database
        .clone()
        .expect("the URL names a database");
    let (Some(dump_runner), Some(restore_runner)) = (
        server.runner(method.dump).await,
        server.runner(method.restore).await,
    ) else {
        eprintln!("skipped {}: tool not available", method.name);
        return None;
    };
    let (seed, schema, tables) = match engine {
        Engine::Postgres => (PG_SEED, "it_rt".to_owned(), Vec::new()),
        Engine::Mysql => (
            MYSQL_SEED,
            database.clone(),
            MYSQL_TABLES.map(String::from).to_vec(),
        ),
    };
    let with_routines = method.dump != Way::BuiltIn;
    let fingerprint = match engine {
        Engine::Postgres => pg_fingerprint(with_routines),
        Engine::Mysql => mysql_fingerprint().to_owned(),
    };
    let conn = &*server.conn;
    fetch(conn, seed).await;
    let before = fetch(conn, &fingerprint).await;
    assert!(
        before.len() >= 8,
        "the fingerprint sees the schema: {before:?}"
    );

    let extension = match method.format {
        DumpFormat::PgArchive => "dump",
        DumpFormat::SqlGz => "sql.gz",
        _ => "sql",
    };
    let plan = DumpPlan {
        engine,
        database: database.clone(),
        schema: schema.clone(),
        table_count: tables.len(),
        tables,
        content: Content::SchemaAndData,
        drop_existing: false,
        skip_owners: true,
        format: method.format,
        destination: temp(&format!("dump.{extension}")),
    };
    let path = plan.destination.clone();
    let (end, log) = job::dump(plan, dump_runner).wait().await;
    assert!(
        matches!(end, JobEnd::Succeeded(_)),
        "{}: dump: {end:?}\n{}",
        method.name,
        log.join("\n")
    );

    // Restore into an empty schema.
    match engine {
        Engine::Postgres => fetch(conn, "DROP SCHEMA it_rt CASCADE").await,
        Engine::Mysql => fetch(conn, MYSQL_DROP).await,
    };
    let source = match method.format {
        DumpFormat::PgArchive => RestoreSource::PgArchive,
        _ => RestoreSource::Sql,
    };
    let plan = RestorePlan {
        engine,
        database,
        schema,
        path,
        source,
        options: ImportOptions {
            on_error: OnError::Stop,
            single_transaction: false,
        },
        drop_existing: false,
        skip_owners: true,
        server_major: parse_server_version(&server.conn.server_info().await.unwrap())
            .map(|v| v.version.major),
    };
    let (end, log) = job::restore(plan, restore_runner).wait().await;
    assert!(
        matches!(end, JobEnd::Succeeded(_)),
        "{}: restore: {end:?}\n{}",
        method.name,
        log.join("\n")
    );

    let after = fetch(conn, &fingerprint).await;
    for (b, a) in before.iter().zip(&after) {
        assert_eq!(
            b, a,
            "{}: schema or rows differ after the round trip",
            method.name
        );
    }
    assert_eq!(
        before.len(),
        after.len(),
        "{}: objects missing after the round trip",
        method.name
    );
    Some(())
}

#[tokio::test]
async fn every_method_round_trips_on_every_server() {
    let urls = urls();
    if urls.is_empty() {
        return;
    }
    for url in urls {
        let server = connect(&url).await;
        let version = server.conn.server_info().await.unwrap().version;
        let methods = match server.config.engine {
            Engine::Postgres => PG_METHODS,
            Engine::Mysql => MYSQL_METHODS,
        };
        let mut ran = 0;
        for method in methods {
            if round_trip(&server, method).await.is_some() {
                ran += 1;
                eprintln!("ok: {version}: {}", method.name);
            }
        }
        assert!(ran >= 2, "{version}: at least the built-in methods ran");
    }
}
