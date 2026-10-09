use async_trait::async_trait;

use crate::{AppResult, Catalog, Endpoint, Engine, QueryHandle, Secrets, ServerInfo};

/// An engine implementation. Stateless; one instance per engine.
#[async_trait]
pub trait Driver: Send + Sync {
    fn engine(&self) -> Engine;

    /// Opens a session to `endpoint`, which may be a local tunnel end rather
    /// than the configured host.
    async fn connect(
        &self,
        endpoint: &Endpoint,
        secrets: &Secrets,
    ) -> AppResult<Box<dyn Connection>>;
}

/// An open session. Implementations must be cheap to share behind an `Arc`.
#[async_trait]
pub trait Connection: Send + Sync {
    async fn server_info(&self) -> AppResult<ServerInfo>;

    /// Databases plus the schemas and objects of the current one.
    async fn catalog(&self) -> AppResult<Catalog>;

    /// Starts running `sql`, which may hold several statements, and returns
    /// once results begin streaming. Errors from the SQL itself arrive as
    /// events, after any results that came before them; `Err` here means the
    /// execution couldn't start.
    async fn execute(&self, sql: String) -> AppResult<QueryHandle>;

    async fn close(&self);
}
