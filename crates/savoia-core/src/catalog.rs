//! What the explorer shows after connecting.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    /// e.g. "PostgreSQL 17.2" or "MySQL 8.4.3".
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaObjects {
    pub tables: Vec<String>,
    pub views: Vec<String>,
    pub functions: Vec<String>,
    pub sequences: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaNode {
    pub name: String,
    pub objects: SchemaObjects,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseNode {
    pub name: String,
    /// The database the session is connected to (Postgres) or the default one (MySQL).
    pub is_current: bool,
    /// Loaded schemas. Empty when not loaded yet (other Postgres databases need
    /// their own session). For MySQL there is exactly one schema, named like
    /// the database, when loaded.
    pub schemas: Vec<SchemaNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    pub server: ServerInfo,
    pub databases: Vec<DatabaseNode>,
}
