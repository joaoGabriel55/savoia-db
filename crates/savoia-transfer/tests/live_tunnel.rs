//! Dumps and restores through an SSH tunnel (backlog 4.6), via the
//! docker-compose `ssh` bastion: the external tools and the built-in engine
//! reach the servers only through the tunnel's local end. Skipped unless
//! `SAVOIA_SSH_TEST=1`.

use std::path::PathBuf;
use std::sync::Arc;

use savoia_core::{
    Connection, ConnectionConfig, Driver, Endpoint, Engine, QueryEvent, Secrets, SshAuth,
    SshConfig, SslMode,
};
use savoia_transfer::import::{ImportOptions, OnError};
use savoia_transfer::job::{
    self, Content, DumpFormat, DumpPlan, JobEnd, RestorePlan, RestoreSource, Runner,
};
use savoia_transfer::tools::{Tool, ToolSearch, detect};
use savoia_tunnel::{HostKeyPolicy, KnownHosts, Tunnel, open};

fn enabled() -> bool {
    std::env::var("SAVOIA_SSH_TEST").is_ok_and(|v| v == "1")
}

fn secrets() -> Secrets {
    Secrets {
        password: Some("savoia".into()),
        ssh_password: Some("savoia".into()),
        ..Default::default()
    }
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("savoia-tunnel-{}-{name}", uuid::Uuid::new_v4()))
}

/// A tunnel to `host:port` as the bastion sees it, and the endpoint through it.
async fn through_bastion(engine: Engine, host: &str, port: u16) -> (Tunnel, Endpoint) {
    let mut config = ConnectionConfig::new(engine);
    config.host = host.into();
    config.port = port;
    config.user = "savoia".into();
    config.database = Some("savoia".into());
    // TLS inside the tunnel, without verifying the containers' self-signed
    // certs. Not for MySQL: TLS rejects `mysql-8.4` as a server name (its last
    // label is numeric).
    config.ssl = match engine {
        Engine::Postgres => SslMode::Require,
        Engine::Mysql => SslMode::Disable,
    };
    let ssh = SshConfig {
        host: "127.0.0.1".into(),
        port: 2222,
        user: "savoia".into(),
        auth: SshAuth::Password,
    };
    config.ssh = Some(ssh.clone());
    let known_hosts = KnownHosts::File(temp("known_hosts"));
    let tunnel = open(
        &ssh,
        &secrets(),
        (host.into(), port),
        HostKeyPolicy::TrustUnknown,
        known_hosts,
    )
    .await
    .expect("tunnel");
    let endpoint = config.endpoint_via("127.0.0.1", tunnel.local_port());
    (tunnel, endpoint)
}

async fn fetch(conn: &dyn Connection, sql: &str) -> Vec<String> {
    let mut handle = conn.execute(sql.to_owned()).await.unwrap();
    let mut out = Vec::new();
    while let Some(event) = handle.next().await {
        if let QueryEvent::Rows(rows) = event.unwrap() {
            for row in rows {
                out.push(
                    row.iter()
                        .map(|c| c.as_deref().unwrap_or("NULL"))
                        .collect::<Vec<_>>()
                        .join(" | "),
                );
            }
        }
    }
    out
}

fn tool_runner(tool: Tool, endpoint: &Endpoint) -> Option<Runner> {
    Some(Runner::Tool {
        tool: detect(tool, &ToolSearch::system(None))?,
        endpoint: endpoint.clone(),
        password: secrets().password,
    })
}

fn dump_plan(engine: Engine, schema: &str, format: DumpFormat, tables: Vec<String>) -> DumpPlan {
    let extension = match format {
        DumpFormat::PgArchive => "dump",
        _ => "sql",
    };
    DumpPlan {
        engine,
        database: "savoia".into(),
        schema: schema.into(),
        table_count: tables.len(),
        tables,
        content: Content::SchemaAndData,
        drop_existing: true,
        skip_owners: true,
        format,
        destination: temp(&format!("dump.{extension}")),
    }
}

fn restore_plan(engine: Engine, path: PathBuf, source: RestoreSource) -> RestorePlan {
    RestorePlan {
        engine,
        database: "savoia".into(),
        schema: "savoia".into(),
        path,
        source,
        options: ImportOptions {
            on_error: OnError::Stop,
            single_transaction: false,
        },
        drop_existing: true,
        skip_owners: true,
        server_major: None,
    }
}

async fn expect_success(job: job::Job, what: &str) {
    let (end, log) = job.wait().await;
    assert!(
        matches!(end, JobEnd::Succeeded(_)),
        "{what}: {end:?}\n{}",
        log.join("\n")
    );
}

