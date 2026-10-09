//! Opening a connection end to end: SSH tunnel (if any), driver, catalog.
//! UI-free; runs on the I/O runtime.

use std::sync::Arc;

use savoia_core::{
    AppError, AppResult, Catalog, Connection, ConnectionConfig, Driver, Engine, QueryEvent,
    QueryHandle, Secrets, ServerInfo,
};
use savoia_mysql::MysqlDriver;
use savoia_pg::PgDriver;
use savoia_tunnel::{HostKeyPolicy, KnownHosts, Tunnel};
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::runtime;

pub struct Session {
    pub catalog: Catalog,
    // Dropping it closes the session.
    conn: Arc<dyn Connection>,
    /// One operation at a time on `conn`. See
    /// `docs/adr/202610091309-serialize-all-work-on-one-connection-per-session.md`.
    gate: Arc<Mutex<()>>,
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
        conn: Arc::from(conn),
        gate: Arc::default(),
        _tunnel: tunnel,
    })
}

impl Session {
    /// Waits for the gate, then starts `sql`. The gate stays taken until the
    /// returned query ends. Runs on the I/O runtime.
    pub async fn execute(&self, sql: String) -> AppResult<RunningQuery> {
        let guard = self.gate.clone().lock_owned().await;
        let handle = self.conn.execute(sql).await?;
        Ok(RunningQuery {
            inner: Some((handle, guard)),
        })
    }
}

/// A query that holds its session's gate. Dropping it before the end cancels
/// the query and keeps the gate until the driver has drained it.
pub struct RunningQuery {
    inner: Option<(QueryHandle, OwnedMutexGuard<()>)>,
}

impl RunningQuery {
    /// The next event, or `None` once the execution has ended. After `None`
    /// (or an `Err`) the gate is free.
    pub async fn next(&mut self) -> Option<AppResult<QueryEvent>> {
        let (handle, _) = self.inner.as_mut()?;
        let event = handle.next().await;
        if matches!(event, None | Some(Err(_))) {
            self.inner = None;
        }
        event
    }
}

impl Drop for RunningQuery {
    fn drop(&mut self) {
        let Some((mut handle, guard)) = self.inner.take() else {
            return;
        };
        drop(runtime::spawn(async move {
            drop(handle.cancel_handle().cancel().await);
            while handle.next().await.is_some() {}
            drop(guard);
        }));
    }
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
