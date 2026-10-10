//! Live tests running the real client tools against real servers. Skipped
//! unless the tool is installed and `SAVOIA_PG_URL` / `SAVOIA_MYSQL_URL` is
//! set, e.g. `SAVOIA_PG_URL=postgres://savoia:savoia@127.0.0.1:54317/savoia`
//! and `SAVOIA_MYSQL_URL=mysql://savoia:savoia@127.0.0.1:33084/savoia`
//! (docker compose).

use std::path::PathBuf;

use savoia_core::{ConnectionConfig, Secrets, SslMode, parse_url};
use savoia_transfer::runner::{RunEnd, RunEvent, ToolCommand};
use savoia_transfer::tools::{FoundTool, Tool, ToolSearch, detect};

fn target(var: &str, tool: Tool) -> Option<(ConnectionConfig, Secrets, FoundTool)> {
    let url = std::env::var(var).ok()?;
    let (mut config, secrets) = parse_url(&url).expect("a valid URL");
    config.ssl = SslMode::Disable;
    let found = detect(tool, &ToolSearch::system(None))?;
    Some((config, secrets, found))
}

fn out_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("savoia-live-{}-{name}", uuid::Uuid::new_v4()))
}

#[tokio::test]
async fn psql_seeds_and_pg_dump_reports_each_table() {
    let Some((config, secrets, psql)) = target("SAVOIA_PG_URL", Tool::Psql) else {
        return;
    };
    let found = detect(Tool::PgDump, &ToolSearch::system(None)).unwrap();
    let endpoint = config.endpoint();
    let password = secrets.password.as_deref();

    let mut seed = ToolCommand::connect(&psql, &endpoint, password).unwrap();
    seed.args([
        "--set=ON_ERROR_STOP=1",
        "--command=CREATE SCHEMA IF NOT EXISTS it_transfer; \
         CREATE TABLE IF NOT EXISTS it_transfer.items (id int PRIMARY KEY); \
         CREATE TABLE IF NOT EXISTS it_transfer.tags (id int PRIMARY KEY);",
    ]);
    let (end, log) = seed.spawn().unwrap().wait().await;
    assert_eq!(end, RunEnd::Succeeded, "{log:?}");

    let out = out_file("pg.sql");
    let mut command = ToolCommand::connect(&found, &endpoint, password).unwrap();
    command
        .args(["--verbose", "--schema=it_transfer", "--file"])
        .arg(&out);
    let mut run = command.spawn().unwrap();
    let (mut tables, mut end) = (Vec::new(), None);
    while let Some(event) = run.next().await {
        match event {
            RunEvent::Table(table) => tables.push(table),
            RunEvent::Finished(e) => end = Some(e),
            RunEvent::Log(_) => {}
        }
    }
    assert_eq!(end, Some(RunEnd::Succeeded));
    assert_eq!(tables, ["it_transfer.items", "it_transfer.tags"]);
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("CREATE TABLE it_transfer.items")
    );
}

#[tokio::test]
async fn pg_dump_reports_a_wrong_password() {
    let Some((config, _, found)) = target("SAVOIA_PG_URL", Tool::PgDump) else {
        return;
    };
    let command = ToolCommand::connect(&found, &config.endpoint(), Some("wrong")).unwrap();
    let (end, _) = command.spawn().unwrap().wait().await;
    let RunEnd::Failed(message) = end else {
        panic!("expected a failure, got {end:?}");
    };
    assert!(
        message.contains("password authentication failed"),
        "{message}"
    );
}

#[tokio::test]
async fn mysqldump_writes_a_dump_and_reports_tables() {
    let Some((config, secrets, found)) = target("SAVOIA_MYSQL_URL", Tool::Mysqldump) else {
        return;
    };
    let out = out_file("my.sql");
    let mut command =
        ToolCommand::connect(&found, &config.endpoint(), secrets.password.as_deref()).unwrap();
    command
        .args(["--verbose", "--no-tablespaces", "--no-data"])
        .arg(config.database.clone().unwrap())
        .stdout_file(&out);
    let mut run = command.spawn().unwrap();
    let (mut tables, mut end) = (0, None);
    while let Some(event) = run.next().await {
        match event {
            RunEvent::Table(_) => tables += 1,
            RunEvent::Finished(e) => end = Some(e),
            RunEvent::Log(_) => {}
        }
    }
    assert_eq!(end, Some(RunEnd::Succeeded));
    assert!(tables > 0, "expected per-table progress");
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("CREATE TABLE")
    );
}

#[tokio::test]
async fn mysql_runs_a_script_from_stdin() {
    let Some((config, secrets, found)) = target("SAVOIA_MYSQL_URL", Tool::Mysql) else {
        return;
    };
    let script = out_file("in.sql");
    std::fs::write(&script, "SELECT 40 + 2 AS answer;\n").unwrap();
    let mut command =
        ToolCommand::connect(&found, &config.endpoint(), secrets.password.as_deref()).unwrap();
    command.stdin_file(&script);
    let (end, log) = command.spawn().unwrap().wait().await;
    assert_eq!(end, RunEnd::Succeeded, "{log:?}");
    assert!(log.iter().any(|line| line == "42"), "{log:?}");
}
