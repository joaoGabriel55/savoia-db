//! The query behind a data view: one base table, the columns shown, filters
//! and a sort, turned into one paged `SELECT`. See
//! `docs/adr/202610091908-add-a-no-sql-data-view-with-visual-joins-to-v1.md`.
//!
//! The base table is always aliased `t0`, so columns reached through
//! relationships can join in under their own aliases.

use crate::sql_text::{quote_ident, quote_literal};
use crate::{Engine, ForeignKey};

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

/// One many-to-one step: from the current table's `columns` to the row of
/// `table` whose `ref_columns` match. Becomes a `LEFT JOIN`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hop {
    pub columns: Vec<String>,
    pub schema: String,
    pub table: String,
    pub ref_columns: Vec<String>,
}

impl Hop {
    /// Following a foreign key of the current table.
    pub fn from_fk(fk: &ForeignKey) -> Self {
        Self {
            columns: fk.columns.clone(),
            schema: fk.ref_schema.clone(),
            table: fk.ref_table.clone(),
            ref_columns: fk.ref_columns.clone(),
        }
    }

    /// How the step reads in a label: the FK column without its `_id`
    /// (`customer_id` → `customer`), else the target table.
    pub fn label(&self) -> String {
        match self.columns.as_slice() {
            [column] => column
                .strip_suffix("_id")
                .or_else(|| column.strip_suffix("_ID"))
                .filter(|s| !s.is_empty())
                .unwrap_or(&self.table)
                .to_owned(),
            _ => self.table.clone(),
        }
    }
}

/// The rows of `table` whose `columns` hold the base row's `ref_columns`:
/// a one-to-many relation, read through a correlated subquery.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Children {
    pub schema: String,
    pub table: String,
    pub columns: Vec<String>,
    pub ref_columns: Vec<String>,
}

impl Children {
    /// The child side of `fk`, which `schema.table` declares.
    pub fn from_fk(schema: &str, table: &str, fk: &ForeignKey) -> Self {
        Self {
            schema: schema.to_owned(),
            table: table.to_owned(),
            columns: fk.columns.clone(),
            ref_columns: fk.ref_columns.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Agg {
    Count,
    Sum,
    Min,
    Max,
    Avg,
    /// Distinct values, comma-separated.
    List,
}

impl Agg {
    pub fn label(self) -> &'static str {
        match self {
            Agg::Count => "count",
            Agg::Sum => "sum",
            Agg::Min => "min",
            Agg::Max => "max",
            Agg::Avg => "avg",
            Agg::List => "list",
        }
    }
}

/// Where a shown column's values come from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Source {
    /// A column of the base table.
    Base(String),
    /// A column of a table reached through many-to-one steps.
    Lookup { path: Vec<Hop>, column: String },
    /// An aggregate over child rows. `column` is `None` for `Count`.
    Summary {
        children: Children,
        agg: Agg,
        column: Option<String>,
    },
}

impl Source {
    /// The column header: `name`, `customer › name`, `order_items: count`.
    pub fn label(&self) -> String {
        match self {
            Source::Base(column) => column.clone(),
            Source::Lookup { path, column } => {
                let mut parts: Vec<String> = path.iter().map(Hop::label).collect();
                parts.push(column.clone());
                parts.join(" › ")
            }
            Source::Summary {
                children,
                agg,
                column,
            } => match column {
                Some(column) => format!("{}: {}({column})", children.table, agg.label()),
                None => format!("{}: {}", children.table, agg.label()),
            },
        }
    }

    pub fn base(&self) -> Option<&str> {
        match self {
            Source::Base(column) => Some(column),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub column: Source,
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
    /// Columns shown, in order.
    pub columns: Vec<Source>,
    /// All must hold.
    pub filters: Vec<Filter>,
    /// Column and whether it sorts descending.
    pub sort: Option<(Source, bool)>,
    /// The row's key (primary or unique), for a stable order between
    /// pages. Empty when the table has none.
    pub key: Vec<String>,
    /// When set, the view shows groups instead of rows.
    pub summary: Option<Summary>,
}

/// Rows grouped by some columns, with aggregates per group. Outputs are
/// the group columns, then the aggregates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub by: Vec<Source>,
    /// `None` aggregates whole rows (`count(*)`).
    pub aggregates: Vec<(Agg, Option<Source>)>,
    /// Output position (0-based) and whether it sorts descending.
    pub sort: Option<(usize, bool)>,
}

impl Summary {
    /// The output headers: group columns, then `count`, `sum(total)`, ….
    pub fn labels(&self) -> Vec<String> {
        self.by
            .iter()
            .map(Source::label)
            .chain(self.aggregates.iter().map(|(agg, source)| match source {
                Some(source) => format!("{}({})", agg.label(), source.label()),
                None => agg.label().to_owned(),
            }))
            .collect()
    }
}

/// The alias of each joined path prefix: `t1`, `t2`, … in first-use order.
struct Joins(Vec<(Vec<Hop>, String)>);

impl Joins {
    fn alias(&self, path: &[Hop]) -> &str {
        if path.is_empty() {
            return "t0";
        }
        self.0
            .iter()
            .find(|(p, _)| p.as_slice() == path)
            .map_or("t0", |(_, alias)| alias.as_str())
    }
}

impl TableQuery {
    fn q(&self, ident: &str) -> String {
        quote_ident(self.engine, ident)
    }

