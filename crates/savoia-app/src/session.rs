//! Opening a connection end to end: SSH tunnel (if any), driver, catalog.
//! UI-free; runs on the I/O runtime.

use std::sync::Arc;

use savoia_core::{
    AppError, AppResult, Catalog, Connection, ConnectionConfig, Driver, Engine, Secrets, ServerInfo,
};
use savoia_mysql::MysqlDriver;
use savoia_pg::PgDriver;
use savoia_tunnel::{HostKeyPolicy, KnownHosts, Tunnel};

pub struct Session {
    pub catalog: Catalog,
    // Held for future query execution (M2); dropping it closes the session.
    _conn: Arc<dyn Connection>,
    // Dropped after the connection; keeps the forward alive meanwhile.
    _tunnel: Option<Tunnel>,
}

fn driver(engine: Engine) -> &'static dyn Driver {
    match engine {
        Engine::Postgres => &PgDriver,
        Engine::Mysql => &MysqlDriver,
    }
}

async fn connect(
    config: &ConnectionConfig,
    secrets: &Secrets,
    policy: HostKeyPolicy,
) -> AppResult<(Box<dyn Connection>, Option<Tunnel>)> {
    config.validate()?;
    let (endpoint, tunnel) = match &config.ssh {
        Some(ssh) => {
            let target = (config.host.clone(), config.port);
            let tunnel =
                savoia_tunnel::open(ssh, secrets, target, policy, KnownHosts::User).await?;
            (
                config.endpoint_via("127.0.0.1", tunnel.local_port()),
                Some(tunnel),
            )
        }
        None => (config.endpoint(), None),
    };
    let conn = driver(config.engine).connect(&endpoint, secrets).await?;
    Ok((conn, tunnel))
}

pub async fn open(
    config: ConnectionConfig,
    secrets: Secrets,
    policy: HostKeyPolicy,
) -> AppResult<Session> {
    let (conn, tunnel) = connect(&config, &secrets, policy).await?;
    let catalog = conn.catalog().await?;
    Ok(Session {
        catalog,
        _conn: Arc::from(conn),
        _tunnel: tunnel,
    })
}

/// Connects, reads the server version and disconnects.
pub async fn test(
    config: ConnectionConfig,
    secrets: Secrets,
    policy: HostKeyPolicy,
) -> AppResult<ServerInfo> {
    let (conn, tunnel) = connect(&config, &secrets, policy).await?;
    let info = conn.server_info().await;
    conn.close().await;
    if let Some(tunnel) = tunnel {
        tunnel.close().await;
    }
    info
}

/// Awaits an I/O task, turning a panic or cancellation into an error.
pub async fn join<T>(handle: tokio::task::JoinHandle<AppResult<T>>) -> AppResult<T> {
    handle.await.unwrap_or_else(|e| Err(AppError::internal(e)))
}
