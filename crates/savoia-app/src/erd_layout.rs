//! Where the ER diagram puts each table: a small layered layout, see
//! `docs/adr/202610091437-draw-er-diagrams-natively-with-a-built-in-layered-layout.md`.
//! UI-free, in diagram units (pixels at 100%).

use savoia_core::TableInfo;

pub const BOX_WIDTH: f32 = 240.;
pub const HEADER_HEIGHT: f32 = 30.;
pub const ROW_HEIGHT: f32 = 22.;
/// Below the last column row.
pub const BOX_PADDING: f32 = 6.;
const GAP_X: f32 = 96.;
const GAP_Y: f32 = 36.;

pub fn box_height(table: &TableInfo) -> f32 {
    HEADER_HEIGHT + table.columns.len().max(1) as f32 * ROW_HEIGHT + BOX_PADDING
}

/// A foreign key between two tables of the diagram, by table and column index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub from: usize,
    pub from_row: usize,
    pub to: usize,
    pub to_row: usize,
}

/// The foreign keys of `tables` that point at tables of the same `schema`.
/// Self-references are left out: the box lists them.
pub fn edges(schema: &str, tables: &[TableInfo]) -> Vec<Edge> {
    let index = |name: &str| tables.iter().position(|t| t.name == name);
    let row = |table: &TableInfo, column: Option<&String>| {
        column
            .and_then(|c| table.columns.iter().position(|col| col.name == *c))
            .unwrap_or(0)
    };
    let mut out = Vec::new();
    for (from, table) in tables.iter().enumerate() {
        for fk in &table.foreign_keys {
            let Some(to) = index(&fk.ref_table).filter(|_| fk.ref_schema == schema) else {
                continue;
            };
            if to == from {
                continue;
            }
            out.push(Edge {
                from,
                from_row: row(table, fk.columns.first()),
                to,
                to_row: row(&tables[to], fk.ref_columns.first()),
            });
        }
    }
    out
}

/// Top-left corner of each table's box, in `tables` order.
///
/// Related tables go in columns by foreign-key depth: a table sits right of
/// every table it references. Within a column, tables are ordered by where
/// their referenced tables are, to keep lines short. Tables without any
/// relationship go in a grid below.
pub fn layout(tables: &[TableInfo], edges: &[Edge]) -> Vec<(f32, f32)> {
    let n = tables.len();
    let mut refs: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut related = vec![false; n];
    for e in edges {
        refs[e.from].push(e.to);
        related[e.from] = true;
        related[e.to] = true;
    }

    // Depth: 0 for tables that reference nothing; cycles are cut where met.
    fn depth(
        i: usize,
        refs: &[Vec<usize>],
        memo: &mut [Option<usize>],
        visiting: &mut [bool],
    ) -> usize {
        if let Some(d) = memo[i] {
            return d;
        }
        if visiting[i] {
            return 0;
        }
        visiting[i] = true;
        let d = refs[i]
            .iter()
            .map(|&j| depth(j, refs, memo, visiting) + 1)
            .max()
            .unwrap_or(0);
        visiting[i] = false;
        memo[i] = Some(d);
        d
    }
    let mut memo = vec![None; n];
    let mut visiting = vec![false; n];
    let mut columns: Vec<Vec<usize>> = Vec::new();
    for i in (0..n).filter(|&i| related[i]) {
        let d = depth(i, &refs, &mut memo, &mut visiting);
        if columns.len() <= d {
            columns.resize(d + 1, Vec::new());
        }
        columns[d].push(i);
    }

    // Order each column by the mean slot of what its tables reference; the
    // first column by how often each table is referenced, most first.
    let mut slot = vec![0f32; n];
    let mut incoming = vec![0usize; n];
    for e in edges {
        incoming[e.to] += 1;
    }
    for (c, column) in columns.iter_mut().enumerate() {
        if c == 0 {
            column.sort_by_key(|&i| std::cmp::Reverse(incoming[i]));
        } else {
            let key = |i: usize| {
                let placed: Vec<f32> = refs[i].iter().map(|&j| slot[j]).collect();
                placed.iter().sum::<f32>() / placed.len().max(1) as f32
            };
            column.sort_by(|&a, &b| key(a).total_cmp(&key(b)));
        }
        for (s, &i) in column.iter().enumerate() {
            slot[i] = s as f32;
        }
    }

    let mut out = vec![(0f32, 0f32); n];
    let mut bottom = 0f32;
    for (c, column) in columns.iter().enumerate() {
        let x = c as f32 * (BOX_WIDTH + GAP_X);
        let mut y = 0f32;
        for &i in column {
            out[i] = (x, y);
            y += box_height(&tables[i]) + GAP_Y;
        }
        bottom = bottom.max(y);
    }

    let lone: Vec<usize> = (0..n).filter(|&i| !related[i]).collect();
    let per_row = (lone.len() as f32)
        .sqrt()
        .ceil()
        .max(columns.len() as f32)
        .max(1.) as usize;
    let mut y = bottom;
    for row in lone.chunks(per_row) {
        let mut tallest = 0f32;
        for (k, &i) in row.iter().enumerate() {
            out[i] = (k as f32 * (BOX_WIDTH + GAP_X), y);
            tallest = tallest.max(box_height(&tables[i]));
        }
        y += tallest + GAP_Y;
    }
    out
}

