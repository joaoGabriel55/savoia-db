//! Local persistence: saved connections and query history in SQLite, secrets
//! in a file only the user can read. See ADR "Store connection secrets in a
//! user-only file".

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::params;
use savoia_core::{AppError, AppResult, ConnectionConfig, ConnectionId, Secrets};

/// A saved connection plus usage metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedConnection {
    pub config: ConnectionConfig,
    pub last_used_at: Option<SystemTime>,
}

/// One run of the query console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub connection: ConnectionId,
    pub sql: String,
    pub ran_at: SystemTime,
    pub duration: Duration,
    /// Rows returned plus rows affected, over every statement of the run.
    pub rows: u64,
    /// Set when the run failed.
    pub error: Option<String>,
}

/// History keeps the most recent runs only.
const HISTORY_LIMIT: i64 = 10_000;

/// Saved connections and query history. Not `Sync`; keep it on one thread
/// or behind a mutex.
pub struct ConnectionStore {
    db: rusqlite::Connection,
}

/// Schema migrations, applied in order; `PRAGMA user_version` is the index of
/// the last one applied. Append only.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE connections (
        id           TEXT PRIMARY KEY,
        config       TEXT NOT NULL,
        created_at   INTEGER NOT NULL,
        last_used_at INTEGER
    );",
    // Query text is kept as typed, so it can hold anything the user ran,
    // secrets included; it lives next to the connections, private to the user.
    "CREATE TABLE history (
        id            INTEGER PRIMARY KEY,
        connection_id TEXT NOT NULL,
        sql           TEXT NOT NULL,
        ran_at        INTEGER NOT NULL,
        duration_ms   INTEGER NOT NULL,
        rows          INTEGER NOT NULL,
        error         TEXT
    );
    CREATE INDEX history_by_time ON history (ran_at DESC);",
    // App preferences by key, e.g. the directory dump tools are taken from.
    "CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );",
];

