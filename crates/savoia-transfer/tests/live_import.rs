//! Built-in import against real servers. Skipped unless `SAVOIA_PG_URL` /
//! `SAVOIA_MYSQL_URL` is set; the dump-tool cases also need the tools.

use std::path::PathBuf;

use savoia_core::{Connection, Driver, Engine, QueryEvent, SslMode, parse_url};
use savoia_transfer::builtin::{ExportRequest, OutputFormat, export};
use savoia_transfer::import::{
    CsvImport, CsvOptions, ImportOptions, OnError, SqlImport, auto_map, import_csv, import_sql,
    preview_csv,
};
use savoia_transfer::runner::{RunEnd, ToolCommand};
use savoia_transfer::tools::{Tool, ToolSearch, detect};

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("savoia-import-{}-{name}", uuid::Uuid::new_v4()))
}

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

async fn connect(
    var: &str,
) -> Option<(
    Box<dyn Connection>,
    savoia_core::ConnectionConfig,
    savoia_core::Secrets,
)> {
    let url = std::env::var(var).ok()?;
    let (mut config, secrets) = parse_url(&url).unwrap();
    config.ssl = SslMode::Disable;
    let conn = match config.engine {
        Engine::Postgres => {
            savoia_pg::PgDriver
                .connect(&config.endpoint(), &secrets)
                .await
        }
        Engine::Mysql => {
            savoia_mysql::MysqlDriver
                .connect(&config.endpoint(), &secrets)
                .await
        }
    }
    .unwrap();
    Some((conn, config, secrets))
}

/// A script bigger than one read chunk, with multibyte text so chunk edges
/// fall inside characters, statements and strings.
fn big_inserts(table: &str, rows: usize) -> String {
    let filler = "ação ☕ ".repeat(60);
    (0..rows)
        .map(|i| format!("INSERT INTO {table} (id, note) VALUES ({i}, '{filler}{i}');\n"))
        .collect()
}

const STOP: ImportOptions = ImportOptions {
    on_error: OnError::Stop,
    single_transaction: false,
};
const CONTINUE: ImportOptions = ImportOptions {
    on_error: OnError::Continue,
    single_transaction: false,
};

#[tokio::test]
async fn postgres_scripts() {
    let Some((conn, _, _)) = connect("SAVOIA_PG_URL").await else {
        return;
    };
    let script = format!(
        "-- header comment; with a semicolon\n\
         DROP SCHEMA IF EXISTS it_import CASCADE;\n\
         CREATE SCHEMA it_import;\n\
         CREATE TABLE it_import.notes (id int PRIMARY KEY, note text);\n\
         CREATE FUNCTION it_import.f() RETURNS int LANGUAGE plpgsql AS $body$\n\
         BEGIN\n  RETURN 1; -- a ; inside\nEND\n$body$;\n\
         /* block /* nested; */ comment */\n\
         {}\
         INSERT INTO it_import.notes VALUES (1, E'duplicate \\' key');\n\
         {}\
         SELECT it_import.f();\n",
        big_inserts("it_import.notes", 3000),
        big_inserts("it_import.notes", 0)
    );
    let path = temp("pg.sql");
    std::fs::write(&path, &script).unwrap();
    assert!(
        script.len() > 2 << 20,
        "the script spans several chunks: {}",
        script.len()
    );

    let mut errors = Vec::new();
    let summary = import_sql(
        &*conn,
        Engine::Postgres,
        &SqlImport {
            path,
            options: CONTINUE,
        },
        |e| {
            if let savoia_transfer::import::ImportEvent::Error(error) = e {
                errors.push(error);
            }
        },
    )
    .await
    .unwrap();
    assert_eq!(summary.failed, 1, "{:?}", summary.errors);
    assert_eq!(summary.done, 4 + 3000 + 1);
    let error = &summary.errors[0];
    assert_eq!(error.line, 11 + 3000, "{error:?}");
    assert!(error.message.contains("duplicate key"), "{error:?}");
    assert_eq!(errors.len(), 1);
    assert_eq!(
        fetch(&*conn, "SELECT count(*) FROM it_import.notes").await,
        ["3000"]
    );
    assert_eq!(
        fetch(&*conn, "SELECT note FROM it_import.notes WHERE id = 1499").await[0],
        format!("{}1499", "ação ☕ ".repeat(60))
    );

    // One transaction: the failure rolls everything back.
    let path = temp("pg-tx.sql");
    std::fs::write(
        &path,
        "CREATE TABLE it_import.tx (id int PRIMARY KEY);\nINSERT INTO it_import.tx VALUES (1);\nINSERT INTO it_import.tx VALUES (1);\n",
    )
    .unwrap();
    let options = ImportOptions {
        on_error: OnError::Continue,
        single_transaction: true,
    };
    let summary = import_sql(
        &*conn,
        Engine::Postgres,
        &SqlImport { path, options },
        |_| {},
    )
    .await
    .unwrap();
    assert!(summary.stopped && summary.rolled_back);
    assert_eq!(
        fetch(&*conn, "SELECT to_regclass('it_import.tx') IS NULL").await,
        ["t"]
    );
}

