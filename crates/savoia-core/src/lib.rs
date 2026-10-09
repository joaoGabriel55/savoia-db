//! Domain types shared by the drivers, storage, transfer engines and the UI.
//! This crate has no UI or database-driver dependencies.

mod catalog;
mod connection;
mod driver;
mod engine;
mod error;
mod query;

pub use catalog::{Catalog, DatabaseNode, SchemaNode, SchemaObjects, ServerInfo};
pub use connection::{
    ConnectionColor, ConnectionConfig, ConnectionId, Endpoint, Secrets, SshAuth, SshConfig,
    SslMode, parse_url,
};
pub use driver::{Connection, Driver};
pub use engine::Engine;
pub use error::{AppError, AppResult};
pub use query::{
    Cancel, CancelHandle, Cell, Closed, ColumnMeta, PAGE_LATENCY, PAGE_ROWS, QueryEvent,
    QueryHandle, QuerySender, Row, ValueKind,
};