impl ConnectionStore {
    /// Opens `<data dir>/savoia-db/savoia.sqlite`, creating it if needed.
    pub fn open_default() -> AppResult<Self> {
        Self::open(&default_path()?)
    }

    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(AppError::storage)?;
        }
        Self::init(rusqlite::Connection::open(path).map_err(AppError::storage)?)
    }

    pub fn open_in_memory() -> AppResult<Self> {
        Self::init(rusqlite::Connection::open_in_memory().map_err(AppError::storage)?)
    }

    fn init(db: rusqlite::Connection) -> AppResult<Self> {
        let version: i64 = db
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(AppError::storage)?;
        for (ix, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            db.execute_batch(sql).map_err(AppError::storage)?;
            db.pragma_update(None, "user_version", ix as i64 + 1)
                .map_err(AppError::storage)?;
        }
        Ok(Self { db })
    }

    /// All saved connections, sorted by display name.
    pub fn list(&self) -> AppResult<Vec<SavedConnection>> {
        let mut list = self.query("SELECT config, last_used_at FROM connections", [])?;
        list.sort_by_key(|c| c.config.display_name().to_lowercase());
        Ok(list)
    }

    /// Most recently used first.
    pub fn recent(&self, limit: usize) -> AppResult<Vec<SavedConnection>> {
        self.query(
            "SELECT config, last_used_at FROM connections WHERE last_used_at IS NOT NULL \
             ORDER BY last_used_at DESC LIMIT ?1",
            [limit as i64],
        )
    }

    pub fn get(&self, id: ConnectionId) -> AppResult<Option<SavedConnection>> {
        Ok(self
            .query(
                "SELECT config, last_used_at FROM connections WHERE id = ?1",
                [id.to_string()],
            )?
            .pop())
    }

    /// Inserts or updates by id.
    pub fn save(&self, config: &ConnectionConfig) -> AppResult<()> {
        let json = serde_json::to_string(config).map_err(AppError::storage)?;
        self.db
            .execute(
                "INSERT INTO connections (id, config, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET config = excluded.config",
                params![config.id.to_string(), json, unix_millis(SystemTime::now())],
            )
            .map_err(AppError::storage)?;
        Ok(())
    }

    /// Deletes the connection and its query history.
    pub fn delete(&self, id: ConnectionId) -> AppResult<()> {
        for sql in [
            "DELETE FROM connections WHERE id = ?1",
            "DELETE FROM history WHERE connection_id = ?1",
        ] {
            self.db
                .execute(sql, [id.to_string()])
                .map_err(AppError::storage)?;
        }
        Ok(())
    }

    /// Adds a run to the history, dropping the oldest beyond the limit.
    pub fn record(&self, entry: &HistoryEntry) -> AppResult<()> {
        self.db
            .execute(
                "INSERT INTO history (connection_id, sql, ran_at, duration_ms, rows, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    entry.connection.to_string(),
                    entry.sql,
                    unix_millis(entry.ran_at),
                    entry.duration.as_millis() as i64,
                    entry.rows as i64,
                    entry.error,
                ],
            )
            .map_err(AppError::storage)?;
        self.db
            .execute(
                "DELETE FROM history WHERE id <= (SELECT id FROM history ORDER BY id DESC LIMIT 1 OFFSET ?1)",
                [HISTORY_LIMIT],
            )
            .map_err(AppError::storage)?;
        Ok(())
    }

    /// The latest runs whose SQL contains `search` (ignoring ASCII case),
    /// newest first.
    pub fn history(&self, search: &str, limit: usize) -> AppResult<Vec<HistoryEntry>> {
        let pattern = format!(
            "%{}%",
            search
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut stmt = self
            .db
            .prepare(
                "SELECT connection_id, sql, ran_at, duration_ms, rows, error FROM history
                 WHERE sql LIKE ?1 ESCAPE '\\' ORDER BY id DESC LIMIT ?2",
            )
            .map_err(AppError::storage)?;
        let rows = stmt
            .query_map(params![pattern, limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })
            .map_err(AppError::storage)?;
        rows.map(|row| {
            let (id, sql, ran_at, ms, rows, error) = row.map_err(AppError::storage)?;
            Ok(HistoryEntry {
                connection: ConnectionId(id.parse().map_err(AppError::storage)?),
                sql,
                ran_at: UNIX_EPOCH + Duration::from_millis(ran_at as u64),
                duration: Duration::from_millis(ms as u64),
                rows: rows as u64,
                error,
            })
        })
        .collect()
    }

    /// A preference, if set.
    pub fn setting(&self, key: &str) -> AppResult<Option<String>> {
        use rusqlite::OptionalExtension as _;
        self.db
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(AppError::storage)
    }

    /// Sets a preference; `None` removes it.
    pub fn set_setting(&self, key: &str, value: Option<&str>) -> AppResult<()> {
        match value {
            Some(value) => self.db.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            ),
            None => self
                .db
                .execute("DELETE FROM settings WHERE key = ?1", [key]),
        }
        .map_err(AppError::storage)?;
        Ok(())
    }

    /// Records a successful connect, for the recent list.
    pub fn touch(&self, id: ConnectionId, at: SystemTime) -> AppResult<()> {
        self.db
            .execute(
                "UPDATE connections SET last_used_at = ?2 WHERE id = ?1",
                params![id.to_string(), unix_millis(at)],
            )
            .map_err(AppError::storage)?;
        Ok(())
    }

    fn query(&self, sql: &str, params: impl rusqlite::Params) -> AppResult<Vec<SavedConnection>> {
        let mut stmt = self.db.prepare(sql).map_err(AppError::storage)?;
        let rows = stmt
            .query_map(params, |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
            })
            .map_err(AppError::storage)?;
        rows.map(|row| {
            let (json, last_used) = row.map_err(AppError::storage)?;
            Ok(SavedConnection {
                config: serde_json::from_str(&json).map_err(AppError::storage)?,
                last_used_at: last_used.map(|ms| UNIX_EPOCH + Duration::from_millis(ms as u64)),
            })
        })
        .collect()
    }
}

fn data_dir() -> AppResult<PathBuf> {
    let dir = dirs::data_dir().ok_or_else(|| AppError::storage("no user data directory"))?;
    // Kept from the Savoia DB name so existing installs keep their
    // connections and saved passwords.
    Ok(dir.join("savoia-db"))
}

fn default_path() -> AppResult<PathBuf> {
    Ok(data_dir()?.join("savoia.sqlite"))
}

