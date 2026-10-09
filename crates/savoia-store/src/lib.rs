//! Local persistence: saved connections in SQLite, secrets in the OS keychain.
//! See ADR "Store connection secrets in the OS keychain".

use std::collections::HashMap;
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

/// Saved connections. Not `Sync`; keep it on one thread or behind a mutex.
pub struct ConnectionStore {
    db: rusqlite::Connection,
}

/// Schema migrations, applied in order; `PRAGMA user_version` is the index of
/// the last one applied. Append only.
const MIGRATIONS: &[&str] = &["CREATE TABLE connections (
        id           TEXT PRIMARY KEY,
        config       TEXT NOT NULL,
        created_at   INTEGER NOT NULL,
        last_used_at INTEGER
    );"];

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

    pub fn delete(&self, id: ConnectionId) -> AppResult<()> {
        self.db
            .execute("DELETE FROM connections WHERE id = ?1", [id.to_string()])
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

fn default_path() -> AppResult<PathBuf> {
    let dir = dirs::data_dir().ok_or_else(|| AppError::storage("no user data directory"))?;
    Ok(dir.join("savoia-db").join("savoia.sqlite"))
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

/// The OS credential store (macOS Keychain, Windows Credential Manager, Secret Service).
pub struct KeychainSecrets;

const SERVICE: &str = "dev.savoia.db";
const FIELDS: [&str; 3] = ["password", "ssh-password", "ssh-key-passphrase"];

fn fields(secrets: &mut Secrets) -> [&mut Option<String>; 3] {
    [
        &mut secrets.password,
        &mut secrets.ssh_password,
        &mut secrets.ssh_key_passphrase,
    ]
}

impl KeychainSecrets {
    /// Whether the platform credential store could be initialized.
    pub fn available() -> bool {
        keyring::Entry::store_status().is_ok()
    }

    fn entry(id: ConnectionId, field: &str) -> AppResult<keyring::Entry> {
        keyring::Entry::new(SERVICE, &format!("{id}/{field}")).map_err(AppError::storage)
    }
}

impl SecretStore for KeychainSecrets {
    fn load(&self, id: ConnectionId) -> AppResult<Secrets> {
        let mut secrets = Secrets::default();
        for (field, slot) in FIELDS.into_iter().zip(fields(&mut secrets)) {
            *slot = match Self::entry(id, field)?.get_password() {
                Ok(value) => Some(value),
                Err(keyring::Error::NoEntry) => None,
                Err(err) => return Err(AppError::storage(err)),
            };
        }
        Ok(secrets)
    }

    fn save(&self, id: ConnectionId, secrets: &Secrets) -> AppResult<()> {
        let mut secrets = secrets.clone();
        for (field, slot) in FIELDS.into_iter().zip(fields(&mut secrets)) {
            let entry = Self::entry(id, field)?;
            match slot {
                Some(value) => entry.set_password(value).map_err(AppError::storage)?,
                None => match entry.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => {}
                    Err(err) => return Err(AppError::storage(err)),
                },
            }
        }
        Ok(())
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

    /// Touches the real OS keychain; run manually with `--ignored`.
    #[test]
    #[ignore]
    fn keychain_roundtrip() {
        let id = ConnectionId::new();
        let secrets = Secrets {
            password: Some("pw".into()),
            ssh_password: None,
            ssh_key_passphrase: Some("kp".into()),
        };
        KeychainSecrets.save(id, &secrets).unwrap();
        assert_eq!(KeychainSecrets.load(id).unwrap(), secrets);
        KeychainSecrets.delete(id).unwrap();
        assert_eq!(KeychainSecrets.load(id).unwrap(), Secrets::default());
    }
}
