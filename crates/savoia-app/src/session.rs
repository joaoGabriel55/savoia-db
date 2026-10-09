//! Opening a connection end to end: SSH tunnel (if any), driver, catalog.
//! UI-free; runs on the I/O runtime.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use savoia_core::{
    AppError, AppResult, Catalog, Connection, ConnectionConfig, Driver, Engine, QueryEvent,
    QueryHandle, Secrets, ServerInfo, TableInfo,
};
use savoia_mysql::MysqlDriver;
use savoia_pg::PgDriver;
use savoia_tunnel::{HostKeyPolicy, KnownHosts, Tunnel};
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::runtime;

/// A table by database, schema and name.
pub type TableKey = (String, String, String);

pub struct Session {
    /// The last catalog loaded, with the object names loaded so far. See
    /// `docs/adr/202610091437-load-schema-objects-and-table-details-on-demand.md`.
    catalog: RwLock<Arc<Catalog>>,
    /// Table details loaded so far; cleared by [`Session::reload_catalog`].
    tables: RwLock<HashMap<TableKey, Arc<TableInfo>>>,
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
        catalog: RwLock::new(Arc::new(catalog)),
        tables: RwLock::default(),
        conn: Arc::from(conn),
        gate: Arc::default(),
        _tunnel: tunnel,
    })
}

impl Session {
    pub fn catalog(&self) -> Arc<Catalog> {
        read(&self.catalog).clone()
    }

    /// Whether a query (or another operation) holds the gate right now.
    pub fn is_busy(&self) -> bool {
        self.gate.try_lock().is_err()
    }

    /// The details of a table, if loaded.
    pub fn table(&self, database: &str, schema: &str, table: &str) -> Option<Arc<TableInfo>> {
        let key = (database.to_owned(), schema.to_owned(), table.to_owned());
        read(&self.tables).get(&key).cloned()
    }

    /// Waits for the gate, then reloads the catalog on this connection, and
    /// the object names of the schemas that were loaded before. Runs on the
    /// I/O runtime.
    pub async fn reload_catalog(&self) -> AppResult<()> {
        let loaded: Vec<(String, String)> = self
            .catalog()
            .databases
            .iter()
            .flat_map(|d| {
                d.schemas
                    .iter()
                    .filter(|s| s.objects.is_some())
                    .map(|s| (d.name.clone(), s.name.clone()))
            })
            .collect();
        let catalog = {
            let _guard = self.gate.lock().await;
            let mut catalog = self.conn.catalog().await?;
            for (database, schema) in loaded {
                if let Some(node) = catalog.schema_mut(&database, &schema) {
                    node.objects = Some(self.conn.list_objects(&database, &schema).await?);
                }
            }
            catalog
        };
        *write(&self.catalog) = Arc::new(catalog);
        write(&self.tables).clear();
        Ok(())
    }

    /// Waits for the gate, then loads the object names of one schema into
    /// the catalog. Runs on the I/O runtime.
    pub async fn load_objects(&self, database: &str, schema: &str) -> AppResult<()> {
        let objects = {
            let _guard = self.gate.lock().await;
            self.conn.list_objects(database, schema).await?
        };
        let mut catalog = write(&self.catalog);
        let mut updated = Catalog::clone(&catalog);
        if let Some(node) = updated.schema_mut(database, schema) {
            node.objects = Some(objects);
        }
        *catalog = Arc::new(updated);
        Ok(())
    }

    /// Waits for the gate, then loads one table's details. Runs on the I/O
    /// runtime.
    pub async fn describe_table(
        &self,
        database: &str,
        schema: &str,
        table: &str,
    ) -> AppResult<Arc<TableInfo>> {
        let info = {
            let _guard = self.gate.lock().await;
            Arc::new(self.conn.describe_table(database, schema, table).await?)
        };
        let key = (database.to_owned(), schema.to_owned(), table.to_owned());
        write(&self.tables).insert(key, info.clone());
        Ok(info)
    }

    /// Waits for the gate, then loads every table of a schema, keeping them
    /// as loaded table details too. Runs on the I/O runtime.
    pub async fn describe_schema(&self, database: &str, schema: &str) -> AppResult<Vec<TableInfo>> {
        let tables = {
            let _guard = self.gate.lock().await;
            self.conn.describe_schema(database, schema).await?
        };
        let mut cache = write(&self.tables);
        for table in &tables {
            let key = (database.to_owned(), schema.to_owned(), table.name.clone());
            cache.insert(key, Arc::new(table.clone()));
        }
        Ok(tables)
    }

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

fn read<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex as StdMutex, RwLock};
    use std::time::Duration;

    use async_trait::async_trait;
    use savoia_core::{
        AppResult, Cancel, CancelHandle, Catalog, Connection, QueryHandle, QuerySender,
        SchemaObjects, ServerInfo, TableInfo,
    };
    use tokio::sync::Mutex;

    use super::Session;

    /// Keeps each execution's sender, so a query runs until the test ends it.
    #[derive(Default)]
    struct Fake {
        sender: StdMutex<Option<QuerySender>>,
        catalog_loaded: AtomicBool,
    }

    struct NoCancel;

    #[async_trait]
    impl Cancel for NoCancel {
        async fn cancel(&self) -> AppResult<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Connection for Fake {
        async fn server_info(&self) -> AppResult<ServerInfo> {
            unimplemented!()
        }

        async fn catalog(&self) -> AppResult<Catalog> {
            self.catalog_loaded.store(true, Ordering::SeqCst);
            Ok(catalog("reloaded"))
        }

        async fn list_objects(&self, _: &str, _: &str) -> AppResult<SchemaObjects> {
            unimplemented!()
        }

        async fn describe_table(&self, _: &str, _: &str, _: &str) -> AppResult<TableInfo> {
            unimplemented!()
        }

        async fn describe_schema(&self, _: &str, _: &str) -> AppResult<Vec<TableInfo>> {
            unimplemented!()
        }

        async fn execute(&self, _: String) -> AppResult<QueryHandle> {
            let (tx, handle) = QueryHandle::channel(CancelHandle::new(NoCancel));
            *self.sender.lock().unwrap() = Some(tx);
            Ok(handle)
        }

        async fn close(&self) {}
    }

    fn catalog(version: &str) -> Catalog {
        Catalog {
            server: ServerInfo {
                version: version.into(),
            },
            databases: Vec::new(),
        }
    }

    #[tokio::test]
    async fn catalog_reload_waits_for_the_running_query() {
        let fake = Arc::new(Fake::default());
        let session = Arc::new(Session {
            catalog: RwLock::new(Arc::new(catalog("first"))),
            tables: RwLock::default(),
            conn: fake.clone(),
            gate: Arc::new(Mutex::new(())),
            _tunnel: None,
        });
        let mut query = session.execute("SELECT 1".into()).await.unwrap();
        assert!(session.is_busy());

        let reload = tokio::spawn({
            let session = session.clone();
            async move { session.reload_catalog().await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !fake.catalog_loaded.load(Ordering::SeqCst),
            "ran during the query"
        );

        // The query ends: its sender goes away and the reader sees the end.
        drop(fake.sender.lock().unwrap().take());
        assert!(query.next().await.is_none());
        reload.await.unwrap().unwrap();
        assert_eq!(session.catalog().server.version, "reloaded");
        assert!(!session.is_busy());
    }
}