fn unix_millis(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

/// Where connection secrets live. Implementations block; call them off the UI thread.
pub trait SecretStore: Send + Sync {
    fn load(&self, id: ConnectionId) -> AppResult<Secrets>;
    /// Stores the `Some` fields and removes the `None` ones.
    fn save(&self, id: ConnectionId, secrets: &Secrets) -> AppResult<()>;
    fn delete(&self, id: ConnectionId) -> AppResult<()> {
        self.save(id, &Secrets::default())
    }
}

/// Secrets in a JSON file readable only by the user (`0600`, like `~/.pgpass`),
/// keyed by connection id.
pub struct FileSecrets {
    path: PathBuf,
    /// One read-modify-write at a time within the process.
    lock: Mutex<()>,
}

const FIELDS: [&str; 3] = ["password", "ssh-password", "ssh-key-passphrase"];

fn fields(secrets: &mut Secrets) -> [&mut Option<String>; 3] {
    [
        &mut secrets.password,
        &mut secrets.ssh_password,
        &mut secrets.ssh_key_passphrase,
    ]
}

type SecretMap = serde_json::Map<String, serde_json::Value>;

impl FileSecrets {
    /// `<data dir>/savoia-db/secrets.json`; created on the first save.
    pub fn open_default() -> AppResult<Self> {
        Ok(Self::at(data_dir()?.join("secrets.json")))
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    fn read_all(&self) -> AppResult<SecretMap> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(AppError::storage),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(SecretMap::new()),
            Err(err) => Err(AppError::storage(err)),
        }
    }

    /// Replaces the file through a temporary one, so it is never half written.
    fn write_all(&self, map: &SecretMap) -> AppResult<()> {
        let dir = self
            .path
            .parent()
            .ok_or_else(|| AppError::storage("secrets file has no directory"))?;
        std::fs::create_dir_all(dir).map_err(AppError::storage)?;
        restrict(dir, 0o700)?;
        let tmp = self.path.with_extension("json.tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&tmp).map_err(AppError::storage)?;
        // An old temporary file keeps its mode; make sure of it.
        restrict(&tmp, 0o600)?;
        let json = serde_json::to_vec_pretty(map).map_err(AppError::storage)?;
        file.write_all(&json).map_err(AppError::storage)?;
        file.sync_all().map_err(AppError::storage)?;
        std::fs::rename(&tmp, &self.path).map_err(AppError::storage)
    }
}

/// Sets Unix permission bits; does nothing elsewhere.
fn restrict(path: &Path, mode: u32) -> AppResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(AppError::storage)?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

impl SecretStore for FileSecrets {
    fn load(&self, id: ConnectionId) -> AppResult<Secrets> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let map = self.read_all()?;
        let mut secrets = Secrets::default();
        if let Some(entry) = map.get(&id.to_string()) {
            for (field, slot) in FIELDS.into_iter().zip(fields(&mut secrets)) {
                *slot = entry.get(field).and_then(|v| v.as_str()).map(str::to_owned);
            }
        }
        Ok(secrets)
    }

    fn save(&self, id: ConnectionId, secrets: &Secrets) -> AppResult<()> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut map = self.read_all()?;
        let mut secrets = secrets.clone();
        let entry: SecretMap = FIELDS
            .into_iter()
            .zip(fields(&mut secrets))
            .filter_map(|(field, slot)| Some((field.to_owned(), slot.take()?.into())))
            .collect();
        let had = if entry.is_empty() {
            map.remove(&id.to_string()).is_some()
        } else {
            map.insert(id.to_string(), entry.into());
            true
        };
        if had { self.write_all(&map) } else { Ok(()) }
    }
}

/// Process-memory secrets: for tests, and for sessions where the user chose
/// not to save passwords.
#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<ConnectionId, Secrets>>);

impl SecretStore for MemorySecrets {
    fn load(&self, id: ConnectionId) -> AppResult<Secrets> {
        Ok(self
            .0
            .lock()
            .expect("poisoned")
            .get(&id)
            .cloned()
            .unwrap_or_default())
    }