#[cfg(test)]
mod tests {
    use savoia_core::{ColumnInfo, ForeignKey, TableInfo, TableKind};

    use super::{BOX_WIDTH, Edge, edges, layout};

    fn table(name: &str, columns: &[&str], fks: &[(&str, &str, &str)]) -> TableInfo {
        let mut t = TableInfo::new(name, TableKind::Table);
        t.columns = columns
            .iter()
            .map(|c| ColumnInfo {
                name: c.to_string(),
                data_type: "int".into(),
                nullable: false,
                default: None,
            })
            .collect();
        t.foreign_keys = fks
            .iter()
            .map(|(column, ref_table, ref_column)| ForeignKey {
                name: format!("{name}_{column}_fkey"),
                columns: vec![column.to_string()],
                ref_schema: "public".into(),
                ref_table: ref_table.to_string(),
                ref_columns: vec![ref_column.to_string()],
            })
            .collect();
        t
    }

    fn shop() -> Vec<TableInfo> {
        vec![
            table(
                "order_items",
                &["order_id", "product_id"],
                &[
                    ("order_id", "orders", "id"),
                    ("product_id", "products", "id"),
                ],
            ),
            table(
                "orders",
                &["id", "customer_id"],
                &[("customer_id", "customers", "id")],
            ),
            table("customers", &["id"], &[]),
            table("products", &["id"], &[]),
            table("audit_log", &["id"], &[]),
        ]
    }

    #[test]
    fn edges_join_key_rows_within_the_schema() {
        let mut tables = shop();
        tables[1].foreign_keys.push(ForeignKey {
            name: "elsewhere".into(),
            columns: vec!["customer_id".into()],
            ref_schema: "billing".into(),
            ref_table: "customers".into(),
            ref_columns: vec!["id".into()],
        });
        assert_eq!(
            edges("public", &tables),
            [
                Edge {
                    from: 0,
                    from_row: 0,
                    to: 1,
                    to_row: 0
                },
                Edge {
                    from: 0,
                    from_row: 1,
                    to: 3,
                    to_row: 0
                },
                Edge {
                    from: 1,
                    from_row: 1,
                    to: 2,
                    to_row: 0
                },
            ]
        );
    }

    #[test]
    fn referenced_tables_sit_left_of_their_referrers() {
        let tables = shop();
        let edges = edges("public", &tables);
        let at = layout(&tables, &edges);
        for e in &edges {
            assert!(at[e.to].0 + BOX_WIDTH < at[e.from].0, "{e:?} {at:?}");
        }
        // The unrelated table goes below the related ones.
        let related_bottom = [0, 1, 2, 3].iter().map(|&i| at[i].1).fold(0., f32::max);
        assert!(at[4].1 > related_bottom, "{at:?}");
    }

    #[test]
    fn boxes_never_overlap() {
        let tables = shop();
        let at = layout(&tables, &edges("public", &tables));
        for i in 0..tables.len() {
            for j in i + 1..tables.len() {
                let (a, b) = (at[i], at[j]);
                let apart_x = a.0 + BOX_WIDTH <= b.0 || b.0 + BOX_WIDTH <= a.0;
                let apart_y = a.1 + super::box_height(&tables[i]) <= b.1
                    || b.1 + super::box_height(&tables[j]) <= a.1;
                assert!(apart_x || apart_y, "{i} and {j} overlap: {at:?}");
            }
        }
    }

    #[test]
    fn cycles_still_lay_out() {
        let tables = vec![
            table("a", &["id", "b_id"], &[("b_id", "b", "id")]),
            table("b", &["id", "a_id"], &[("a_id", "a", "id")]),
        ];
        let at = layout(&tables, &edges("public", &tables));
        assert_ne!(at[0], at[1]);
    }
}
