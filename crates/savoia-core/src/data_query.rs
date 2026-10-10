//! The query behind a data view: one base table, the columns shown, filters
//! and a sort, turned into one paged `SELECT`. See
//! `docs/adr/202610091908-add-a-no-sql-data-view-with-visual-joins-to-v1.md`.
//!
//! The base table is always aliased `t0`, so columns reached through
//! relationships can join in under their own aliases.

use crate::Engine;
use crate::sql_text::{quote_ident, quote_literal};

/// Rows per page of a data view.
pub const PAGE_SIZE: usize = 200;

/// How a filter compares a column with its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
    StartsWith,
    IsNull,
    IsNotNull,
}

impl Op {
    pub const ALL: [Op; 10] = [
        Op::Eq,
        Op::Ne,
        Op::Lt,
        Op::Le,
        Op::Gt,
        Op::Ge,
        Op::Contains,
        Op::StartsWith,
        Op::IsNull,
        Op::IsNotNull,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Op::Eq => "=",
            Op::Ne => "≠",
            Op::Lt => "<",
            Op::Le => "≤",
            Op::Gt => ">",
            Op::Ge => "≥",
            Op::Contains => "contains",
            Op::StartsWith => "starts with",
            Op::IsNull => "is NULL",
            Op::IsNotNull => "is not NULL",
        }
    }

    /// Whether the operator compares with a value.
    pub fn takes_value(self) -> bool {
        !matches!(self, Op::IsNull | Op::IsNotNull)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub column: String,
    pub op: Op,
    /// Ignored by operators that take no value.
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableQuery {
    pub engine: Engine,
    /// Schema on Postgres, database on MySQL.
    pub container: String,
    pub table: String,
    /// Base-table columns shown, in order.
    pub columns: Vec<String>,
    /// All must hold.
    pub filters: Vec<Filter>,
    /// Column and whether it sorts descending.
    pub sort: Option<(String, bool)>,
    /// The row's key (primary or unique), for a stable order between
    /// pages. Empty when the table has none.
    pub key: Vec<String>,
}

impl TableQuery {
    fn q(&self, ident: &str) -> String {
        quote_ident(self.engine, ident)
    }

    fn base(&self, column: &str) -> String {
        format!("t0.{}", self.q(column))
    }

    fn from(&self) -> String {
        format!("{}.{} AS t0", self.q(&self.container), self.q(&self.table))
    }

    fn condition(&self, filter: &Filter) -> String {
        let column = self.base(&filter.column);
        let value = || quote_literal(self.engine, &filter.value);
        let like = |pattern: String| {
            let text = match self.engine {
                Engine::Postgres => format!("CAST({column} AS text) ILIKE"),
                Engine::Mysql => format!("CAST({column} AS CHAR) LIKE"),
            };
            format!("{text} {}", quote_literal(self.engine, &pattern))
        };
        let escaped = filter
            .value
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        match filter.op {
            Op::Eq => format!("{column} = {}", value()),
            Op::Ne => format!("{column} <> {}", value()),
            Op::Lt => format!("{column} < {}", value()),
            Op::Le => format!("{column} <= {}", value()),
            Op::Gt => format!("{column} > {}", value()),
            Op::Ge => format!("{column} >= {}", value()),
            Op::Contains => like(format!("%{escaped}%")),
            Op::StartsWith => like(format!("{escaped}%")),
            Op::IsNull => format!("{column} IS NULL"),
            Op::IsNotNull => format!("{column} IS NOT NULL"),
        }
    }

    /// The `WHERE` clause, or nothing.
    fn where_clause(&self) -> String {
        if self.filters.is_empty() {
            return String::new();
        }
        let conditions: Vec<String> = self.filters.iter().map(|f| self.condition(f)).collect();
        format!(" WHERE {}", conditions.join(" AND "))
    }

    /// The sort, then the key, so pages never overlap or skip rows.
    fn order_clause(&self) -> String {
        let mut terms = Vec::new();
        if let Some((column, descending)) = &self.sort {
            terms.push(format!(
                "{}{}",
                self.base(column),
                if *descending { " DESC" } else { "" }
            ));
        }
        for column in &self.key {
            if self
                .sort
                .as_ref()
                .is_none_or(|(sorted, _)| sorted != column)
            {
                terms.push(self.base(column));
            }
        }
        if terms.is_empty() {
            String::new()
        } else {
            format!(" ORDER BY {}", terms.join(", "))
        }
    }

    /// The statement without paging, as shown to the user.
    pub fn sql(&self) -> String {
        let columns: Vec<String> = self.columns.iter().map(|c| self.base(c)).collect();
        format!(
            "SELECT {} FROM {}{}{}",
            columns.join(", "),
            self.from(),
            self.where_clause(),
            self.order_clause()
        )
    }

    /// One page: `limit` rows after the first `offset`. Asks for one row
    /// more than `limit`, so the caller can tell whether another page exists.
    pub fn page_sql(&self, offset: usize, limit: usize) -> String {
        format!("{} LIMIT {} OFFSET {offset}", self.sql(), limit + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(engine: Engine) -> TableQuery {
        TableQuery {
            engine,
            container: "shop".into(),
            table: "orders".into(),
            columns: vec!["id".into(), "note".into()],
            filters: Vec::new(),
            sort: None,
            key: vec!["id".into()],
        }
    }

    #[test]
    fn pages_in_key_order() {
        assert_eq!(
            query(Engine::Postgres).page_sql(200, 200),
            "SELECT t0.\"id\", t0.\"note\" FROM \"shop\".\"orders\" AS t0 \
             ORDER BY t0.\"id\" LIMIT 201 OFFSET 200"
        );
    }

    #[test]
    fn sort_then_key_without_repeating_it() {
        let mut q = query(Engine::Mysql);
        q.sort = Some(("note".into(), true));
        assert!(
            q.sql().ends_with("ORDER BY t0.`note` DESC, t0.`id`"),
            "{}",
            q.sql()
        );
        q.sort = Some(("id".into(), true));
        assert!(q.sql().ends_with("ORDER BY t0.`id` DESC"), "{}", q.sql());
        q.key.clear();
        q.sort = None;
        assert!(!q.sql().contains("ORDER BY"));
    }

    #[test]
    fn filters_quote_values_and_escape_patterns() {
        let mut q = query(Engine::Postgres);
        q.filters = vec![
            Filter {
                column: "id".into(),
                op: Op::Ge,
                value: "10".into(),
            },
            Filter {
                column: "note".into(),
                op: Op::Contains,
                value: "50%_o'k".into(),
            },
            Filter {
                column: "note".into(),
                op: Op::IsNotNull,
                value: "ignored".into(),
            },
        ];
        assert_eq!(
            q.sql(),
            "SELECT t0.\"id\", t0.\"note\" FROM \"shop\".\"orders\" AS t0 \
             WHERE t0.\"id\" >= '10' AND CAST(t0.\"note\" AS text) ILIKE '%50\\%\\_o''k%' \
             AND t0.\"note\" IS NOT NULL ORDER BY t0.\"id\""
        );
        q.engine = Engine::Mysql;
        q.filters.truncate(2);
        q.filters[1].op = Op::StartsWith;
        assert!(
            q.sql()
                .contains("CAST(t0.`note` AS CHAR) LIKE '50\\\\%\\\\_o''k%'"),
            "{}",
            q.sql()
        );
    }
}