    fn save(&self, id: ConnectionId, secrets: &Secrets) -> AppResult<()> {
        let mut map = self.0.lock().expect("poisoned");
        if *secrets == Secrets::default() {
            map.remove(&id);
        } else {
            map.insert(id, secrets.clone());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use savoia_core::Engine;

    use super::*;

    fn config(name: &str) -> ConnectionConfig {
        let mut c = ConnectionConfig::new(Engine::Postgres);
        c.name = name.into();
        c.user = "u".into();
        c
    }

    #[test]
    fn save_list_update_delete() {
        let store = ConnectionStore::open_in_memory().unwrap();
        let mut b = config("beta");
        let a = config("Alpha");
        store.save(&b).unwrap();
        store.save(&a).unwrap();

        let names: Vec<_> = store
            .list()
            .unwrap()
            .into_iter()
            .map(|c| c.config.name)
            .collect();
        assert_eq!(names, ["Alpha", "beta"], "sorted case-insensitively");

        b.port = 6543;
        store.save(&b).unwrap();
        assert_eq!(store.get(b.id).unwrap().unwrap().config.port, 6543);
        assert_eq!(store.list().unwrap().len(), 2, "update, not insert");

        store.delete(a.id).unwrap();
        assert!(store.get(a.id).unwrap().is_none());
    }

    #[test]
    fn recent_orders_by_last_use() {
        let store = ConnectionStore::open_in_memory().unwrap();
        let (a, b, c) = (config("a"), config("b"), config("c"));
        for x in [&a, &b, &c] {
            store.save(x).unwrap();
        }
        let t0 = UNIX_EPOCH + Duration::from_secs(1_000);
        store.touch(a.id, t0).unwrap();
        store.touch(b.id, t0 + Duration::from_secs(5)).unwrap();

        let recent = store.recent(10).unwrap();
        let ids: Vec<_> = recent.iter().map(|c| c.config.id).collect();
        assert_eq!(ids, [b.id, a.id], "never-used `c` is not recent");
        assert_eq!(recent[1].last_used_at, Some(t0));
    }

    #[test]
    fn reopening_keeps_data_and_does_not_rerun_migrations() {
        let dir = std::env::temp_dir().join(format!("savoia-store-{}", ConnectionId::new()));
        let path = dir.join("db.sqlite");
        let c = config("persisted");
        ConnectionStore::open(&path).unwrap().save(&c).unwrap();
        let reopened = ConnectionStore::open(&path).unwrap();
        assert_eq!(reopened.get(c.id).unwrap().unwrap().config, c);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn run(connection: ConnectionId, sql: &str) -> HistoryEntry {
        HistoryEntry {
            connection,
            sql: sql.into(),
            ran_at: UNIX_EPOCH + Duration::from_secs(1_000),
            duration: Duration::from_millis(12),
            rows: 3,
            error: None,
        }
    }

    #[test]
    fn settings_are_set_replaced_and_removed() {
        let store = ConnectionStore::open_in_memory().unwrap();
        assert_eq!(store.setting("tools").unwrap(), None);
        store.set_setting("tools", Some("/opt/pg/bin")).unwrap();
        store.set_setting("tools", Some("/usr/local/bin")).unwrap();
        assert_eq!(
            store.setting("tools").unwrap().as_deref(),
            Some("/usr/local/bin")
        );
        store.set_setting("tools", None).unwrap();
        assert_eq!(store.setting("tools").unwrap(), None);
    }

    #[test]
    fn history_searches_newest_first_and_goes_with_its_connection() {
        let store = ConnectionStore::open_in_memory().unwrap();
        let (a, b) = (config("a"), config("b"));
        store.save(&a).unwrap();
        store.save(&b).unwrap();
        store.record(&run(a.id, "SELECT * FROM orders")).unwrap();
        store
            .record(&HistoryEntry {
                error: Some("boom".into()),
                ..run(b.id, "select 100% FROM Orders_x")
            })
            .unwrap();
        store.record(&run(a.id, "DELETE FROM users")).unwrap();

        let sql = |search: &str| -> Vec<String> {
            store
                .history(search, 10)
                .unwrap()
                .into_iter()
                .map(|e| e.sql)
                .collect()
        };
        assert_eq!(
            sql(""),
            [
                "DELETE FROM users",
                "select 100% FROM Orders_x",
                "SELECT * FROM orders"
            ]
        );
        assert_eq!(
            sql("ORDERS"),
            ["select 100% FROM Orders_x", "SELECT * FROM orders"]
        );
        assert_eq!(sql("100%"), ["select 100% FROM Orders_x"]);
        assert_eq!(sql("s_x"), ["select 100% FROM Orders_x"], "_ is literal");
        assert_eq!(
            store.history("", 1).unwrap()[0],
            run(a.id, "DELETE FROM users")
        );

        store.delete(b.id).unwrap();
        assert_eq!(sql("100"), Vec::<String>::new());
    }

    #[test]
    fn memory_secrets_roundtrip_and_clear() {
        let store = MemorySecrets::default();
        let id = ConnectionId::new();
        let secrets = Secrets {
            password: Some("pw".into()),
            ..Default::default()
        };
        store.save(id, &secrets).unwrap();
        assert_eq!(store.load(id).unwrap(), secrets);
        store.delete(id).unwrap();
        assert_eq!(store.load(id).unwrap(), Secrets::default());
    }

    #[test]
    fn file_secrets_roundtrip_private_to_the_user() {
        let dir = std::env::temp_dir().join(format!("savoia-secrets-{}", ConnectionId::new()));
        let path = dir.join("secrets.json");
        let store = FileSecrets::at(&path);
        let (a, b) = (ConnectionId::new(), ConnectionId::new());
        assert_eq!(store.load(a).unwrap(), Secrets::default(), "no file yet");

        let secrets = Secrets {
            password: Some("pw".into()),
            ssh_password: None,
            ssh_key_passphrase: Some("kp".into()),
        };
        store.save(a, &secrets).unwrap();
        store
            .save(
                b,
                &Secrets {
                    password: Some("other".into()),
                    ..Secrets::default()
                },
            )
            .unwrap();
        // A fresh instance reads what the first one wrote.
        assert_eq!(FileSecrets::at(&path).load(a).unwrap(), secrets);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(&dir), 0o700);
        }

        store.delete(a).unwrap();
        assert_eq!(store.load(a).unwrap(), Secrets::default());
        assert_eq!(store.load(b).unwrap().password.as_deref(), Some("other"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
