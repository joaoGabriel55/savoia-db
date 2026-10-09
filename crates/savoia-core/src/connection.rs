//! Saved connection settings, URL import, and the resolved endpoint a driver
//! connects to.

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppResult, Engine};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(pub Uuid);

impl ConnectionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Tag color shown on the explorer node, console tabs and status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionColor {
    Red,
    Orange,
    Yellow,
    Green,
    Cyan,
    Blue,
    Purple,
    Pink,
}

impl ConnectionColor {
    pub const ALL: [ConnectionColor; 8] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Cyan,
        Self::Blue,
        Self::Purple,
        Self::Pink,
    ];

    pub fn rgb(self) -> u32 {
        match self {
            Self::Red => 0xEF5B5B,
            Self::Orange => 0xF59E42,
            Self::Yellow => 0xF2CC4B,
            Self::Green => 0x5BC97A,
            Self::Cyan => 0x4FC3E8,
            Self::Blue => 0x5B8DEF,
            Self::Purple => 0x9B6BEF,
            Self::Pink => 0xEF6BC4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SslMode {
    Disable,
    /// Use TLS if the server offers it, without verifying the certificate.
    #[default]
    Prefer,
    /// Require TLS, without verifying the certificate.
    Require,
    /// Require TLS and verify the certificate chain and host name.
    VerifyFull,
}

impl SslMode {
    pub const ALL: [SslMode; 4] = [Self::Disable, Self::Prefer, Self::Require, Self::VerifyFull];

    pub fn label(self) -> &'static str {
        match self {
            Self::Disable => "Disable",
            Self::Prefer => "Prefer",
            Self::Require => "Require",
            Self::VerifyFull => "Verify full",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().replace('_', "-").as_str() {
            "disable" | "disabled" | "false" | "0" => Some(Self::Disable),
            "prefer" | "preferred" | "allow" => Some(Self::Prefer),
            "require" | "required" | "true" | "1" => Some(Self::Require),
            "verify-full" | "verify-ca" | "verify-identity" => Some(Self::VerifyFull),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "kebab-case")]
pub enum SshAuth {
    /// Password from the secret store.
    Password,
    /// Private key file; its passphrase, if any, comes from the secret store.
    PrivateKey { path: PathBuf },
    /// Keys from the running SSH agent.
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: SshAuth,
}

/// A saved (or about to be saved) connection. Holds no secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: ConnectionId,
    pub name: String,
    pub color: Option<ConnectionColor>,
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Database to open; engine default when `None`.
    pub database: Option<String>,
    pub ssl: SslMode,
    pub ssh: Option<SshConfig>,
    pub read_only: bool,
    /// Whether passwords are saved in the user-only secrets file.
    pub save_password: bool,
}

impl ConnectionConfig {
    pub fn new(engine: Engine) -> Self {
        Self {
            id: ConnectionId::new(),
            name: String::new(),
            color: None,
            engine,
            host: "localhost".into(),
            port: engine.default_port(),
            user: String::new(),
            database: None,
            ssl: SslMode::default(),
            ssh: None,
            read_only: false,
            save_password: true,
        }
    }

    /// Name shown in lists: the saved name, or `host:port`.
    pub fn display_name(&self) -> String {
        if self.name.trim().is_empty() {
            format!("{}:{}", self.host, self.port)
        } else {
            self.name.clone()
        }
    }

    pub fn validate(&self) -> AppResult<()> {
        if self.host.trim().is_empty() {
            return Err(AppError::invalid("Host is required"));
        }
        if self.port == 0 {
            return Err(AppError::invalid("Port must be between 1 and 65535"));
        }
        if self.user.trim().is_empty() {
            return Err(AppError::invalid("User is required"));
        }
        if let Some(ssh) = &self.ssh {
            if ssh.host.trim().is_empty() {
                return Err(AppError::invalid("SSH host is required"));
            }
            if ssh.user.trim().is_empty() {
                return Err(AppError::invalid("SSH user is required"));
            }
            if let SshAuth::PrivateKey { path } = &ssh.auth
                && path.as_os_str().is_empty()
            {
                return Err(AppError::invalid("SSH private key path is required"));
            }
        }
        Ok(())
    }

    /// The endpoint for a direct connection.
    pub fn endpoint(&self) -> Endpoint {
        Endpoint {
            engine: self.engine,
            host: self.host.clone(),
            port: self.port,
            tls_server_name: self.host.clone(),
            user: self.user.clone(),
            database: self.database.clone().filter(|db| !db.is_empty()),
            ssl: self.ssl,
            read_only: self.read_only,
        }
    }

    /// The endpoint when traffic goes through a local tunnel. TLS still
    /// verifies against the configured host name.
    pub fn endpoint_via(&self, local_host: &str, local_port: u16) -> Endpoint {
        Endpoint {
            host: local_host.into(),
            port: local_port,
            ..self.endpoint()
        }
    }
}

/// Where and how a driver connects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    /// Host name used for TLS verification (differs from `host` behind a tunnel).
    pub tls_server_name: String,
    pub user: String,
    pub database: Option<String>,
    pub ssl: SslMode,
    pub read_only: bool,
}

/// Secrets for one connection. Never serialized; `Debug` redacts values.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secrets {
    pub password: Option<String>,
    pub ssh_password: Option<String>,
    pub ssh_key_passphrase: Option<String>,
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mask = |s: &Option<String>| s.as_ref().map(|_| "***");
        f.debug_struct("Secrets")
            .field("password", &mask(&self.password))
            .field("ssh_password", &mask(&self.ssh_password))
            .field("ssh_key_passphrase", &mask(&self.ssh_key_passphrase))
            .finish()
    }
}