    /// Every source the statement uses: columns (or the summary's),
    /// filters, sort.
    fn sources(&self) -> Box<dyn Iterator<Item = &Source> + '_> {
        let filters = self.filters.iter().map(|f| &f.column);
        match &self.summary {
            Some(summary) => Box::new(
                summary
                    .by
                    .iter()
                    .chain(summary.aggregates.iter().filter_map(|(_, s)| s.as_ref()))
                    .chain(filters),
            ),
            None => Box::new(
                self.columns
                    .iter()
                    .chain(filters)
                    .chain(self.sort.iter().map(|(s, _)| s)),
            ),
        }
    }

    /// `agg` over `value`: count, sum, …, or the engine's string list.
    fn aggregate(&self, agg: Agg, value: &str) -> String {
        match (agg, self.engine) {
            (Agg::Count, _) if value.is_empty() => "count(*)".to_string(),
            (Agg::Count, _) => format!("count({value})"),
            (Agg::List, Engine::Postgres) => {
                format!("string_agg(DISTINCT CAST({value} AS text), ', ')")
            }
            (Agg::List, Engine::Mysql) => format!("GROUP_CONCAT(DISTINCT {value} SEPARATOR ', ')"),
            (agg, _) => format!("{}({value})", agg.label()),
        }
    }

    fn joins(&self) -> Joins {
        let mut joins: Vec<(Vec<Hop>, String)> = Vec::new();
        for source in self.sources() {
            if let Source::Lookup { path, .. } = source {
                for len in 1..=path.len() {
                    let prefix = &path[..len];
                    if !joins.iter().any(|(p, _)| p.as_slice() == prefix) {
                        let alias = format!("t{}", joins.len() + 1);
                        joins.push((prefix.to_vec(), alias));
                    }
                }
            }
        }
        Joins(joins)
    }

    fn table_name(&self, schema: &str, table: &str) -> String {
        format!("{}.{}", self.q(schema), self.q(table))
    }

    fn pairs(
        &self,
        left: &str,
        left_cols: &[String],
        right: &str,
        right_cols: &[String],
    ) -> String {
        left_cols
            .iter()
            .zip(right_cols)
            .map(|(l, r)| format!("{left}.{} = {right}.{}", self.q(l), self.q(r)))
            .collect::<Vec<_>>()
            .join(" AND ")
    }

    fn expr(&self, source: &Source, joins: &Joins) -> String {
        match source {
            Source::Base(column) => format!("t0.{}", self.q(column)),
            Source::Lookup { path, column } => {
                format!("{}.{}", joins.alias(path), self.q(column))
            }
            Source::Summary {
                children,
                agg,
                column,
            } => {
                let value = column
                    .as_deref()
                    .map(|c| format!("c.{}", self.q(c)))
                    .unwrap_or_default();
                let aggregate = match agg {
                    Agg::Count => "count(*)".to_string(),
                    agg => self.aggregate(*agg, &value),
                };
                format!(
                    "(SELECT {aggregate} FROM {} AS c WHERE {})",
                    self.table_name(&children.schema, &children.table),
                    self.pairs("c", &children.columns, "t0", &children.ref_columns)
                )
            }
        }
    }

    fn from(&self, joins: &Joins) -> String {
        let mut from = format!("{} AS t0", self.table_name(&self.container, &self.table));
        for (path, alias) in &joins.0 {
            let hop = path.last().expect("joined paths are not empty");
            let parent = joins.alias(&path[..path.len() - 1]);
            from.push_str(&format!(
                " LEFT JOIN {} AS {alias} ON {}",
                self.table_name(&hop.schema, &hop.table),
                self.pairs(alias, &hop.ref_columns, parent, &hop.columns)
            ));
        }
        from
    }

