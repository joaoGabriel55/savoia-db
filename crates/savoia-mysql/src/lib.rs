//! MySQL / MariaDB driver built on `mysql_async`. Must run inside a Tokio runtime.

use std::time::Duration;

use async_trait::async_trait;
use mysql_async::prelude::Queryable;
use mysql_async::{
    Conn, DriverError, Error as MyError, OptsBuilder, Pool, PoolConstraints, PoolOpts, SslOpts,
};
use savoia_core::{
    AppError, AppResult, Catalog, Connection, DatabaseNode, Driver, Endpoint, Engine, SchemaNode,
    SchemaObjects, Secrets, ServerInfo, SslMode,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// MySQL server error codes that mean the credentials were rejected.
const ACCESS_DENIED: [u16; 2] = [1044, 1045];

pub struct MysqlDriver;

#[async_trait]
impl Driver for MysqlDriver {
    fn engine(&self) -> Engine {
        Engine::Mysql
    }

    async fn connect(
        &self,
        endpoint: &Endpoint,
        secrets: &Secrets,
    ) -> AppResult<Box<dyn Connection>> {
        let attempt = |tls: Option<SslOpts>| async move {
            let pool = Pool::new(opts(endpoint, secrets, tls));
            // The pool is lazy; take one connection now so errors surface here.
            let conn = tokio::time::timeout(CONNECT_TIMEOUT, pool.get_conn())
                .await
                .map_err(|_| {
                    AppError::connect(format!("timed out after {}s", CONNECT_TIMEOUT.as_secs()))
                })?
                .map_err(map_error);
            match conn {
                Ok(conn) => {
                    drop(conn);
                    Ok(pool)
                }
                Err(err) => {
                    drop(pool.disconnect().await);
                    Err(err)
                }
            }
        };

        let pool = match endpoint.ssl {
            SslMode::Disable => attempt(None).await?,
            SslMode::Prefer => match attempt(Some(ssl_opts(endpoint, false))).await {
                Err(AppError::Connect { message }) if message.contains(NO_SERVER_TLS) => {
                    attempt(None).await?
                }
                other => other?,
            },
            SslMode::Require => attempt(Some(ssl_opts(endpoint, false))).await?,
            SslMode::VerifyFull => attempt(Some(ssl_opts(endpoint, true))).await?,
        };
        Ok(Box::new(MysqlConnection { pool }))
    }
}

/// Substring of the driver error when the server can't do TLS (used by Prefer).
const NO_SERVER_TLS: &str = "server does not have this capability";

fn opts(endpoint: &Endpoint, secrets: &Secrets, tls: Option<SslOpts>) -> OptsBuilder {
    let mut init = Vec::new();
    if endpoint.read_only {
        init.push("SET SESSION TRANSACTION READ ONLY");
    }
    OptsBuilder::default()
        .ip_or_hostname(endpoint.host.clone())
        .tcp_port(endpoint.port)
        .user(Some(endpoint.user.clone()))
        .pass(secrets.password.clone())
        .db_name(endpoint.database.clone())
        .prefer_socket(false)
        .init(init)
        .pool_opts(
            PoolOpts::default().with_constraints(PoolConstraints::new(0, 4).expect("0 <= 4")),
        )
        .ssl_opts(tls)
}

fn ssl_opts(endpoint: &Endpoint, verify: bool) -> SslOpts {
    let opts = SslOpts::default()
        .with_danger_accept_invalid_certs(!verify)
        .with_danger_skip_domain_validation(!verify);
    if endpoint.host != endpoint.tls_server_name {
        // Behind a tunnel: verify against the real host, not 127.0.0.1.
        opts.with_danger_tls_hostname_override(Some(endpoint.tls_server_name.clone()))
    } else {
        opts
    }
}

fn map_error(err: MyError) -> AppError {
    match &err {
        MyError::Server(server) if ACCESS_DENIED.contains(&server.code) => {
            AppError::auth(&server.message)
        }
        MyError::Server(server) => AppError::connect(&server.message),
        MyError::Driver(DriverError::NoClientSslFlagFromServer) => AppError::connect(&err),
        _ => AppError::connect(error_chain(&err)),
    }
}

fn error_chain(err: &dyn std::error::Error) -> String {
    let mut msg = err.to_string();
    let mut source = err.source();
    while let Some(s) = source {
        msg = format!("{msg}: {s}");
        source = s.source();
    }
    msg
}

fn query_error(err: MyError) -> AppError {
    match err {
        MyError::Server(server) => AppError::query(server.message),
        other => AppError::query(error_chain(&other)),
    }
}

pub struct MysqlConnection {
    pool: Pool,
}

impl MysqlConnection {
    async fn conn(&self) -> AppResult<Conn> {
        self.pool.get_conn().await.map_err(map_error)
    }
}

#[async_trait]
impl Connection for MysqlConnection {
    async fn server_info(&self) -> AppResult<ServerInfo> {
        let mut conn = self.conn().await?;
        let version: Option<String> = conn
            .query_first("SELECT VERSION()")
            .await
            .map_err(query_error)?;
        let version = version.unwrap_or_default();
        let product = if version.contains("MariaDB") {
            "MariaDB"
        } else {
            "MySQL"
        };
        let short = version.split('-').next().unwrap_or(&version);
        Ok(ServerInfo {
            version: format!("{product} {short}"),
        })
    }

    async fn catalog(&self) -> AppResult<Catalog> {
        let server = self.server_info().await?;
        let mut conn = self.conn().await?;
        let current: Option<String> = conn
            .query_first::<Option<String>, _>("SELECT DATABASE()")
            .await
            .map_err(query_error)?
            .flatten();
        let names: Vec<String> = conn.query("SHOW DATABASES").await.map_err(query_error)?;

        let mut databases = Vec::with_capacity(names.len());
        for name in names {
            let is_current = current.as_deref() == Some(name.as_str());
            let schemas = if is_current {
                vec![SchemaNode {
                    objects: objects(&mut conn, &name).await?,
                    name: name.clone(),
                }]
            } else {
                Vec::new()
            };
            databases.push(DatabaseNode {
                name,
                is_current,
                schemas,
            });
        }
        Ok(Catalog { server, databases })
    }

    async fn close(&self) {
        drop(self.pool.clone().disconnect().await);
    }
}

async fn objects(conn: &mut Conn, database: &str) -> AppResult<SchemaObjects> {
    let relations: Vec<(String, String)> = conn
        .exec(
            "SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (database,),
        )
        .await
        .map_err(query_error)?;
    let functions: Vec<String> = conn
        .exec(
            "SELECT ROUTINE_NAME FROM information_schema.ROUTINES \
             WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_NAME",
            (database,),
        )
        .await
        .map_err(query_error)?;

    let mut objects = SchemaObjects {
        functions,
        ..SchemaObjects::default()
    };
    for (name, kind) in relations {
        if kind == "VIEW" {
            objects.views.push(name);
        } else {
            objects.tables.push(name);
        }
    }
    Ok(objects)
}