/// Parses `postgres://user:pass@host:5432/db?sslmode=require` (or `mysql://…`)
/// into a new, unsaved connection and its password.
pub fn parse_url(input: &str) -> AppResult<(ConnectionConfig, Secrets)> {
    let url = url::Url::parse(input.trim())
        .map_err(|e| AppError::invalid(format!("Invalid URL: {e}")))?;
    let engine = Engine::ALL
        .into_iter()
        .find(|e| e.url_schemes().contains(&url.scheme()))
        .ok_or_else(|| AppError::invalid(format!("Unsupported scheme `{}`", url.scheme())))?;

    let decode = |s: &str| {
        percent_decode(s).map_err(|_| AppError::invalid("URL contains invalid percent-encoding"))
    };

    let mut config = ConnectionConfig::new(engine);
    config.host = url
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| AppError::invalid("URL has no host"))?
        .trim_matches(['[', ']'])
        .to_owned();
    config.port = url.port().unwrap_or(engine.default_port());
    config.user = decode(url.username())?;
    let database = decode(url.path().trim_start_matches('/'))?;
    config.database = (!database.is_empty()).then_some(database);

    for (key, value) in url.query_pairs() {
        if matches!(key.as_ref(), "sslmode" | "ssl-mode" | "ssl_mode" | "ssl")
            && let Some(mode) = SslMode::parse(&value)
        {
            config.ssl = mode;
        }
    }

    let secrets = Secrets {
        password: url.password().map(decode).transpose()?,
        ..Secrets::default()
    };
    Ok((config, secrets))
}

fn percent_decode(s: &str) -> Result<String, ()> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3).ok_or(())?;
            let hex = std::str::from_utf8(hex).map_err(|_| ())?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| ())?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_postgres_url() {
        let (c, s) =
            parse_url("postgresql://app%40x:p%40ss@db.example.com:6543/shop?sslmode=verify-full")
                .unwrap();
        assert_eq!(c.engine, Engine::Postgres);
        assert_eq!(c.host, "db.example.com");
        assert_eq!(c.port, 6543);
        assert_eq!(c.user, "app@x");
        assert_eq!(c.database.as_deref(), Some("shop"));
        assert_eq!(c.ssl, SslMode::VerifyFull);
        assert_eq!(s.password.as_deref(), Some("p@ss"));
    }

    #[test]
    fn mysql_defaults() {
        let (c, s) = parse_url("mysql://root@localhost").unwrap();
        assert_eq!(c.engine, Engine::Mysql);
        assert_eq!(c.port, 3306);
        assert_eq!(c.database, None);
        assert_eq!(c.ssl, SslMode::Prefer);
        assert_eq!(s.password, None);
        assert_eq!(c.display_name(), "localhost:3306");
    }

    #[test]
    fn mysql_ssl_mode_alias() {
        let (c, _) = parse_url("mysql://u@h/db?ssl-mode=REQUIRED").unwrap();
        assert_eq!(c.ssl, SslMode::Require);
    }

    #[test]
    fn rejects_unknown_scheme_and_missing_host() {
        assert!(matches!(
            parse_url("redis://h"),
            Err(AppError::InvalidInput { .. })
        ));
        assert!(parse_url("not a url").is_err());
    }

    #[test]
    fn tunnel_endpoint_keeps_tls_name() {
        let mut c = ConnectionConfig::new(Engine::Postgres);
        c.host = "db.internal".into();
        let e = c.endpoint_via("127.0.0.1", 40001);
        assert_eq!((e.host.as_str(), e.port), ("127.0.0.1", 40001));
        assert_eq!(e.tls_server_name, "db.internal");
    }

    #[test]
    fn validate_reports_missing_fields() {
        let mut c = ConnectionConfig::new(Engine::Mysql);
        assert!(c.validate().is_err(), "user missing");
        c.user = "root".into();
        assert!(c.validate().is_ok());
        c.ssh = Some(SshConfig {
            host: "bastion".into(),
            port: 22,
            user: "".into(),
            auth: SshAuth::Agent,
        });
        assert!(c.validate().is_err(), "ssh user missing");
    }

    #[test]
    fn secrets_debug_is_redacted() {
        let s = Secrets {
            password: Some("hunter2".into()),
            ..Default::default()
        };
        assert!(!format!("{s:?}").contains("hunter2"));
    }

    #[test]
    fn config_roundtrips_through_json() {
        let mut c = ConnectionConfig::new(Engine::Postgres);
        c.ssh = Some(SshConfig {
            host: "b".into(),
            port: 22,
            user: "u".into(),
            auth: SshAuth::PrivateKey { path: "/k".into() },
        });
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<ConnectionConfig>(&json).unwrap(), c);
    }
}
