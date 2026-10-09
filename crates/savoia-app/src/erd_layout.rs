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

/// Smallest and largest zoom; 1.0 is 100%.
pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 2.5;

/// A collapsed box shows only its header.
pub fn box_height(table: &TableInfo, collapsed: bool) -> f32 {
    if collapsed {
        HEADER_HEIGHT
    } else {
        HEADER_HEIGHT + table.columns.len().max(1) as f32 * ROW_HEIGHT + BOX_PADDING
    }
}

/// How the diagram maps to the view: `screen = offset + diagram * zoom`,
/// with screen coordinates relative to the view's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub zoom: f32,
    pub offset: (f32, f32),
}

impl Viewport {
    pub fn new(offset: (f32, f32)) -> Self {
        Self { zoom: 1., offset }
    }

    pub fn to_screen(self, (x, y): (f32, f32)) -> (f32, f32) {
        (self.offset.0 + x * self.zoom, self.offset.1 + y * self.zoom)
    }

    /// Multiplies the zoom by `factor` (within limits), keeping the diagram
    /// point under `anchor` (a screen point) where it is.
    pub fn zoom_at(&mut self, factor: f32, anchor: (f32, f32)) {
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let ratio = zoom / self.zoom;
        self.offset = (
            anchor.0 - (anchor.0 - self.offset.0) * ratio,
            anchor.1 - (anchor.1 - self.offset.1) * ratio,
        );
        self.zoom = zoom;
    }

    /// Shows all of `extent` (`min_x, min_y, max_x, max_y` in diagram units)
    /// centered in a `view` of `(width, height)`, never above 100%.
    pub fn fit(extent: (f32, f32, f32, f32), view: (f32, f32), margin: f32) -> Self {
        let (w, h) = ((extent.2 - extent.0).max(1.), (extent.3 - extent.1).max(1.));
        let room = (
            (view.0 - 2. * margin).max(1.),
            (view.1 - 2. * margin).max(1.),
        );
        let zoom = (room.0 / w).min(room.1 / h).clamp(MIN_ZOOM, 1.);
        let offset = (
            (view.0 - w * zoom) / 2. - extent.0 * zoom,
            (view.1 - h * zoom) / 2. - extent.1 * zoom,
        );
        Self { zoom, offset }
    }
}

/// The box enclosing every table, or `None` for no tables.
pub fn extent(positions: &[(f32, f32)], heights: &[f32]) -> Option<(f32, f32, f32, f32)> {
    positions
        .iter()
        .zip(heights)
        .fold(None, |acc, (&(x, y), &h)| {
            let (a, b, c, d) = acc.unwrap_or((x, y, x + BOX_WIDTH, y + h));
            Some((a.min(x), b.min(y), c.max(x + BOX_WIDTH), d.max(y + h)))
        })
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
///
/// `heights` are the boxes' heights, in `tables` order.
pub fn layout(heights: &[f32], edges: &[Edge]) -> Vec<(f32, f32)> {
    let n = heights.len();
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
            y += heights[i] + GAP_Y;
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
            tallest = tallest.max(heights[i]);
        }
        y += tallest + GAP_Y;
    }
    out
}

#[cfg(test)]
mod tests {
    use savoia_core::{ColumnInfo, ForeignKey, TableInfo, TableKind};

    use super::{BOX_WIDTH, Edge, MAX_ZOOM, Viewport, box_height, edges, extent, layout};

    fn heights(tables: &[TableInfo]) -> Vec<f32> {
        tables.iter().map(|t| box_height(t, false)).collect()
    }

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
        let at = layout(&heights(&tables), &edges);
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
        let at = layout(&heights(&tables), &edges("public", &tables));
        for i in 0..tables.len() {
            for j in i + 1..tables.len() {
                let (a, b) = (at[i], at[j]);
                let apart_x = a.0 + BOX_WIDTH <= b.0 || b.0 + BOX_WIDTH <= a.0;
                let apart_y = a.1 + box_height(&tables[i], false) <= b.1
                    || b.1 + box_height(&tables[j], false) <= a.1;
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
        let at = layout(&heights(&tables), &edges("public", &tables));
        assert_ne!(at[0], at[1]);
    }

    #[test]
    fn collapsed_boxes_pack_tighter() {
        let tables = shop();
        let edges = edges("public", &tables);
        let open = layout(&heights(&tables), &edges);
        let shut: Vec<f32> = tables.iter().map(|t| box_height(t, true)).collect();
        let packed = layout(&shut, &edges);
        let bottom = |at: &[(f32, f32)], h: &[f32]| extent(at, h).unwrap().3;
        assert!(bottom(&packed, &shut) < bottom(&open, &heights(&tables)));
    }

    #[test]
    fn zooming_keeps_the_point_under_the_cursor() {
        let mut v = Viewport::new((40., 10.));
        let cursor = (300., 200.);
        let under = (
            (cursor.0 - v.offset.0) / v.zoom,
            (cursor.1 - v.offset.1) / v.zoom,
        );
        v.zoom_at(1.5, cursor);
        let (x, y) = v.to_screen(under);
        assert!(
            (x - cursor.0).abs() < 1e-3 && (y - cursor.1).abs() < 1e-3,
            "{v:?}"
        );
        v.zoom_at(100., cursor);
        assert_eq!(v.zoom, MAX_ZOOM);
    }

    #[test]
    fn fit_shows_everything_centered_and_never_magnifies() {
        let extent = (100., 50., 2100., 1050.); // 2000 x 1000
        let v = Viewport::fit(extent, (1040., 640.), 20.);
        assert!((v.zoom - 0.5).abs() < 1e-6, "{v:?}");
        let (left, top) = v.to_screen((extent.0, extent.1));
        let (right, bottom) = v.to_screen((extent.2, extent.3));
        assert!(
            (left - (1040. - right)).abs() < 1e-3,
            "centered across: {left} {right}"
        );
        assert!(
            (top - (640. - bottom)).abs() < 1e-3,
            "centered down: {top} {bottom}"
        );
        let small = Viewport::fit((0., 0., 100., 100.), (1000., 1000.), 20.);
        assert_eq!(small.zoom, 1.);
    }
}