#[tokio::test]
async fn postgres_dump_and_restore_through_a_tunnel() {
    if !enabled() {
        return;
    }
    let (_tunnel, endpoint) = through_bastion(Engine::Postgres, "postgres-17", 5432).await;
    let conn: Arc<dyn Connection> = Arc::from(
        savoia_pg::PgDriver
            .connect(&endpoint, &secrets())
            .await
            .expect("connect through the tunnel"),
    );
    fetch(
        &*conn,
        "DROP SCHEMA IF EXISTS it_tunnel CASCADE; CREATE SCHEMA it_tunnel;
         CREATE TABLE it_tunnel.t (id serial PRIMARY KEY, note text);
         INSERT INTO it_tunnel.t (note) SELECT 'row ' || i FROM generate_series(1, 500) i;",
    )
    .await;
    let checksum = "SELECT count(*), md5(string_agg(t::text, '|' ORDER BY id)) FROM it_tunnel.t t";
    let before = fetch(&*conn, checksum).await;

    // pg_dump → pg_restore, both through the tunnel with TLS inside it.
    if let (Some(dump), Some(restore)) = (
        tool_runner(Tool::PgDump, &endpoint),
        tool_runner(Tool::PgRestore, &endpoint),
    ) {
        let plan = dump_plan(
            Engine::Postgres,
            "it_tunnel",
            DumpFormat::PgArchive,
            Vec::new(),
        );
        let archive = plan.destination.clone();
        expect_success(job::dump(plan, dump), "pg_dump").await;
        expect_success(
            job::restore(
                restore_plan(Engine::Postgres, archive, RestoreSource::PgArchive),
                restore,
            ),
            "pg_restore",
        )
        .await;
        assert_eq!(fetch(&*conn, checksum).await, before);
    }

    // The built-in engine both ways, on a connection through the tunnel.
    let plan = dump_plan(Engine::Postgres, "it_tunnel", DumpFormat::Sql, Vec::new());
    let script = plan.destination.clone();
    expect_success(
        job::dump(plan, Runner::BuiltIn(conn.clone())),
        "built-in export",
    )
    .await;
    expect_success(
        job::restore(
            restore_plan(Engine::Postgres, script, RestoreSource::Sql),
            Runner::BuiltIn(conn.clone()),
        ),
        "built-in import",
    )
    .await;
    assert_eq!(fetch(&*conn, checksum).await, before);
}

#[tokio::test]
async fn mysql_dump_and_restore_through_a_tunnel() {
    if !enabled() {
        return;
    }
    let (_tunnel, endpoint) = through_bastion(Engine::Mysql, "mysql-8.4", 3306).await;
    let conn: Arc<dyn Connection> = Arc::from(
        savoia_mysql::MysqlDriver
            .connect(&endpoint, &secrets())
            .await
            .expect("connect through the tunnel"),
    );
    fetch(
        &*conn,
        "DROP TABLE IF EXISTS it_tunnel_t;
         CREATE TABLE it_tunnel_t (id int AUTO_INCREMENT PRIMARY KEY, note text);
         INSERT INTO it_tunnel_t (note) VALUES ('a'), ('b'), (NULL), ('');",
    )
    .await;
    let checksum = "CHECKSUM TABLE it_tunnel_t";
    let before = fetch(&*conn, checksum).await;
    let tables = vec!["it_tunnel_t".to_owned()];

    if let (Some(dump), Some(restore)) = (
        tool_runner(Tool::Mysqldump, &endpoint),
        tool_runner(Tool::Mysql, &endpoint),
    ) {
        let plan = dump_plan(Engine::Mysql, "savoia", DumpFormat::Sql, tables.clone());
        let script = plan.destination.clone();
        expect_success(job::dump(plan, dump), "mysqldump").await;
        expect_success(
            job::restore(
                restore_plan(Engine::Mysql, script, RestoreSource::Sql),
                restore,
            ),
            "mysql",
        )
        .await;
        assert_eq!(fetch(&*conn, checksum).await, before);
    }

    let plan = dump_plan(Engine::Mysql, "savoia", DumpFormat::SqlGz, tables);
    let script = plan.destination.with_extension("sql.gz");
    let plan = DumpPlan {
        destination: script.clone(),
        ..plan
    };
    expect_success(
        job::dump(plan, Runner::BuiltIn(conn.clone())),
        "built-in export",
    )
    .await;
    expect_success(
        job::restore(
            restore_plan(Engine::Mysql, script, RestoreSource::Sql),
            Runner::BuiltIn(conn.clone()),
        ),
        "built-in import",
    )
    .await;
    assert_eq!(fetch(&*conn, checksum).await, before);
}
