//! What the explorer shows after connecting. Loaded in levels, on demand: see
//! `docs/adr/202610091437-load-schema-objects-and-table-details-on-demand.md`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    /// e.g. "PostgreSQL 17.2" or "MySQL 8.4.3".
    pub version: String,
}

/// How many objects of each kind a schema holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ObjectCounts {
    pub tables: usize,
    pub views: usize,
    pub functions: usize,
    pub sequences: usize,
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
    pub counts: ObjectCounts,
    /// `None` until [`crate::Connection::list_objects`] has loaded them.
    pub objects: Option<SchemaObjects>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseNode {
    pub name: String,
    /// The database the session is connected to (Postgres) or the default one (MySQL).
    pub is_current: bool,
    /// Its schemas. Empty when the connection can't read them (other Postgres
    /// databases need their own session). For MySQL there is exactly one
    /// schema, named like the database.
    pub schemas: Vec<SchemaNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    pub server: ServerInfo,
    pub databases: Vec<DatabaseNode>,
}

impl Catalog {
    pub fn schema(&self, database: &str, schema: &str) -> Option<&SchemaNode> {
        self.databases
            .iter()
            .find(|d| d.name == database)?
            .schemas
            .iter()
            .find(|s| s.name == schema)
    }

    pub fn schema_mut(&mut self, database: &str, schema: &str) -> Option<&mut SchemaNode> {
        self.databases
            .iter_mut()
            .find(|d| d.name == database)?
            .schemas
            .iter_mut()
            .find(|s| s.name == schema)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableKind {
    Table,
    View,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    pub name: String,
    /// The type as the server prints it, e.g. `character varying(80)` or `int unsigned`.
    pub data_type: String,
    pub nullable: bool,
    /// The default expression as the server prints it.
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKey {
    pub name: String,
    pub columns: Vec<String>,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexInfo {
    pub name: String,
    /// Column names, or the expression text for expression indexes.
    pub columns: Vec<String>,
    pub unique: bool,
    pub primary: bool,
}

/// One table or view with its columns and keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableInfo {
    pub name: String,
    pub kind: TableKind,
    /// In table order.
    pub columns: Vec<ColumnInfo>,
    /// Empty when there is none.
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKey>,
    pub indexes: Vec<IndexInfo>,
}

impl TableInfo {
    pub fn new(name: impl Into<String>, kind: TableKind) -> Self {
        Self {
            name: name.into(),
            kind,
            columns: Vec::new(),
            primary_key: Vec::new(),
            foreign_keys: Vec::new(),
            indexes: Vec::new(),
        }
    }

    pub fn is_key_column(&self, column: &str) -> bool {
        self.primary_key.iter().any(|c| c == column)
    }

    pub fn is_foreign_column(&self, column: &str) -> bool {
        self.foreign_keys
            .iter()
            .any(|fk| fk.columns.iter().any(|c| c == column))
    }
}

/// Puts rows of `(table, item)` into the tables they belong to, keeping
/// order. Rows naming an unknown table are dropped.
pub fn attach<T>(tables: &mut [TableInfo], rows: Vec<(String, T)>, f: impl Fn(&mut TableInfo, T)) {
    for (table, item) in rows {
        if let Some(info) = tables.iter_mut().find(|t| t.name == table) {
            f(info, item);
        }
    }
}
