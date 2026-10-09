//! PostgreSQL driver built on `tokio-postgres`. Must run inside a Tokio runtime.

mod catalog;
mod execute;
mod tls;

use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;
use savoia_core::{
    AppError, AppResult, Catalog, Connection, DatabaseNode, Driver, Endpoint, Engine, QueryHandle,
    SchemaObjects, Secrets, ServerInfo, SslMode, TableInfo,
};
use tokio::task::JoinHandle;
use tokio_postgres::{Client, Config, NoTls, config::SslMode as PgSslMode, error::SqlState};

/// What Postgres clients connect to when no database is given.
const DEFAULT_DATABASE: &str = "postgres";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub struct PgDriver;

#[async_trait]
impl Driver for PgDriver {
    fn engine(&self) -> Engine {
        Engine::Postgres
    }

    async fn connect(
        &self,
        endpoint: &Endpoint,
        secrets: &Secrets,
    ) -> AppResult<Box<dyn Connection>> {
        let config = pg_config(endpoint, secrets)?;
        let (client, task) = match endpoint.ssl {
            SslMode::Disable => {
                let (client, conn) = config.connect(NoTls).await.map_err(map_connect_error)?;
                (client, tokio::spawn(async move { drop(conn.await) }))
            }
            mode => {
                let tls = tls::connector(mode == SslMode::VerifyFull);
                let (client, conn) = config.connect(tls).await.map_err(map_connect_error)?;
                (client, tokio::spawn(async move { drop(conn.await) }))
            }
        };
        Ok(Box::new(PgConnection {
            client,
            database: endpoint
                .database
                .clone()
                .unwrap_or_else(|| DEFAULT_DATABASE.into()),
            ssl: endpoint.ssl,
            task,
        }))
    }
}

fn pg_config(endpoint: &Endpoint, secrets: &Secrets) -> AppResult<Config> {
    let mut config = Config::new();
    // `host` is what TLS verifies; behind a tunnel `hostaddr` is where TCP goes.
    config.host(&endpoint.tls_server_name);
    if endpoint.host != endpoint.tls_server_name {
        let addr: IpAddr = endpoint
            .host
            .parse()
            .map_err(|_| AppError::internal("tunnel endpoint must be an IP address"))?;
        config.hostaddr(addr);
    }
    config
        .port(endpoint.port)
        .user(&endpoint.user)
        .dbname(endpoint.database.as_deref().unwrap_or(DEFAULT_DATABASE))
        .application_name("Savoia DB")
        .connect_timeout(CONNECT_TIMEOUT)
        .ssl_mode(match endpoint.ssl {
            SslMode::Disable => PgSslMode::Disable,
            SslMode::Prefer => PgSslMode::Prefer,
            SslMode::Require | SslMode::VerifyFull => PgSslMode::Require,
        });
    if let Some(password) = &secrets.password {
        config.password(password);
    }
    if endpoint.read_only {
        config.options("-c default_transaction_read_only=on");
    }
    Ok(config)
}

fn map_connect_error(err: tokio_postgres::Error) -> AppError {
    match err.code() {
        Some(code)
            if *code == SqlState::INVALID_PASSWORD
                || *code == SqlState::INVALID_AUTHORIZATION_SPECIFICATION =>
        {
            AppError::auth(db_message(&err))
        }
        _ => AppError::connect(db_message(&err)),
    }
}

/// The server's message when there is one, else the client-side error chain.
fn db_message(err: &tokio_postgres::Error) -> String {
    match err.as_db_error() {
        Some(db) => db.message().to_owned(),
        None => {
            let mut msg = err.to_string();
            let mut source = std::error::Error::source(err);
            while let Some(s) = source {
                msg = format!("{msg}: {s}");
                source = s.source();
            }
            msg
        }
    }
}

fn query_error(err: tokio_postgres::Error) -> AppError {
    AppError::query(db_message(&err))
}

pub struct PgConnection {
    client: Client,
    /// The one database this session can read the catalog of.
    database: String,
    /// How the session connected; cancel requests connect the same way.
    ssl: SslMode,
    task: JoinHandle<()>,
}

impl PgConnection {
    async fn strings(&self, sql: &str) -> AppResult<Vec<String>> {
        let rows = self.client.query(sql, &[]).await.map_err(query_error)?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    /// Fails for databases other than the connected one: their catalogs
    /// need a session of their own.
    fn check_database(&self, database: &str) -> AppResult<()> {
        if database == self.database {
            Ok(())
        } else {
            Err(AppError::query(format!(
                "\"{database}\" can only be browsed from a connection to it"
            )))
        }
    }
}

#[async_trait]
impl Connection for PgConnection {
    async fn server_info(&self) -> AppResult<ServerInfo> {
        let version: String = self
            .client
            .query_one("SHOW server_version", &[])
            .await
            .map_err(query_error)?
            .get(0);
        Ok(ServerInfo {
            version: format!("PostgreSQL {version}"),
        })
    }

    async fn catalog(&self) -> AppResult<Catalog> {
        let server = self.server_info().await?;
        let databases = self
            .strings("SELECT datname FROM pg_database WHERE NOT datistemplate AND datallowconn ORDER BY 1")
            .await?;
        let mut schemas = catalog::schemas(&self.client).await?;
        let databases = databases
            .into_iter()
            .map(|name| {
                let is_current = name == self.database;
                DatabaseNode {
                    schemas: if is_current {
                        std::mem::take(&mut schemas)
                    } else {
                        Vec::new()
                    },
                    name,
                    is_current,
                }
            })
            .collect();
        Ok(Catalog { server, databases })
    }

    async fn list_objects(&self, database: &str, schema: &str) -> AppResult<SchemaObjects> {
        self.check_database(database)?;
        catalog::objects(&self.client, schema).await
    }

    async fn describe_table(
        &self,
        database: &str,
        schema: &str,
        table: &str,
    ) -> AppResult<TableInfo> {
        self.check_database(database)?;
        let mut tables = catalog::describe(&self.client, schema, Some(table)).await?;
        Ok(tables.remove(0))
    }

    async fn describe_schema(&self, database: &str, schema: &str) -> AppResult<Vec<TableInfo>> {
        self.check_database(database)?;
        catalog::describe(&self.client, schema, None).await
    }

    async fn execute(&self, sql: String) -> AppResult<QueryHandle> {
        execute::execute(&self.client, self.ssl, sql).await
    }

    async fn close(&self) {
        self.task.abort();
    }
}

impl Drop for PgConnection {
    fn drop(&mut self) {
        self.task.abort();
    }
}
