//! App-wide connection state: saved connections, their secrets, and live
//! sessions. Views observe this entity and react to its events.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use gpui_kit::{App, AppContext as _, Context, Entity, EventEmitter, Task};
use savoia_core::{AppError, AppResult, ConnectionConfig, ConnectionId, Secrets};
use savoia_store::{ConnectionStore, FileSecrets, HistoryEntry, MemorySecrets, SecretStore};
use savoia_tunnel::HostKeyPolicy;

use crate::runtime;
use crate::session::{self, Session};

pub enum SourceState {
    Disconnected,
    Connecting,
    Connected(Arc<Session>),
    Failed(String),
}

/// A catalog refresh in progress on a connected source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshState {
    /// A query holds the session's gate; the refresh runs once it ends.
    Waiting,
    Loading,
}

/// Dropping it abandons the refresh, including its wait for the gate.
struct Refresh {
    state: RefreshState,
    io: tokio::task::AbortHandle,
    _task: Task<()>,
}

impl Drop for Refresh {
    fn drop(&mut self) {
        self.io.abort();
    }
}

/// Part of a connected source's catalog that loads when its tree node opens.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LoadTarget {
    /// The schemas of another Postgres database, on a connection of its own.
    Schemas {
        database: String,
    },
    Objects {
        database: String,
        schema: String,
    },
    Table {
        database: String,
        schema: String,
        table: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadState {
    /// A query holds the session's gate; the load runs once it ends.
    Waiting,
    Loading,
    /// Kept until the next Refresh, so a failing node doesn't retry in a loop.
    Failed(String),
}

struct Load {
    state: LoadState,
    /// `None` once finished. Dropping it abandons the load.
    _io: Option<Io>,
}

/// An I/O task and the UI task awaiting it; dropping it aborts both.
struct Io {
    io: tokio::task::AbortHandle,
    _task: Task<()>,
}

impl Drop for Io {
    fn drop(&mut self) {
        self.io.abort();
    }
}

pub enum DataSourcesEvent {
    /// The list or a connection state changed.
    Changed,
    /// Connecting stopped at an unknown SSH host key; ask the user, then call
    /// [`DataSources::connect`] with [`HostKeyPolicy::TrustUnknown`].
    NeedsHostTrust {
        id: ConnectionId,
        host: String,
        fingerprint: String,
    },
    /// The server refused the login: no password was stored, or the stored
    /// one is wrong. Ask for it, then call [`DataSources::connect_with_password`].
    NeedsPassword {
        id: ConnectionId,
        /// Why the last attempt failed, when a password was tried.
        error: Option<String>,
    },
    Error(String),
}

pub struct DataSources {
    store: Option<ConnectionStore>,
    /// Saved secrets: the user-only secrets file, or memory when there is no
    /// data directory.
    saved_secrets: Arc<dyn SecretStore>,
    /// Secrets entered this session (always populated on save), so connecting
    /// doesn't depend on a file read or on "save password".
    session_secrets: Arc<MemorySecrets>,
    connections: Vec<ConnectionConfig>,
    states: HashMap<ConnectionId, SourceState>,
    refreshes: HashMap<ConnectionId, Refresh>,
    loads: HashMap<(ConnectionId, LoadTarget), Load>,
    /// Shown in the explorer when local storage can't be opened.
    pub storage_error: Option<String>,
}

impl EventEmitter<DataSourcesEvent> for DataSources {}

impl DataSources {
    /// Uses the app's SQLite file and secrets file.
    pub fn new(_: &mut Context<Self>) -> Self {
        let saved_secrets: Arc<dyn SecretStore> = match FileSecrets::open_default() {
            Ok(file) => Arc::new(file),
            Err(_) => Arc::new(MemorySecrets::default()),
        };
        Self::with_stores(ConnectionStore::open_default(), saved_secrets)
    }

    pub fn with_stores(
        store: AppResult<ConnectionStore>,
        saved_secrets: Arc<dyn SecretStore>,
    ) -> Self {
        let (store, storage_error) = match store {
            Ok(store) => (Some(store), None),
            Err(err) => (None, Some(err.to_string())),
        };
        let mut this = Self {
            store,
            saved_secrets,
            session_secrets: Arc::default(),
            connections: Vec::new(),
            states: HashMap::new(),
            refreshes: HashMap::new(),
            loads: HashMap::new(),
            storage_error,
        };
        this.reload();
        this
    }

    fn reload(&mut self) {
        let Some(store) = &self.store else { return };
        match store.list() {
            Ok(list) => self.connections = list.into_iter().map(|c| c.config).collect(),
            Err(err) => self.storage_error = Some(err.to_string()),
        }
    }

    /// The data source connected most recently, if any.
    pub fn most_recent(&self) -> Option<ConnectionId> {
        let store = self.store.as_ref()?;
        store.recent(1).ok()?.first().map(|c| c.config.id)
    }

    pub fn connections(&self) -> &[ConnectionConfig] {
        &self.connections
    }

    pub fn get(&self, id: ConnectionId) -> Option<&ConnectionConfig> {
        self.connections.iter().find(|c| c.id == id)
    }

    /// Adds a console run to the query history. History is best-effort:
    /// a failure to store it never interrupts the user.
    pub fn record_history(&mut self, entry: &HistoryEntry) {
        if let Some(store) = &self.store {
            drop(store.record(entry));
        }
    }

    /// The latest runs whose SQL contains `search`, newest first.
    pub fn history(&self, search: &str, limit: usize) -> Vec<HistoryEntry> {
        self.store
            .as_ref()
            .and_then(|store| store.history(search, limit).ok())
            .unwrap_or_default()
    }

    /// A saved preference; best-effort like history.
    pub fn setting(&self, key: &str) -> Option<String> {
        self.store.as_ref()?.setting(key).ok().flatten()
    }

    /// Saves a preference (`None` removes it); a failure is ignored.
    pub fn set_setting(&mut self, key: &str, value: Option<&str>) {
        if let Some(store) = &self.store {
            drop(store.set_setting(key, value));
        }
    }

    pub fn state(&self, id: ConnectionId) -> &SourceState {
        self.states.get(&id).unwrap_or(&SourceState::Disconnected)
    }

    pub fn refresh_state(&self, id: ConnectionId) -> Option<RefreshState> {
        self.refreshes.get(&id).map(|r| r.state)
    }

    pub fn session(&self, id: ConnectionId) -> Option<Arc<Session>> {
        match self.state(id) {
            SourceState::Connected(session) => Some(session.clone()),
            _ => None,
        }
    }

    /// `None` when not loading and not failed: loaded, or never asked for.
    pub fn load_state(&self, id: ConnectionId, target: &LoadTarget) -> Option<&LoadState> {
        self.loads.get(&(id, target.clone())).map(|l| &l.state)
    }

    /// Loads part of a connected source's catalog into its session, after
    /// any running query. Does nothing if that load is running or failed.
    pub fn load(&mut self, id: ConnectionId, target: LoadTarget, cx: &mut Context<Self>) {
        let key = (id, target.clone());
        if self.loads.contains_key(&key) || self.refreshes.contains_key(&id) {
            return;
        }
        let Some(session) = self.session(id) else {
            return;
        };
        let database = match &target {
            LoadTarget::Schemas { database }
            | LoadTarget::Objects { database, .. }
            | LoadTarget::Table { database, .. } => database,
        };
        let state = if session.is_busy_on(database) {
            LoadState::Waiting
        } else {
            LoadState::Loading
        };
        let io = runtime::spawn(async move {
            match target {
                LoadTarget::Schemas { database } => session.load_schemas(&database).await,
                LoadTarget::Objects { database, schema } => {
                    session.load_objects(&database, &schema).await.map(drop)
                }
                LoadTarget::Table {
                    database,
                    schema,
                    table,
                } => session
                    .describe_table(&database, &schema, &table)
                    .await
                    .map(drop),
            }
        });
        let abort = io.abort_handle();
        let task = cx.spawn({
            let key = key.clone();
            async move |this, cx| {
                let result = session::join(io).await;
                this.update(cx, |this, cx| this.finish_load(key, result, cx))
                    .ok();
            }
        });
        self.loads.insert(
            key,
            Load {
                state,
                _io: Some(Io {
                    io: abort,
                    _task: task,
                }),
            },
        );
        cx.emit(DataSourcesEvent::Changed);
    }

    fn finish_load(
        &mut self,
        key: (ConnectionId, LoadTarget),
        result: AppResult<()>,
        cx: &mut Context<Self>,
    ) {
        // Abandoned (refreshed, disconnected or reconnected) while it ran.
        let Some(load) = self.loads.get_mut(&key) else {
            return;
        };
        match result {
            Ok(()) => {
                self.loads.remove(&key);
            }
            Err(err) => {
                load.state = LoadState::Failed(err.to_string());
                load._io = None;
            }
        }
        cx.emit(DataSourcesEvent::Changed);
    }

    fn forget_loads(&mut self, id: ConnectionId) {
        self.loads.retain(|(source, _), _| *source != id);
    }

    /// Loads the secrets for `id` off the UI thread: this session's first,
    /// then the saved ones.
    pub fn load_secrets(
        &self,
        id: ConnectionId,
    ) -> impl Future<Output = AppResult<Secrets>> + use<> {
        let session = self.session_secrets.clone();
        let saved = self.saved_secrets.clone();
        async move {
            let entered = session.load(id)?;
            if entered != Secrets::default() {
                return Ok(entered);
            }
            session::join(runtime::spawn_blocking(move || saved.load(id))).await
        }
    }

    pub fn save(
        &mut self,
        config: ConnectionConfig,
        secrets: Secrets,
        cx: &mut Context<Self>,
    ) -> AppResult<()> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| AppError::storage("local storage is unavailable"))?;
        store.save(&config)?;
        self.session_secrets.save(config.id, &secrets)?;

        let saved = self.saved_secrets.clone();
        let (id, keep) = (config.id, config.save_password);
        cx.spawn(async move |this, cx| {
            let result = runtime::spawn_blocking(move || {
                if keep {
                    saved.save(id, &secrets)
                } else {
                    saved.delete(id)
                }
            });
            if let Err(err) = session::join(result).await {
                this.update(cx, |_, cx| {
                    cx.emit(DataSourcesEvent::Error(format!(
                        "Could not store the password: {err}"
                    )))
                })
                .ok();
            }
        })
        .detach();

        self.reload();
        cx.emit(DataSourcesEvent::Changed);
        Ok(())
    }

    /// Replaces the state of `id`, releasing any old session on the I/O
    /// runtime (pools and SSH handles may need it while shutting down).
    fn set_state(&mut self, id: ConnectionId, state: SourceState) {
        self.refreshes.remove(&id);
        self.forget_loads(id);
        if let Some(SourceState::Connected(session)) = self.states.insert(id, state) {
            drop(runtime::spawn(async move { drop(session) }));
        }
    }

    pub fn delete(&mut self, id: ConnectionId, cx: &mut Context<Self>) {
        self.set_state(id, SourceState::Disconnected);
        self.states.remove(&id);
        if let Some(store) = &self.store
            && let Err(err) = store.delete(id)
        {
            cx.emit(DataSourcesEvent::Error(err.to_string()));
        }
        drop(self.session_secrets.delete(id));
        let saved = self.saved_secrets.clone();
        drop(runtime::spawn_blocking(move || saved.delete(id)));
        self.reload();
        cx.emit(DataSourcesEvent::Changed);
    }

    pub fn connect(&mut self, id: ConnectionId, policy: HostKeyPolicy, cx: &mut Context<Self>) {
        self.start_connect(id, policy, None, cx);
    }

    /// Connects with the password the user just typed in place of the
    /// stored one. Once it works, it is kept for this run, and saved too
    /// if the connection saves passwords.
    pub fn connect_with_password(
        &mut self,
        id: ConnectionId,
        password: String,
        cx: &mut Context<Self>,
    ) {
        self.start_connect(id, HostKeyPolicy::KnownOnly, Some(password), cx);
    }

    fn start_connect(
        &mut self,
        id: ConnectionId,
        policy: HostKeyPolicy,
        password: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(config) = self.get(id).cloned() else {
            return;
        };
        if matches!(self.state(id), SourceState::Connecting) {
            return;
        }
        self.set_state(id, SourceState::Connecting);
        cx.emit(DataSourcesEvent::Changed);

        let secrets = self.load_secrets(id);
        cx.spawn(async move |this, cx| {
            let mut tried = false;
            let result = async {
                let mut secrets = secrets.await?;
                let entered = password.is_some();
                if entered {
                    secrets.password = password;
                }
                tried = secrets.password.as_ref().is_some_and(|p| !p.is_empty());
                let open = session::open(config, secrets.clone(), policy);
                let session = session::join(runtime::spawn(open)).await?;
                Ok((session, entered.then_some(secrets)))
            }
            .await;
            this.update(cx, |this, cx| this.finish_connect(id, result, tried, cx))
                .ok();
        })
        .detach();
    }

    fn finish_connect(
        &mut self,
        id: ConnectionId,
        result: AppResult<(Session, Option<Secrets>)>,
        tried_password: bool,
        cx: &mut Context<Self>,
    ) {
        let state = match result {
            Ok((session, entered)) => {
                if let Some(store) = &self.store {
                    drop(store.touch(id, SystemTime::now()));
                }
                if let Some(secrets) = entered {
                    self.remember(id, secrets, cx);
                }
                SourceState::Connected(Arc::new(session))
            }
            Err(AppError::UnknownHostKey { host, fingerprint }) => {
                cx.emit(DataSourcesEvent::NeedsHostTrust {
                    id,
                    host,
                    fingerprint,
                });
                SourceState::Disconnected
            }
            Err(err @ AppError::Auth { .. }) => {
                cx.emit(DataSourcesEvent::NeedsPassword {
                    id,
                    error: tried_password.then(|| err.to_string()),
                });
                SourceState::Failed(err.to_string())
            }
            Err(err) => {
                let name = self.get(id).map(|c| c.display_name()).unwrap_or_default();
                cx.emit(DataSourcesEvent::Error(format!("{name}: {err}")));
                SourceState::Failed(err.to_string())
            }
        };
        self.set_state(id, state);
        cx.emit(DataSourcesEvent::Changed);
    }

    /// Keeps secrets that just worked for this run, and in the secrets file
    /// if the connection saves passwords.
    fn remember(&mut self, id: ConnectionId, secrets: Secrets, cx: &mut Context<Self>) {
        drop(self.session_secrets.save(id, &secrets));
        if !self.get(id).is_some_and(|c| c.save_password) {
            return;
        }
        let saved = self.saved_secrets.clone();
        cx.spawn(async move |this, cx| {
            let result = runtime::spawn_blocking(move || saved.save(id, &secrets));
            if let Err(err) = session::join(result).await {
                this.update(cx, |_, cx| {
                    cx.emit(DataSourcesEvent::Error(format!(
                        "Could not store the password: {err}"
                    )))
                })
                .ok();
            }
        })
        .detach();
    }

    /// Reloads the catalog of a connected source on its own connection,
    /// after any running query (see
    /// `docs/adr/202610091309-serialize-all-work-on-one-connection-per-session.md`).
    /// Reconnects if the reload fails or the source isn't connected.
    pub fn refresh(&mut self, id: ConnectionId, cx: &mut Context<Self>) {
        let SourceState::Connected(session) = self.state(id) else {
            self.connect(id, HostKeyPolicy::KnownOnly, cx);
            return;
        };
        if self.refreshes.contains_key(&id) {
            return;
        }
        let state = if session.is_busy() {
            RefreshState::Waiting
        } else {
            RefreshState::Loading
        };
        let session = session.clone();
        // The reload brings back loaded object names; failed loads get retried.
        self.forget_loads(id);
        let io = runtime::spawn(async move { session.reload_catalog().await });
        let abort = io.abort_handle();
        let task = cx.spawn(async move |this, cx| {
            let result = session::join(io).await;
            this.update(cx, |this, cx| this.finish_refresh(id, result, cx))
                .ok();
        });
        self.refreshes.insert(
            id,
            Refresh {
                state,
                io: abort,
                _task: task,
            },
        );
        cx.emit(DataSourcesEvent::Changed);
    }

    fn finish_refresh(&mut self, id: ConnectionId, result: AppResult<()>, cx: &mut Context<Self>) {
        // Abandoned (disconnected or reconnected) while it ran.
        if self.refreshes.remove(&id).is_none() {
            return;
        }
        if result.is_err() {
            self.connect(id, HostKeyPolicy::KnownOnly, cx);
        }
        cx.emit(DataSourcesEvent::Changed);
    }

    pub fn disconnect(&mut self, id: ConnectionId, cx: &mut Context<Self>) {
        // Dropping the session closes the connection and the tunnel.
        self.set_state(id, SourceState::Disconnected);
        cx.emit(DataSourcesEvent::Changed);
    }
}

pub fn init(cx: &mut App) -> Entity<DataSources> {
    cx.new(DataSources::new)
}
