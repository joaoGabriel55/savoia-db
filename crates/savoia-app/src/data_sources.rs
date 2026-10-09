//! App-wide connection state: saved connections, their secrets, and live
//! sessions. Views observe this entity and react to its events.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use gpui_kit::{App, AppContext as _, Context, Entity, EventEmitter, Task};
use savoia_core::{AppError, AppResult, ConnectionConfig, ConnectionId, Secrets};
use savoia_store::{ConnectionStore, KeychainSecrets, MemorySecrets, SecretStore};
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
    Error(String),
}

pub struct DataSources {
    store: Option<ConnectionStore>,
    /// Saved secrets: the OS keychain, or memory when no keychain is available.
    saved_secrets: Arc<dyn SecretStore>,
    /// Secrets entered this session (always populated on save), so connecting
    /// doesn't depend on a keychain round trip or on "save password".
    session_secrets: Arc<MemorySecrets>,
    connections: Vec<ConnectionConfig>,
    states: HashMap<ConnectionId, SourceState>,
    refreshes: HashMap<ConnectionId, Refresh>,
    /// Shown in the explorer when local storage can't be opened.
    pub storage_error: Option<String>,
}

impl EventEmitter<DataSourcesEvent> for DataSources {}

impl DataSources {
    /// Uses the app's SQLite file and the OS keychain.
    pub fn new(_: &mut Context<Self>) -> Self {
        let saved_secrets: Arc<dyn SecretStore> = if KeychainSecrets::available() {
            Arc::new(KeychainSecrets)
        } else {
            Arc::new(MemorySecrets::default())
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

    pub fn connections(&self) -> &[ConnectionConfig] {
        &self.connections
    }

    pub fn get(&self, id: ConnectionId) -> Option<&ConnectionConfig> {
        self.connections.iter().find(|c| c.id == id)
    }

    pub fn state(&self, id: ConnectionId) -> &SourceState {
        self.states.get(&id).unwrap_or(&SourceState::Disconnected)
    }

    pub fn refresh_state(&self, id: ConnectionId) -> Option<RefreshState> {
        self.refreshes.get(&id).map(|r| r.state)
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
            let result = async {
                let secrets = secrets.await?;
                session::join(runtime::spawn(session::open(config, secrets, policy))).await
            }
            .await;
            this.update(cx, |this, cx| this.finish_connect(id, result, cx))
                .ok();
        })
        .detach();
    }

    fn finish_connect(
        &mut self,
        id: ConnectionId,
        result: AppResult<Session>,
        cx: &mut Context<Self>,
    ) {
        let state = match result {
            Ok(session) => {
                if let Some(store) = &self.store {
                    drop(store.touch(id, SystemTime::now()));
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
            Err(err) => {
                let name = self.get(id).map(|c| c.display_name()).unwrap_or_default();
                cx.emit(DataSourcesEvent::Error(format!("{name}: {err}")));
                SourceState::Failed(err.to_string())
            }
        };
        self.set_state(id, state);
        cx.emit(DataSourcesEvent::Changed);
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
