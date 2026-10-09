use serde::Serialize;

/// Application error. The variant says which layer failed, so the UI can
/// pick wording and a recovery hint; `message` is the human-readable detail.
#[derive(Debug, Clone, thiserror::Error, Serialize)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum AppError {
    #[error("{message}")]
    InvalidInput { message: String },
    #[error("could not connect: {message}")]
    Connect { message: String },
    #[error("authentication failed: {message}")]
    Auth { message: String },
    #[error("SSH tunnel: {message}")]
    Ssh { message: String },
    /// The SSH server's host key isn't in known_hosts. The UI asks the user to
    /// trust `fingerprint`, then retries with trust allowed.
    #[error("unknown SSH host key for {host} ({fingerprint})")]
    UnknownHostKey { host: String, fingerprint: String },
    #[error("{message}")]
    Query { message: String },
    #[error("local storage: {message}")]
    Storage { message: String },
    #[error("{message}")]
    Internal { message: String },
}

impl AppError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
        }
    }
    pub fn connect(message: impl ToString) -> Self {
        Self::Connect {
            message: message.to_string(),
        }
    }
    pub fn auth(message: impl ToString) -> Self {
        Self::Auth {
            message: message.to_string(),
        }
    }
    pub fn ssh(message: impl ToString) -> Self {
        Self::Ssh {
            message: message.to_string(),
        }
    }
    pub fn query(message: impl ToString) -> Self {
        Self::Query {
            message: message.to_string(),
        }
    }
    pub fn storage(message: impl ToString) -> Self {
        Self::Storage {
            message: message.to_string(),
        }
    }
    pub fn internal(message: impl ToString) -> Self {
        Self::Internal {
            message: message.to_string(),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_tagged_by_code() {
        let err = AppError::auth("bad password");
        assert_eq!(
            serde_json::to_string(&err).unwrap(),
            r#"{"code":"auth","message":"bad password"}"#
        );
        assert_eq!(err.to_string(), "authentication failed: bad password");
    }
}
