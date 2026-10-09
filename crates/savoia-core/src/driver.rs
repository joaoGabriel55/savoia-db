use async_trait::async_trait;

use crate::{
    AppResult, Catalog, Endpoint, Engine, QueryHandle, SchemaObjects, Secrets, ServerInfo,
    TableInfo,
};

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

    /// Databases, and the schemas with object counts of each database this
    /// connection can read. Object names come from [`Self::list_objects`].
    async fn catalog(&self) -> AppResult<Catalog>;

    /// The object names of one schema.
    async fn list_objects(&self, database: &str, schema: &str) -> AppResult<SchemaObjects>;

    /// Columns, keys and indexes of one table or view.
    async fn describe_table(
        &self,
        database: &str,
        schema: &str,
        table: &str,
    ) -> AppResult<TableInfo>;

    /// [`Self::describe_table`] for every table and view of a schema, by name.
    async fn describe_schema(&self, database: &str, schema: &str) -> AppResult<Vec<TableInfo>>;

    /// Starts running `sql`, which may hold several statements, and returns
    /// once results begin streaming. Errors from the SQL itself arrive as
    /// events, after any results that came before them; `Err` here means the
    /// execution couldn't start.
    async fn execute(&self, sql: String) -> AppResult<QueryHandle>;

    async fn close(&self);
}
