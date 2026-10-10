//! Pending data-view changes as SQL. Values are written back as quoted
//! string literals, the way the server rendered them; see
//! `docs/adr/202610091908-write-data-edits-as-generated-sql-in-one-previewed-transaction.md`.

use crate::Engine;
use crate::sql_text::{quote_ident, quote_literal};

/// A cell value: `None` is SQL `NULL`.
pub type Value = Option<String>;

/// The table changes are written to, with each column's type as the
/// server prints it (for Postgres casts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditTarget {
    pub engine: Engine,
    /// Schema on Postgres, database on MySQL.
    pub container: String,
    pub table: String,
    pub columns: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Sets `values` on the row whose key columns hold `key`.
    Update {
        key: Vec<(String, Value)>,
        values: Vec<(String, Value)>,
    },
    /// Columns left out take their defaults.
    Insert {
        values: Vec<(String, Value)>,
    },
    Delete {
        key: Vec<(String, Value)>,
    },
}

impl EditTarget {
    fn name(&self) -> String {
        format!(
            "{}.{}",
            quote_ident(self.engine, &self.container),
            quote_ident(self.engine, &self.table)
        )
    }

    fn data_type(&self, column: &str) -> Option<&str> {
        self.columns
            .iter()
            .find(|(name, _)| name == column)
            .map(|(_, t)| t.as_str())
    }

    /// `value` for `column`: `NULL`, or a literal (cast on Postgres unless
    /// the column is text).
    fn value(&self, column: &str, value: &Value) -> String {
        let Some(text) = value else {
            return "NULL".into();
        };
        let literal = quote_literal(self.engine, text);
        match (self.engine, self.data_type(column)) {
            (Engine::Postgres, Some(t)) if !is_text_type(t) => format!("{literal}::{t}"),
            _ => literal,
        }
    }

    fn key_clause(&self, key: &[(String, Value)]) -> String {
        key.iter()
            .map(|(column, value)| {
                let column_sql = quote_ident(self.engine, column);
                match value {
                    None => format!("{column_sql} IS NULL"),
                    Some(_) => format!("{column_sql} = {}", self.value(column, value)),
                }
            })
            .collect::<Vec<_>>()
            .join(" AND ")
    }

    /// The statement for one change, without a trailing `;`.
    pub fn statement(&self, change: &Change) -> String {
        let q = |c: &str| quote_ident(self.engine, c);
        match change {
            Change::Update { key, values } => {
                let sets: Vec<String> = values
                    .iter()
                    .map(|(c, v)| format!("{} = {}", q(c), self.value(c, v)))
                    .collect();
                format!(
                    "UPDATE {} SET {} WHERE {}",
                    self.name(),
                    sets.join(", "),
                    self.key_clause(key)
                )
            }
            Change::Insert { values } if values.is_empty() => match self.engine {
                Engine::Postgres => format!("INSERT INTO {} DEFAULT VALUES", self.name()),
                Engine::Mysql => format!("INSERT INTO {} () VALUES ()", self.name()),
            },
            Change::Insert { values } => {
                let columns: Vec<String> = values.iter().map(|(c, _)| q(c)).collect();
                let literals: Vec<String> = values.iter().map(|(c, v)| self.value(c, v)).collect();
                format!(
                    "INSERT INTO {} ({}) VALUES ({})",
                    self.name(),
                    columns.join(", "),
                    literals.join(", ")
                )
            }
            Change::Delete { key } => {
                format!("DELETE FROM {} WHERE {}", self.name(), self.key_clause(key))
            }
        }
    }

    /// The whole script as previewed: the statements in a transaction.
    pub fn script(&self, changes: &[Change]) -> String {
        let begin = match self.engine {
            Engine::Postgres => "BEGIN;",
            Engine::Mysql => "START TRANSACTION;",
        };
        let mut lines = vec![begin.to_owned()];
        lines.extend(changes.iter().map(|c| format!("{};", self.statement(c))));
        lines.push("COMMIT;".into());
        lines.join("\n")
    }
}

/// Types a plain string literal already fits, so no cast is written.
fn is_text_type(data_type: &str) -> bool {
    let t = data_type.to_ascii_lowercase();
    t == "text" || t.starts_with("character") || t.starts_with("varchar") || t.starts_with("char")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(engine: Engine) -> EditTarget {
        EditTarget {
            engine,
            container: "shop".into(),
            table: "orders".into(),
            columns: vec![
                ("id".into(), "integer".into()),
                ("note".into(), "character varying(80)".into()),
                ("meta".into(), "jsonb".into()),
            ],
        }
    }

    fn key(id: &str) -> Vec<(String, Value)> {
        vec![("id".into(), Some(id.into()))]
    }

    #[test]
    fn postgres_casts_non_text_values_and_keeps_null() {
        let t = target(Engine::Postgres);
        assert_eq!(
            t.statement(&Change::Update {
                key: key("7"),
                values: vec![("note".into(), Some("it's".into())), ("meta".into(), None),],
            }),
            "UPDATE \"shop\".\"orders\" SET \"note\" = 'it''s', \"meta\" = NULL \
             WHERE \"id\" = '7'::integer"
        );
        assert_eq!(
            t.statement(&Change::Update {
                key: key("7"),
                values: vec![("note".into(), Some(String::new()))],
            }),
            "UPDATE \"shop\".\"orders\" SET \"note\" = '' WHERE \"id\" = '7'::integer",
            "empty string is not NULL"
        );
    }

    #[test]
    fn inserts_and_deletes() {
        let t = target(Engine::Mysql);
        assert_eq!(
            t.statement(&Change::Insert {
                values: vec![("note".into(), Some("a\\b".into()))],
            }),
            "INSERT INTO `shop`.`orders` (`note`) VALUES ('a\\\\b')"
        );
        assert_eq!(
            t.statement(&Change::Insert { values: Vec::new() }),
            "INSERT INTO `shop`.`orders` () VALUES ()"
        );
        assert_eq!(
            t.statement(&Change::Delete { key: key("7") }),
            "DELETE FROM `shop`.`orders` WHERE `id` = '7'"
        );
        assert_eq!(
            target(Engine::Postgres).statement(&Change::Insert { values: Vec::new() }),
            "INSERT INTO \"shop\".\"orders\" DEFAULT VALUES"
        );
    }

    #[test]
    fn script_wraps_in_a_transaction() {
        let script = target(Engine::Mysql).script(&[Change::Delete { key: key("1") }]);
        assert_eq!(
            script,
            "START TRANSACTION;\nDELETE FROM `shop`.`orders` WHERE `id` = '1';\nCOMMIT;"
        );
    }
}
