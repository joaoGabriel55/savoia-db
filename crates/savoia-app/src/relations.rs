//! What a data view can reach from its table through relationships: the
//! columns its "+ Column" picker offers, and the related rows its row menu
//! opens. See `docs/adr/202610091908-build-joins-from-foreign-key-relationship-paths.md`.

use std::collections::HashMap;
use std::sync::Arc;

use savoia_core::TableInfo;
use savoia_core::data_query::{Agg, Children, Hop, Source};

use crate::data_grid::is_numeric_type;

/// Table details by (schema, table).
pub type Tables = HashMap<(String, String), Arc<TableInfo>>;

/// A group of columns in the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub choices: Vec<(String, Source)>,
}

/// Rows related to a row of the view: its parent through a foreign key, or
/// its children through one pointing at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub label: String,
    pub schema: String,
    pub table: String,
    /// Columns of the related table that must match…
    pub target: Vec<String>,
    /// …these base-table columns of the row.
    pub base: Vec<String>,
}

/// How deep lookups follow foreign keys.
const DEPTH: usize = 2;

/// Lookup sections along every many-to-one path from `path`'s end, then
/// one summary section per table pointing at the base table.
pub fn sections(schema: &str, base: &TableInfo, tables: &Tables, custom: &[Hop]) -> Vec<Section> {
    let mut out = vec![Section {
        title: base.name.clone(),
        choices: base
            .columns
            .iter()
            .map(|c| (c.name.clone(), Source::Base(c.name.clone())))
            .collect(),
    }];
    let hops: Vec<Hop> = base
        .foreign_keys
        .iter()
        .map(Hop::from_fk)
        .chain(custom.iter().cloned())
        .collect();
    for hop in hops {
        lookups(vec![hop], tables, &mut out);
    }
    for (schema_name, table, fk) in children(schema, &base.name, tables) {
        let Some(info) = tables.get(&(schema_name.clone(), table.clone())) else {
            continue;
        };
        let children = Children::from_fk(&schema_name, &table, fk);
        let mut choices = vec![(
            "count".to_string(),
            Source::Summary {
                children: children.clone(),
                agg: Agg::Count,
                column: None,
            },
        )];
        for c in &info.columns {
            let aggs: &[Agg] = if is_numeric_type(&c.data_type) {
                &[Agg::Sum, Agg::Avg, Agg::Min, Agg::Max]
            } else {
                &[Agg::List, Agg::Min, Agg::Max]
            };
            for agg in aggs {
                choices.push((
                    format!("{}({})", agg.label(), c.name),
                    Source::Summary {
                        children: children.clone(),
                        agg: *agg,
                        column: Some(c.name.clone()),
                    },
                ));
            }
        }
        out.push(Section {
            title: format!("{table} ← has many, via {}", fk.columns.join(", ")),
            choices,
        });
    }
    out
}

fn lookups(path: Vec<Hop>, tables: &Tables, out: &mut Vec<Section>) {
    let hop = path.last().expect("paths are not empty");
    let Some(info) = tables.get(&(hop.schema.clone(), hop.table.clone())) else {
        return;
    };
    let labels: Vec<String> = path.iter().map(Hop::label).collect();
    out.push(Section {
        title: format!("{} → {}", labels.join(" › "), hop.table),
        choices: info
            .columns
            .iter()
            .map(|c| {
                (
                    c.name.clone(),
                    Source::Lookup {
                        path: path.clone(),
                        column: c.name.clone(),
                    },
                )
            })
            .collect(),
    });
    if path.len() < DEPTH {
        for fk in &info.foreign_keys {
            let next = Hop::from_fk(fk);
            // Don't walk straight back where we came from.
            if path
                .iter()
                .any(|h| h.table == next.table && h.schema == next.schema)
            {
                continue;
            }
            let mut longer = path.clone();
            longer.push(next);
            lookups(longer, tables, out);
        }
    }
}

/// Tables with a foreign key to `schema.table`, with that key.
fn children<'a>(
    schema: &str,
    table: &str,
    tables: &'a Tables,
) -> Vec<(String, String, &'a savoia_core::ForeignKey)> {
    let mut out: Vec<_> = tables
        .iter()
        .flat_map(|((s, t), info)| {
            info.foreign_keys
                .iter()
                .filter(|fk| fk.ref_schema == schema && fk.ref_table == table)
                .map(move |fk| (s.clone(), t.clone(), fk))
        })
        .collect();
    out.sort_by(|a, b| (&a.0, &a.1, &a.2.name).cmp(&(&b.0, &b.1, &b.2.name)));
    out
}

