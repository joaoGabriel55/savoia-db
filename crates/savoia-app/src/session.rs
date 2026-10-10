//! Opening a connection end to end: SSH tunnel (if any), driver, catalog.
//! UI-free; runs on the I/O runtime.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use savoia_core::{
    AppError, AppResult, Catalog, ColumnMeta, Connection, ConnectionConfig, Driver, Endpoint,
    Engine, QueryEvent, QueryHandle, Row, SchemaNode, Secrets, ServerInfo, TableInfo,
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
    /// Catalog-only connections to the other Postgres databases the user
    /// opened, by name. See
    /// `docs/adr/202610091856-open-a-catalog-connection-per-other-postgres-database.md`.
    others: RwLock<HashMap<String, Arc<Other>>>,
    /// Held while opening one of `others`, so two loads don't both open it.
    opening: Mutex<()>,
    /// How `conn` was reached (through the tunnel, if any), for `others`.
    endpoint: Endpoint,
    secrets: Secrets,
    // Dropped after the connections; keeps the forward alive meanwhile.
    _tunnel: Option<Tunnel>,
}

/// A connection to another database of the server, with its own gate.
struct Other {
    conn: Arc<dyn Connection>,
    gate: Arc<Mutex<()>>,
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
) -> AppResult<(Box<dyn Connection>, Endpoint, Option<Tunnel>)> {
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
    Ok((conn, endpoint, tunnel))
}