    fn condition(&self, filter: &Filter, joins: &Joins) -> String {
        let column = self.expr(&filter.column, joins);
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
    fn where_clause(&self, joins: &Joins) -> String {
        if self.filters.is_empty() {
            return String::new();
        }
        let conditions: Vec<String> = self
            .filters
            .iter()
            .map(|f| self.condition(f, joins))
            .collect();
        format!(" WHERE {}", conditions.join(" AND "))
    }

    /// The sort, then the key, so pages never overlap or skip rows.
    fn order_clause(&self, joins: &Joins) -> String {
        let mut terms = Vec::new();
        if let Some((source, descending)) = &self.sort {
            terms.push(format!(
                "{}{}",
                self.expr(source, joins),
                if *descending { " DESC" } else { "" }
            ));
        }
        for column in &self.key {
            let sorted = self
                .sort
                .as_ref()
                .is_some_and(|(s, _)| s.base() == Some(column));
            if !sorted {
                terms.push(format!("t0.{}", self.q(column)));
            }
        }
        if terms.is_empty() {
            String::new()
        } else {
            format!(" ORDER BY {}", terms.join(", "))
        }
    }

    /// The statement without paging, as shown to the user. Columns from
    /// other tables are named by their labels.
    pub fn sql(&self) -> String {
        let joins = self.joins();
        if let Some(summary) = &self.summary {
            return self.summary_sql(summary, &joins);
        }
        let columns: Vec<String> = self
            .columns
            .iter()
            .map(|source| match source {
                Source::Base(_) => self.expr(source, &joins),
                _ => format!(
                    "{} AS {}",
                    self.expr(source, &joins),
                    self.q(&source.label())
                ),
            })
            .collect();
        format!(
            "SELECT {} FROM {}{}{}",
            columns.join(", "),
            self.from(&joins),
            self.where_clause(&joins),
            self.order_clause(&joins)
        )
    }

    fn summary_sql(&self, summary: &Summary, joins: &Joins) -> String {
        let by: Vec<String> = summary.by.iter().map(|s| self.expr(s, joins)).collect();
        let labels = summary.labels();
        let outputs: Vec<String> = by
            .iter()
            .cloned()
            .chain(summary.aggregates.iter().map(|(agg, source)| {
                let value = source
                    .as_ref()
                    .map(|s| self.expr(s, joins))
                    .unwrap_or_default();
                self.aggregate(*agg, &value)
            }))
            .zip(&labels)
            .map(|(expr, label)| format!("{expr} AS {}", self.q(label)))
            .collect();
        let group = if by.is_empty() {
            String::new()
        } else {
            format!(" GROUP BY {}", by.join(", "))
        };
        // By position: the chosen output, then the groups for a stable order.
        let mut order: Vec<String> = Vec::new();
        if let Some((ix, descending)) = summary.sort {
            order.push(format!(
                "{}{}",
                ix + 1,
                if descending { " DESC" } else { "" }
            ));
        }
        for ix in 0..by.len() {
            if summary.sort.is_none_or(|(sorted, _)| sorted != ix) {
                order.push((ix + 1).to_string());
            }
        }
        let order = if order.is_empty() {
            String::new()
        } else {
            format!(" ORDER BY {}", order.join(", "))
        };
        format!(
            "SELECT {} FROM {}{}{group}{order}",
            outputs.join(", "),
            self.from(joins),
            self.where_clause(joins),
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
            columns: vec![Source::Base("id".into()), Source::Base("note".into())],
            filters: Vec::new(),
            sort: None,
            key: vec!["id".into()],
            summary: None,
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
        q.sort = Some((Source::Base("note".into()), true));
        assert!(
            q.sql().ends_with("ORDER BY t0.`note` DESC, t0.`id`"),
            "{}",
            q.sql()
        );
        q.sort = Some((Source::Base("id".into()), true));
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
                column: Source::Base("id".into()),
                op: Op::Ge,
                value: "10".into(),
            },
            Filter {
                column: Source::Base("note".into()),
                op: Op::Contains,
                value: "50%_o'k".into(),
            },
            Filter {
                column: Source::Base("note".into()),
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

    fn customer() -> Hop {
        Hop {
            columns: vec!["customer_id".into()],
            schema: "shop".into(),
            table: "customers".into(),
            ref_columns: vec!["id".into()],
        }
    }

    fn country() -> Hop {
        Hop {
            columns: vec!["country_code".into()],
            schema: "shop".into(),
            table: "countries".into(),
            ref_columns: vec!["code".into()],
        }
    }

    #[test]
    fn lookups_join_each_path_once() {
        let mut q = query(Engine::Postgres);
        q.columns = vec![
            Source::Base("id".into()),
            Source::Lookup {
                path: vec![customer()],
                column: "name".into(),
            },
            Source::Lookup {
                path: vec![customer(), country()],
                column: "name".into(),
            },
        ];
        q.filters = vec![Filter {
            column: Source::Lookup {
                path: vec![customer(), country()],
                column: "code".into(),
            },
            op: Op::Eq,
            value: "IT".into(),
        }];
        q.sort = Some((
            Source::Lookup {
                path: vec![customer()],
                column: "name".into(),
            },
            false,
        ));
        assert_eq!(
            q.sql(),
            "SELECT t0.\"id\", t1.\"name\" AS \"customer › name\", \
             t2.\"name\" AS \"customer › countries › name\" \
             FROM \"shop\".\"orders\" AS t0 \
             LEFT JOIN \"shop\".\"customers\" AS t1 ON t1.\"id\" = t0.\"customer_id\" \
             LEFT JOIN \"shop\".\"countries\" AS t2 ON t2.\"code\" = t1.\"country_code\" \
             WHERE t2.\"code\" = 'IT' ORDER BY t1.\"name\", t0.\"id\""
        );
    }

    #[test]
    fn summaries_are_correlated_subqueries() {
        let items = Children {
            schema: "shop".into(),
            table: "order_items".into(),
            columns: vec!["order_id".into()],
            ref_columns: vec!["id".into()],
        };
        let mut q = query(Engine::Mysql);
        q.columns = vec![
            Source::Summary {
                children: items.clone(),
                agg: Agg::Count,
                column: None,
            },
            Source::Summary {
                children: items.clone(),
                agg: Agg::List,
                column: Some("sku".into()),
            },
        ];
        q.filters = vec![Filter {
            column: q.columns[0].clone(),
            op: Op::Gt,
            value: "2".into(),
        }];
        let sql = q.sql();
        assert!(
            sql.starts_with(
                "SELECT (SELECT count(*) FROM `shop`.`order_items` AS c WHERE c.`order_id` = t0.`id`) \
                 AS `order_items: count`, \
                 (SELECT GROUP_CONCAT(DISTINCT c.`sku` SEPARATOR ', ') FROM `shop`.`order_items` AS c \
                 WHERE c.`order_id` = t0.`id`) AS `order_items: list(sku)` FROM"
            ),
            "{sql}"
        );
        assert!(sql.contains("WHERE (SELECT count(*)"), "{sql}");
    }

    #[test]
    fn hop_labels() {
        assert_eq!(customer().label(), "customer");
        assert_eq!(country().label(), "countries");
    }

    #[test]
    fn summaries_group_and_aggregate() {
        let mut q = query(Engine::Postgres);
        q.filters = vec![Filter {
            column: Source::Base("note".into()),
            op: Op::IsNotNull,
            value: String::new(),
        }];
        q.summary = Some(Summary {
            by: vec![Source::Lookup {
                path: vec![customer()],
                column: "name".into(),
            }],
            aggregates: vec![
                (Agg::Count, None),
                (Agg::Sum, Some(Source::Base("total".into()))),
            ],
            sort: Some((1, true)),
        });
        assert_eq!(
            q.summary.as_ref().unwrap().labels(),
            ["customer › name", "count", "sum(total)"]
        );
        assert_eq!(
            q.sql(),
            "SELECT t1.\"name\" AS \"customer › name\", count(*) AS \"count\", \
             sum(t0.\"total\") AS \"sum(total)\" FROM \"shop\".\"orders\" AS t0 \
             LEFT JOIN \"shop\".\"customers\" AS t1 ON t1.\"id\" = t0.\"customer_id\" \
             WHERE t0.\"note\" IS NOT NULL GROUP BY t1.\"name\" ORDER BY 2 DESC, 1"
        );
        q.summary = Some(Summary {
            by: Vec::new(),
            aggregates: vec![(Agg::List, Some(Source::Base("note".into())))],
            sort: None,
        });
        q.engine = Engine::Mysql;
        assert_eq!(
            q.sql(),
            "SELECT GROUP_CONCAT(DISTINCT t0.`note` SEPARATOR ', ') AS `list(note)` \
             FROM `shop`.`orders` AS t0 WHERE t0.`note` IS NOT NULL"
        );
    }
}