/// Related rows reachable from a row: referenced rows first, then children.
pub fn links(schema: &str, base: &TableInfo, tables: &Tables, custom: &[Hop]) -> Vec<Link> {
    let parents = base
        .foreign_keys
        .iter()
        .map(Hop::from_fk)
        .chain(custom.iter().cloned())
        .map(|hop| Link {
            label: format!("Open {} ({})", hop.label(), hop.table),
            schema: hop.schema.clone(),
            table: hop.table.clone(),
            target: hop.ref_columns.clone(),
            base: hop.columns.clone(),
        });
    let kids = children(schema, &base.name, tables)
        .into_iter()
        .map(|(s, t, fk)| Link {
            label: format!("Show {t} rows (via {})", fk.columns.join(", ")),
            schema: s,
            table: t,
            target: fk.columns.clone(),
            base: fk.ref_columns.clone(),
        });
    parents.chain(kids).collect()
}

/// The column that names a row of `info` in a picker: the first text-like
/// column outside the key, else the first key column.
pub fn display_column(info: &TableInfo) -> Option<String> {
    let texty = |t: &str| {
        let t = t.to_ascii_lowercase();
        t.contains("char") || t == "text" || t.contains("text") || t.starts_with("varchar")
    };
    info.columns
        .iter()
        .find(|c| texty(&c.data_type) && !info.is_key_column(&c.name))
        .or_else(|| info.columns.iter().find(|c| info.is_key_column(&c.name)))
        .or(info.columns.first())
        .map(|c| c.name.clone())
}

#[cfg(test)]
mod tests {
    use savoia_core::{ColumnInfo, ForeignKey, TableKind};

    use super::*;

    fn table(
        name: &str,
        columns: &[(&str, &str)],
        pk: &str,
        fks: &[(&str, &str, &str)],
    ) -> Arc<TableInfo> {
        let mut t = TableInfo::new(name, TableKind::Table);
        t.columns = columns
            .iter()
            .map(|(n, ty)| ColumnInfo {
                name: (*n).into(),
                data_type: (*ty).into(),
                nullable: true,
                default: None,
            })
            .collect();
        t.primary_key = vec![pk.into()];
        t.foreign_keys = fks
            .iter()
            .map(|(col, rt, rc)| ForeignKey {
                name: format!("{name}_{col}_fk"),
                columns: vec![(*col).into()],
                ref_schema: "shop".into(),
                ref_table: (*rt).into(),
                ref_columns: vec![(*rc).into()],
            })
            .collect();
        Arc::new(t)
    }

    fn shop() -> Tables {
        let mut t = Tables::new();
        let mut put = |info: Arc<TableInfo>| {
            t.insert(("shop".into(), info.name.clone()), info);
        };
        put(table(
            "countries",
            &[("code", "char(2)"), ("name", "text")],
            "code",
            &[],
        ));
        put(table(
            "customers",
            &[
                ("id", "integer"),
                ("name", "text"),
                ("country_code", "char(2)"),
            ],
            "id",
            &[("country_code", "countries", "code")],
        ));
        put(table(
            "orders",
            &[("id", "integer"), ("customer_id", "integer")],
            "id",
            &[("customer_id", "customers", "id")],
        ));
        put(table(
            "order_items",
            &[("order_id", "integer"), ("qty", "integer"), ("sku", "text")],
            "order_id",
            &[("order_id", "orders", "id")],
        ));
        t
    }

    #[test]
    fn sections_follow_paths_and_summaries() {
        let tables = shop();
        let orders = tables[&("shop".to_string(), "orders".to_string())].clone();
        let titles: Vec<String> = sections("shop", &orders, &tables, &[])
            .into_iter()
            .map(|s| s.title)
            .collect();
        assert_eq!(
            titles,
            [
                "orders",
                "customer → customers",
                "customer › countries → countries",
                "order_items ← has many, via order_id",
            ]
        );
        let items = sections("shop", &orders, &tables, &[]).pop().unwrap();
        let labels: Vec<&str> = items.choices.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(&labels[..3], ["count", "sum(order_id)", "avg(order_id)"]);
        assert!(labels.contains(&"list(sku)"));
    }

    #[test]
    fn links_to_parents_and_children() {
        let tables = shop();
        let orders = tables[&("shop".to_string(), "orders".to_string())].clone();
        let links = links("shop", &orders, &tables, &[]);
        assert_eq!(
            links,
            [
                Link {
                    label: "Open customer (customers)".into(),
                    schema: "shop".into(),
                    table: "customers".into(),
                    target: vec!["id".into()],
                    base: vec!["customer_id".into()],
                },
                Link {
                    label: "Show order_items rows (via order_id)".into(),
                    schema: "shop".into(),
                    table: "order_items".into(),
                    target: vec!["order_id".into()],
                    base: vec!["id".into()],
                },
            ]
        );
    }

    #[test]
    fn display_columns() {
        let tables = shop();
        let get = |t: &str| tables[&("shop".to_string(), t.to_string())].clone();
        assert_eq!(display_column(&get("customers")).as_deref(), Some("name"));
        assert_eq!(display_column(&get("orders")).as_deref(), Some("id"));
    }
}
