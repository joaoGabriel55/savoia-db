//! PostgreSQL driver built on `tokio-postgres`. Must run inside a Tokio runtime.

mod tls;

use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;
use savoia_core::{
    AppError, AppResult, Catalog, Connection, DatabaseNode, Driver, Endpoint, Engine, QueryHandle,
    SchemaNode, SchemaObjects, Secrets, ServerInfo, SslMode,
};
use tokio::task::JoinHandle;
use tokio_postgres::{Client, Config, NoTls, config::SslMode as PgSslMode, error::SqlState};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SYSTEM_SCHEMA_FILTER: &str =
    "n.nspname NOT LIKE 'pg\\_%' AND n.nspname <> 'information_schema'";

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
        Ok(Box::new(PgConnection { client, task }))
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
        .dbname(endpoint.database.as_deref().unwrap_or("postgres"))
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
    task: JoinHandle<()>,
}

impl PgConnection {
    async fn strings(&self, sql: &str) -> AppResult<Vec<String>> {
        let rows = self.client.query(sql, &[]).await.map_err(query_error)?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    async fn pairs(&self, sql: &str) -> AppResult<Vec<(String, String, String)>> {
        let rows = self.client.query(sql, &[]).await.map_err(query_error)?;
        Ok(rows
            .iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect())
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
        let current: String = self
            .client
            .query_one("SELECT current_database()", &[])
            .await
            .map_err(query_error)?
            .get(0);
        let databases = self
            .strings("SELECT datname FROM pg_database WHERE NOT datistemplate AND datallowconn ORDER BY 1")
            .await?;
        let schema_names = self
            .strings(&format!(
                "SELECT n.nspname FROM pg_namespace n WHERE {SYSTEM_SCHEMA_FILTER} ORDER BY 1"
            ))
            .await?;
        let relations = self
            .pairs(&format!(
                "SELECT n.nspname, c.relname, c.relkind::text FROM pg_class c \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE c.relkind IN ('r','p','v','m','S','f') AND NOT c.relispartition AND {SYSTEM_SCHEMA_FILTER} \
                 ORDER BY 1, 2"
            ))
            .await?;
        let functions = self
            .pairs(&format!(
                "SELECT DISTINCT n.nspname, p.proname, '' FROM pg_proc p \
                 JOIN pg_namespace n ON n.oid = p.pronamespace \
                 WHERE p.prokind IN ('f','p') AND {SYSTEM_SCHEMA_FILTER} ORDER BY 1, 2"
            ))
            .await?;

        let mut schemas: Vec<SchemaNode> = schema_names
            .into_iter()
            .map(|name| SchemaNode {
                name,
                objects: SchemaObjects::default(),
            })
            .collect();
        for (schema, name, kind) in relations {
            if let Some(objects) = objects_of(&mut schemas, &schema) {
                match kind.as_str() {
                    "r" | "p" | "f" => objects.tables.push(name),
                    "v" | "m" => objects.views.push(name),
                    "S" => objects.sequences.push(name),
                    _ => {}
                }
            }
        }
        for (schema, name, _) in functions {
            if let Some(objects) = objects_of(&mut schemas, &schema) {
                objects.functions.push(name);
            }
        }

        let databases = databases
            .into_iter()
            .map(|name| {
                let is_current = name == current;
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

    async fn execute(&self, _sql: String) -> AppResult<QueryHandle> {
        // Replaced by the streaming implementation in the next commit.
        Err(AppError::query(
            "query execution is not implemented yet for PostgreSQL",
        ))
    }

    async fn close(&self) {
        self.task.abort();
    }
}

fn objects_of<'a>(schemas: &'a mut [SchemaNode], schema: &str) -> Option<&'a mut SchemaObjects> {
    schemas
        .iter_mut()
        .find(|s| s.name == schema)
        .map(|s| &mut s.objects)
}

impl Drop for PgConnection {
    fn drop(&mut self) {
        self.task.abort();
    }
}
