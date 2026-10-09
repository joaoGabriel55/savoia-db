use serde::{Deserialize, Serialize};

/// A database engine Savoia DB can connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Postgres,
    Mysql,
}

impl Engine {
    pub const ALL: [Engine; 2] = [Engine::Postgres, Engine::Mysql];

    pub fn default_port(self) -> u16 {
        match self {
            Engine::Postgres => 5432,
            Engine::Mysql => 3306,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Engine::Postgres => "PostgreSQL",
            Engine::Mysql => "MySQL",
        }
    }

    /// Whether objects live in schemas inside a database (Postgres), or the
    /// database *is* the schema (MySQL). The explorer skips the schema level
    /// when this is false.
    pub fn has_schemas(self) -> bool {
        matches!(self, Engine::Postgres)
    }

    pub fn url_schemes(self) -> &'static [&'static str] {
        match self {
            Engine::Postgres => &["postgres", "postgresql"],
            Engine::Mysql => &["mysql", "mariadb"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ports() {
        assert_eq!(Engine::Postgres.default_port(), 5432);
        assert_eq!(Engine::Mysql.default_port(), 3306);
    }

    #[test]
    fn serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Engine::Mysql).unwrap(), "\"mysql\"");
    }
}