pub async fn open(
    config: ConnectionConfig,
    secrets: Secrets,
    policy: HostKeyPolicy,
) -> AppResult<Session> {
    let (conn, endpoint, tunnel) = connect(&config, &secrets, policy).await?;
    let catalog = conn.catalog().await?;
    Ok(Session {
        catalog: RwLock::new(Arc::new(catalog)),
        tables: RwLock::default(),
        conn: Arc::from(conn),
        gate: Arc::default(),
        others: RwLock::default(),
        opening: Mutex::default(),
        endpoint,
        secrets,
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

    /// [`Self::is_busy`] for the connection that serves `database`.
    pub fn is_busy_on(&self, database: &str) -> bool {
        self.route(database).1.try_lock().is_err()
    }

    /// The details of a table, if loaded.
    pub fn table(&self, database: &str, schema: &str, table: &str) -> Option<Arc<TableInfo>> {
        let key = (database.to_owned(), schema.to_owned(), table.to_owned());
        read(&self.tables).get(&key).cloned()
    }

    /// The loaded table details of `database`, by (schema, table).
    pub fn tables_in(&self, database: &str) -> Vec<((String, String), Arc<TableInfo>)> {
        read(&self.tables)
            .iter()
            .filter(|((db, _, _), _)| db == database)
            .map(|((_, schema, table), info)| ((schema.clone(), table.clone()), info.clone()))
            .collect()
    }

    /// The connection and gate that serve `database`'s catalog: its own
    /// connection if one is open, else the session's.
    fn route(&self, database: &str) -> (Arc<dyn Connection>, Arc<Mutex<()>>) {
        match read(&self.others).get(database) {
            Some(other) => (other.conn.clone(), other.gate.clone()),
            None => (self.conn.clone(), self.gate.clone()),
        }
    }

    /// Waits for the gate, then reloads the catalog on this connection, the
    /// schemas of the other databases opened before, and the object names
    /// of the schemas that were loaded before. Runs on the I/O runtime.
    pub async fn reload_catalog(&self) -> AppResult<()> {
        let loaded: Vec<(String, String)> = self
            .catalog()
            .databases
            .iter()
            .flat_map(|d| {
                d.schemas
                    .iter()
                    .flatten()
                    .filter(|s| s.objects.is_some())
                    .map(|s| (d.name.clone(), s.name.clone()))
            })
            .collect();
        let mut catalog = {
            let _guard = self.gate.lock().await;
            self.conn.catalog().await?
        };
        // Close the connections of databases that are gone.
        write(&self.others).retain(|name, _| catalog.database(name).is_some());
        let others: Vec<(String, Arc<Other>)> = read(&self.others)
            .iter()
            .map(|(name, other)| (name.clone(), other.clone()))
            .collect();
        for (name, other) in others {
            let schemas = {
                let _guard = other.gate.lock().await;
                current_schemas(other.conn.catalog().await?)
            };
            if let Some(node) = catalog.database_mut(&name) {
                node.schemas = Some(schemas);
            }
        }
        for (database, schema) in loaded {
            if catalog.schema(&database, &schema).is_none() {
                continue;
            }
            let (conn, gate) = self.route(&database);
            let objects = {
                let _guard = gate.lock().await;
                conn.list_objects(&database, &schema).await?
            };
            if let Some(node) = catalog.schema_mut(&database, &schema) {
                node.objects = Some(objects);
            }
        }
        *write(&self.catalog) = Arc::new(catalog);
        write(&self.tables).clear();
        Ok(())
    }

    /// Opens a connection to another Postgres database of the server, if
    /// not open yet, and loads its schemas into the catalog. Runs on the
    /// I/O runtime.
    pub async fn load_schemas(&self, database: &str) -> AppResult<()> {
        if self
            .catalog()
            .database(database)
            .is_none_or(|d| d.is_current)
        {
            return Err(AppError::internal(format!(
                "\"{database}\" has no schemas to load"
            )));
        }
        let other = {
            let _opening = self.opening.lock().await;
            let open = read(&self.others).get(database).cloned();
            match open {
                Some(other) => other,
                None => {
                    let endpoint = Endpoint {
                        database: Some(database.to_owned()),
                        ..self.endpoint.clone()
                    };
                    let conn = driver(self.endpoint.engine)
                        .connect(&endpoint, &self.secrets)
                        .await?;
                    let other = Arc::new(Other {
                        conn: Arc::from(conn),
                        gate: Arc::default(),
                    });
                    write(&self.others).insert(database.to_owned(), other.clone());
                    other
                }
            }
        };
        let schemas = {
            let _guard = other.gate.lock().await;
            current_schemas(other.conn.catalog().await?)
        };
        let mut catalog = write(&self.catalog);
        let mut updated = Catalog::clone(&catalog);
        if let Some(node) = updated.database_mut(database) {
            node.schemas = Some(schemas);
        }
        *catalog = Arc::new(updated);
        Ok(())
    }

    /// Waits for the gate, then loads the object names of one schema into
    /// the catalog. Runs on the I/O runtime.
    pub async fn load_objects(&self, database: &str, schema: &str) -> AppResult<()> {
        let objects = {
            let (conn, gate) = self.route(database);
            let _guard = gate.lock().await;
            conn.list_objects(database, schema).await?
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
            let (conn, gate) = self.route(database);
            let _guard = gate.lock().await;
            Arc::new(conn.describe_table(database, schema, table).await?)
        };
        let key = (database.to_owned(), schema.to_owned(), table.to_owned());
        write(&self.tables).insert(key, info.clone());
        Ok(info)
    }

    /// Waits for the gate, then loads every table of a schema, keeping them
    /// as loaded table details too. Runs on the I/O runtime.
    pub async fn describe_schema(&self, database: &str, schema: &str) -> AppResult<Vec<TableInfo>> {
        let tables = {
            let (conn, gate) = self.route(database);
            let _guard = gate.lock().await;
            conn.describe_schema(database, schema).await?
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

impl Session {
    /// Runs one statement to its end and returns its result set: for small,
    /// bounded reads such as a data view's page. Runs on the I/O runtime.
    pub async fn fetch(&self, sql: String) -> AppResult<(Arc<[ColumnMeta]>, Vec<Row>)> {
        let mut query = self.execute(sql).await?;
        let (mut columns, mut rows): (Arc<[ColumnMeta]>, Vec<Row>) = (Arc::new([]), Vec::new());
        while let Some(event) = query.next().await {
            match event? {
                QueryEvent::Columns(meta) => columns = meta,
                QueryEvent::Rows(page) => rows.extend(page),
                QueryEvent::Done { .. } => {}
            }
        }
        Ok((columns, rows))
    }

    /// Runs `statements` in one transaction, holding the gate throughout.
    /// Each must affect exactly one row; otherwise, or on an error, the
    /// transaction is rolled back and the failing statement's index is
    /// returned with the reason. Runs on the I/O runtime.
    pub async fn transaction(
        &self,
        engine: Engine,
        statements: Vec<String>,
    ) -> Result<(), (Option<usize>, AppError)> {
        let _guard = self.gate.lock().await;
        let begin = match engine {
            Engine::Postgres => "BEGIN",
            Engine::Mysql => "START TRANSACTION",
        };
        run_one(&*self.conn, begin.into())
            .await
            .map_err(|e| (None, e))?;
        for (ix, sql) in statements.into_iter().enumerate() {
            let failure = match run_one(&*self.conn, sql).await {
                Ok(Some(1)) => None,
                Ok(affected) => Some(AppError::query(format!(
                    "expected to change 1 row, changed {}; the row may have been changed or \
                     deleted since it was loaded",
                    affected.map_or("an unknown number".into(), |n| n.to_string())
                ))),
                Err(err) => Some(err),
            };
            if let Some(err) = failure {
                drop(run_one(&*self.conn, "ROLLBACK".into()).await);
                return Err((Some(ix), err));
            }
        }
        run_one(&*self.conn, "COMMIT".into())
            .await
            .map_err(|e| (None, e))?;
        Ok(())
    }

    /// Whether `database` is the one this session's own connection is on,
    /// where queries run. Always true on MySQL, which reaches every database.
    pub fn runs_in(&self, database: &str, engine: Engine) -> bool {
        engine == Engine::Mysql
            || self
                .catalog()
                .database(database)
                .is_some_and(|d| d.is_current)
    }
}

/// Runs one statement to its end without the gate (the caller holds it),
/// returning the rows it affected.
async fn run_one(conn: &dyn Connection, sql: String) -> AppResult<Option<u64>> {
    let mut handle = conn.execute(sql).await?;
    let mut affected = None;
    while let Some(event) = handle.next().await {
        if let QueryEvent::Done { rows_affected, .. } = event? {
            affected = rows_affected;
        }
    }
    Ok(affected)
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

/// The schemas of the database a catalog was loaded on.
fn current_schemas(catalog: Catalog) -> Vec<SchemaNode> {
    catalog
        .databases
        .into_iter()
        .find(|d| d.is_current)
        .and_then(|d| d.schemas)
        .unwrap_or_default()
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
    let (conn, _, tunnel) = connect(&config, &secrets, policy).await?;
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
        AppResult, Cancel, CancelHandle, Catalog, Connection, ConnectionConfig, Engine,
        QueryHandle, QuerySender, SchemaObjects, Secrets, ServerInfo, TableInfo,
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
            others: RwLock::default(),
            opening: Mutex::default(),
            endpoint: ConnectionConfig::new(Engine::Postgres).endpoint(),
            secrets: Secrets::default(),
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