#[tokio::test]
async fn postgres_dumps_reimport() {
    let Some((conn, config, secrets)) = connect("SAVOIA_PG_URL").await else {
        return;
    };
    fetch(
        &*conn,
        "DROP SCHEMA IF EXISTS it_reimport CASCADE; CREATE SCHEMA it_reimport;
         CREATE TABLE it_reimport.t (id serial PRIMARY KEY, note text, data bytea);
         INSERT INTO it_reimport.t (note, data)
           SELECT CASE WHEN i % 3 = 0 THEN NULL ELSE 'n''' || i END, decode(lpad(to_hex(i), 4, '0'), 'hex')
           FROM generate_series(1, 300) i;",
    )
    .await;
    let checksum = "SELECT md5(string_agg(t::text, '|' ORDER BY id)) FROM it_reimport.t t";
    let before = fetch(&*conn, checksum).await;

    // Our own gzip dump, replayed by our own importer.
    let dump = temp("own.sql.gz");
    let request = ExportRequest {
        schema: "it_reimport".into(),
        tables: Vec::new(),
        ddl: true,
        data: true,
        drop_existing: true,
        format: OutputFormat::SqlGz,
        destination: dump.clone(),
    };
    export(&*conn, Engine::Postgres, &request, |_| {})
        .await
        .unwrap();
    let summary = import_sql(
        &*conn,
        Engine::Postgres,
        &SqlImport {
            path: dump,
            options: STOP,
        },
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(summary.failed, 0, "{:?}", summary.errors);
    assert_eq!(fetch(&*conn, checksum).await, before);

    // CSV back into an emptied table, with one bad row.
    let csv_dir = temp("csv");
    let request = ExportRequest {
        format: OutputFormat::Csv,
        destination: csv_dir.clone(),
        ..request
    };
    export(&*conn, Engine::Postgres, &request, |_| {})
        .await
        .unwrap();
    let csv = csv_dir.join("t.csv");
    let mut text = std::fs::read_to_string(&csv).unwrap();
    text.push_str("not-a-number,x,\n");
    std::fs::write(&csv, text).unwrap();
    fetch(&*conn, "TRUNCATE it_reimport.t").await;
    let preview = preview_csv(&csv, CsvOptions::default(), 5).unwrap();
    let mapping = auto_map(
        &preview.headers,
        &[column("id"), column("note"), column("data")],
    );
    assert_eq!(mapping.len(), 3);
    let summary = import_csv(
        &*conn,
        Engine::Postgres,
        &CsvImport {
            path: csv,
            csv: CsvOptions::default(),
            schema: "it_reimport".into(),
            table: "t".into(),
            mapping,
            options: CONTINUE,
        },
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(
        (summary.done, summary.failed),
        (300, 1),
        "{:?}",
        summary.errors
    );
    assert_eq!(summary.errors[0].line, 302);
    assert_eq!(fetch(&*conn, checksum).await, before);

    // pg_dump --inserts output, including pg_dump 17.6+'s \restrict lines.
    let Some(pg_dump) = detect(Tool::PgDump, &ToolSearch::system(None)) else {
        return;
    };
    let dump = temp("pg_dump.sql");
    let mut command =
        ToolCommand::connect(&pg_dump, &config.endpoint(), secrets.password.as_deref()).unwrap();
    command
        .args([
            "--inserts",
            "--clean",
            "--if-exists",
            "--schema=it_reimport",
            "--file",
        ])
        .arg(&dump);
    assert_eq!(command.spawn().unwrap().wait().await.0, RunEnd::Succeeded);
    let summary = import_sql(
        &*conn,
        Engine::Postgres,
        &SqlImport {
            path: dump,
            options: STOP,
        },
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(summary.failed, 0, "{:?}", summary.errors);
    assert_eq!(fetch(&*conn, checksum).await, before);

    // Default pg_dump output uses COPY, which needs psql.
    let dump = temp("pg_dump_copy.sql");
    let mut command =
        ToolCommand::connect(&pg_dump, &config.endpoint(), secrets.password.as_deref()).unwrap();
    command
        .args(["--clean", "--if-exists", "--schema=it_reimport", "--file"])
        .arg(&dump);
    assert_eq!(command.spawn().unwrap().wait().await.0, RunEnd::Succeeded);
    let err = import_sql(
        &*conn,
        Engine::Postgres,
        &SqlImport {
            path: dump,
            options: STOP,
        },
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("COPY"), "{err}");
    // Found before anything ran: the table is still there and full.
    assert_eq!(fetch(&*conn, checksum).await, before);
}

fn column(name: &str) -> savoia_core::ColumnInfo {
    savoia_core::ColumnInfo {
        name: name.into(),
        data_type: String::new(),
        nullable: true,
        default: None,
    }
}

#[tokio::test]
async fn mysql_scripts_and_dumps() {
    let Some((conn, config, secrets)) = connect("SAVOIA_MYSQL_URL").await else {
        return;
    };
    let script = format!(
        "# hash comment\n\
         DROP TABLE IF EXISTS it_import_notes;\n\
         DROP PROCEDURE IF EXISTS it_import_p;\n\
         CREATE TABLE it_import_notes (id int PRIMARY KEY, note text, n int DEFAULT 0);\n\
         DELIMITER $$\n\
         CREATE PROCEDURE it_import_p() BEGIN\n  UPDATE it_import_notes SET n = 1; SELECT 1;\nEND $$\n\
         DELIMITER ;\n\
         {}\
         INSERT INTO it_import_notes (id, note) VALUES (1, 'dup \\\\ key');\n\
         CALL it_import_p();\n",
        big_inserts("it_import_notes", 1500)
    );
    let path = temp("my.sql");
    std::fs::write(&path, &script).unwrap();
    let summary = import_sql(
        &*conn,
        Engine::Mysql,
        &SqlImport {
            path,
            options: CONTINUE,
        },
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(summary.failed, 1, "{:?}", summary.errors);
    assert_eq!(summary.errors[0].line, 10 + 1500);
    assert_eq!(summary.done, 4 + 1500 + 1);
    assert_eq!(
        fetch(&*conn, "SELECT count(*), sum(n) FROM it_import_notes").await,
        ["1500 | 1500"]
    );

    // mysqldump output with a trigger (DELIMITER ;;) replays through our importer.
    let Some(mysqldump) = detect(Tool::Mysqldump, &ToolSearch::system(None)) else {
        return;
    };
    fetch(
        &*conn,
        "DROP TRIGGER IF EXISTS it_import_t; \
         CREATE TRIGGER it_import_t BEFORE INSERT ON it_import_notes FOR EACH ROW SET NEW.n = NEW.n + 10",
    )
    .await;
    let checksum = "CHECKSUM TABLE it_import_notes";
    let before = fetch(&*conn, checksum).await;
    let dump = temp("mysqldump.sql");
    let mut command =
        ToolCommand::connect(&mysqldump, &config.endpoint(), secrets.password.as_deref()).unwrap();
    command
        .args(["--no-tablespaces", "--skip-dump-date"])
        .arg(config.database.clone().unwrap())
        .arg("it_import_notes")
        .stdout_file(&dump);
    assert_eq!(command.spawn().unwrap().wait().await.0, RunEnd::Succeeded);
    assert!(
        std::fs::read_to_string(&dump)
            .unwrap()
            .contains("DELIMITER ;;")
    );
    let summary = import_sql(
        &*conn,
        Engine::Mysql,
        &SqlImport {
            path: dump,
            options: STOP,
        },
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(summary.failed, 0, "{:?}", summary.errors);
    assert_eq!(fetch(&*conn, checksum).await, before);
    assert_eq!(
        fetch(
            &*conn,
            "SELECT count(*) FROM information_schema.TRIGGERS WHERE TRIGGER_NAME = 'it_import_t'"
        )
        .await,
        ["1"]
    );
}
