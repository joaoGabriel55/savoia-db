//! Result grid: a virtualized `DataTable` over the rows streamed so far.
//!
//! The console's reader stops pulling pages once the grid holds what the
//! user has scrolled near ([`Pacer`]); the driver then stops reading and the
//! server waits, so a huge result never piles up in memory.
//!
//! Sorting and the quick filter work on the rows loaded so far, through a
//! view of row indices; the rows themselves stay in arrival order.

use std::cmp::Ordering as Order;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use gpui_kit::component::IconName;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::*;
use savoia_core::export::{self, Format};
use savoia_core::{ColumnMeta, Engine, PAGE_ROWS, Row};
use tokio::sync::Notify;

/// Rows loaded ahead of the last one the user has scrolled near.
const LOAD_AHEAD: usize = 2 * PAGE_ROWS;

/// How many rows the reader may load before it waits for the grid to ask
/// for more. Shared between the grid (UI) and the console's reader task.
///
/// A paused result also holds up the statements after it, so the user can
/// load all of it or skip the rest (read off and discard it).
pub struct Pacer {
    wanted: AtomicUsize,
    skip: AtomicBool,
    more: Notify,
}

/// What the reader does with the page it holds.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    Load,
    Skip,
}

impl Pacer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            wanted: AtomicUsize::new(LOAD_AHEAD),
            skip: AtomicBool::new(false),
            more: Notify::new(),
        })
    }

    /// Waits until the grid wants more than `loaded` rows, or until the user
    /// skips the rest.
    pub async fn room_for(&self, loaded: usize) -> Next {
        loop {
            if self.skip.load(Ordering::Acquire) {
                return Next::Skip;
            }
            if loaded < self.wanted.load(Ordering::Acquire) {
                return Next::Load;
            }
            self.more.notified().await;
        }
    }

    /// Whether the reader stops at `loaded` rows.
    fn is_full(&self, loaded: usize) -> bool {
        !self.skip.load(Ordering::Acquire) && loaded >= self.wanted.load(Ordering::Acquire)
    }

    pub fn load_all(&self) {
        self.want(usize::MAX);
    }

    pub fn skip_rest(&self) {
        self.skip.store(true, Ordering::Release);
        self.more.notify_one();
    }

    fn want(&self, rows: usize) {
        self.wanted.fetch_max(rows, Ordering::AcqRel);
        // `notify_one` keeps a permit, so a wake-up before the reader starts
        // waiting isn't lost.
        self.more.notify_one();
    }
}

/// The target table named in copied INSERTs; results don't know theirs.
const INSERT_TABLE: &str = "my_table";

pub struct ResultSet {
    engine: Engine,
    columns: Vec<Column>,
    meta: Arc<[ColumnMeta]>,
    rows: Vec<Row>,
    /// Set while the result is still streaming.
    pacer: Option<Arc<Pacer>>,
    /// Data column (0-based) and whether it sorts descending.
    sort: Option<(usize, bool)>,
    /// Lower-cased quick filter; empty shows every row.
    filter: String,
    /// The rows shown, by index into `rows`, while sorted or filtered.
    view: Option<Vec<usize>>,
}

impl ResultSet {
    pub fn empty(engine: Engine) -> Self {
        Self {
            engine,
            columns: vec![row_number_column()],
            meta: Arc::new([]),
            rows: Vec::new(),
            pacer: None,
            sort: None,
            filter: String::new(),
            view: None,
        }
    }

    /// Replaces the grid with a new, still-streaming result set. Call
    /// `TableState::refresh` afterwards.
    pub fn start(&mut self, meta: Arc<[ColumnMeta]>, pacer: Arc<Pacer>) {
        self.columns = std::iter::once(row_number_column())
            .chain(meta.iter().map(data_column))
            .collect();
        self.meta = meta;
        self.rows.clear();
        self.pacer = Some(pacer);
        self.sort = None;
        self.view = None;
    }

    pub fn extend(&mut self, rows: Vec<Row>) {
        self.rows.extend(rows);
        if self.view.is_some() {
            self.rebuild_view();
        }
    }

    /// Shows only rows with a cell containing `text`, ignoring case. Call
    /// `TableState::refresh` afterwards.
    pub fn set_filter(&mut self, text: &str) {
        self.filter = text.trim().to_lowercase();
        self.rebuild_view();
    }

    fn rebuild_view(&mut self) {
        if self.sort.is_none() && self.filter.is_empty() {
            self.view = None;
            return;
        }
        let mut view: Vec<usize> = (0..self.rows.len())
            .filter(|&i| self.matches(&self.rows[i]))
            .collect();
        if let Some((col, descending)) = self.sort {
            let numeric = self.meta[col].kind.is_numeric();
            view.sort_by(|&a, &b| {
                let order = compare(
                    self.rows[a][col].as_deref(),
                    self.rows[b][col].as_deref(),
                    numeric,
                );
                // NULLs stay last either way.
                match (
                    self.rows[a][col].is_none(),
                    self.rows[b][col].is_none(),
                    descending,
                ) {
                    (false, false, true) => order.reverse(),
                    _ => order,
                }
            });
        }
        self.view = Some(view);
    }

    fn matches(&self, row: &Row) -> bool {
        self.filter.is_empty()
            || row
                .iter()
                .flatten()
                .any(|cell| cell.to_lowercase().contains(&self.filter))
    }

    /// The index into `rows` of the row shown at `row_ix`.
    fn index(&self, row_ix: usize) -> usize {
        self.view.as_ref().map_or(row_ix, |view| view[row_ix])
    }

    /// The rows shown, in order.
    pub fn shown(&self) -> impl Iterator<Item = &Row> {
        (0..self.shown_len()).map(|i| &self.rows[self.index(i)])
    }

    pub fn shown_len(&self) -> usize {
        self.view.as_ref().map_or(self.rows.len(), Vec::len)
    }

    /// The shown rows (or only the one at `row_ix`) as text in `format`.
    pub fn export(&self, format: Format, row_ix: Option<usize>) -> String {
        let rows: Vec<&Row> = match row_ix {
            Some(ix) => vec![&self.rows[self.index(ix)]],
            None => self.shown().collect(),
        };
        export::write(format, self.engine, INSERT_TABLE, &self.meta, rows)
    }

    /// No more rows will arrive.
    pub fn finish(&mut self) {
        self.pacer = None;
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// The pacer of a still-streaming result.
    pub fn pacer(&self) -> Option<&Arc<Pacer>> {
        self.pacer.as_ref()
    }

    /// Streaming has stopped at the rows the grid asked for. (It may also
    /// have reached the end; the reader only learns that from the next event.)
    pub fn is_paused(&self) -> bool {
        self.pacer
            .as_ref()
            .is_some_and(|p| p.is_full(self.rows.len()))
    }

    fn cell(&self, row_ix: usize, col_ix: usize) -> Option<&str> {
        self.rows[self.index(row_ix)][col_ix - 1].as_deref()
    }
}

fn row_number_column() -> Column {
    Column::new("#", "")
        .width(px(44.))
        .text_right()
        .fixed_left()
        .resizable(false)
        .selectable(false)
}

/// Numbers by value when both parse, else text order; NULL sorts last.
fn compare(a: Option<&str>, b: Option<&str>, numeric: bool) -> Order {
    match (a, b) {
        (None, None) => Order::Equal,
        (None, Some(_)) => Order::Greater,
        (Some(_), None) => Order::Less,
        (Some(a), Some(b)) => {
            let parsed = numeric
                .then(|| a.parse::<f64>().ok().zip(b.parse::<f64>().ok()))
                .flatten();
            match parsed {
                Some((x, y)) => x.partial_cmp(&y).unwrap_or(Order::Equal),
                None => a.cmp(b),
            }
        }
    }
}

fn data_column(meta: &ColumnMeta) -> Column {
    let column = Column::new(SharedString::from(meta.name.clone()), meta.name.clone()).sortable();
    if meta.kind.is_numeric() {
        column.text_right()
    } else {
        column
    }
}

impl TableDelegate for ResultSet {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.shown_len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let cell = h_flex().size_full().font_family("monospace").text_sm();
        match col_ix {
            0 => cell
                .justify_end()
                .text_color(muted)
                .child((row_ix + 1).to_string()),
            _ => {
                let cell = if self.meta[col_ix - 1].kind.is_numeric() {
                    cell.justify_end()
                } else {
                    cell
                };
                match self.cell(row_ix, col_ix) {
                    Some(text) => cell.child(SharedString::from(text.to_owned())),
                    None => cell.text_color(muted).child("NULL"),
                }
            }
        }
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        self.sort = match (col_ix, sort) {
            (0, _) | (_, ColumnSort::Default) => None,
            (col, ColumnSort::Ascending) => Some((col - 1, false)),
            (col, ColumnSort::Descending) => Some((col - 1, true)),
        };
        self.rebuild_view();
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        let copy = |label: String, text: String| {
            PopupMenuItem::new(label)
                .icon(IconName::Copy)
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                })
        };
        let table = cx.entity().downgrade();
        let mut menu = menu;
        for format in Format::ALL {
            menu = menu.item(copy(
                format!("Copy row as {}", format.label()),
                self.export(format, Some(row_ix)),
            ));
        }
        menu = menu.separator();
        for format in Format::ALL {
            let table = table.clone();
            menu = menu.item(
                PopupMenuItem::new(format!("Copy all shown rows as {}", format.label()))
                    .icon(IconName::Copy)
                    .on_click(move |_, _, cx| {
                        if let Some(table) = table.upgrade() {
                            let text = table.read(cx).delegate().export(format, None);
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                        }
                    }),
            );
        }
        menu
    }

    fn has_more(&self, _: &App) -> bool {
        self.pacer.is_some()
    }

    fn load_more(&mut self, _: &mut Window, _: &mut Context<TableState<Self>>) {
        if let Some(pacer) = &self.pacer {
            pacer.want(self.rows.len() + LOAD_AHEAD);
        }
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
        match col_ix {
            0 => (row_ix + 1).to_string(),
            _ => self.cell(row_ix, col_ix).unwrap_or("NULL").to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use std::sync::Arc;

    use savoia_core::export::Format;
    use savoia_core::{ColumnMeta, Engine, Row, ValueKind};

    use super::{LOAD_AHEAD, Next, Pacer, ResultSet};

    fn result(rows: &[(&str, Option<&str>)]) -> ResultSet {
        let mut set = ResultSet::empty(Engine::Postgres);
        let meta: Arc<[ColumnMeta]> = Arc::new([
            ColumnMeta {
                name: "n".into(),
                type_name: "int4".into(),
                kind: ValueKind::Integer,
            },
            ColumnMeta::untyped("name"),
        ]);
        set.start(meta, Pacer::new());
        set.extend(
            rows.iter()
                .map(|(n, name)| -> Row { Box::new([Some((*n).into()), name.map(Into::into)]) })
                .collect(),
        );
        set
    }

    fn column(set: &ResultSet, col: usize) -> Vec<Option<&str>> {
        set.shown().map(|row| row[col].as_deref()).collect()
    }

    #[test]
    fn sorts_numbers_by_value_with_nulls_last() {
        let mut set = result(&[("10", Some("b")), ("9", None), ("100", Some("a"))]);
        set.sort = Some((0, false));
        set.rebuild_view();
        assert_eq!(column(&set, 0), [Some("9"), Some("10"), Some("100")]);
        set.sort = Some((1, true));
        set.rebuild_view();
        assert_eq!(column(&set, 1), [Some("b"), Some("a"), None]);
    }

    #[test]
    fn filter_keeps_order_and_follows_new_rows() {
        let mut set = result(&[("1", Some("Roma")), ("2", Some("Torino"))]);
        set.set_filter(" O");
        assert_eq!(column(&set, 0), [Some("1"), Some("2")]);
        set.set_filter("ROMA");
        assert_eq!(column(&set, 0), [Some("1")]);
        set.extend(vec![Box::new([Some("3".into()), Some("roma nord".into())])]);
        assert_eq!(column(&set, 0), [Some("1"), Some("3")]);
        assert_eq!(
            set.export(Format::Csv, None),
            "n,name\n1,Roma\n3,roma nord\n"
        );
        assert_eq!(set.export(Format::Csv, Some(1)), "n,name\n3,roma nord\n");
        set.set_filter("");
        assert_eq!(set.shown_len(), 3);
    }

    #[tokio::test]
    async fn reader_waits_until_the_grid_wants_more() {
        let pacer = Pacer::new();
        pacer.room_for(LOAD_AHEAD - 1).await;
        let waiting = tokio::time::timeout(Duration::from_millis(20), pacer.room_for(LOAD_AHEAD));
        assert!(waiting.await.is_err(), "should wait at the limit");

        // Asked for before the reader waits: the permit isn't lost.
        pacer.want(LOAD_AHEAD + 1);
        let next = tokio::time::timeout(Duration::from_millis(20), pacer.room_for(LOAD_AHEAD));
        assert_eq!(next.await.expect("room after want"), Next::Load);
    }

    #[tokio::test]
    async fn skipping_releases_a_paused_reader() {
        let pacer = Pacer::new();
        assert!(pacer.is_full(LOAD_AHEAD));
        pacer.skip_rest();
        assert!(!pacer.is_full(LOAD_AHEAD));
        assert_eq!(pacer.room_for(LOAD_AHEAD).await, Next::Skip);
        assert_eq!(pacer.room_for(0).await, Next::Skip);
    }

    #[test]
    fn load_all_never_pauses() {
        let pacer = Pacer::new();
        pacer.load_all();
        assert!(!pacer.is_full(usize::MAX - 1));
    }

    #[test]
    fn want_never_shrinks() {
        let pacer = Pacer::new();
        pacer.want(5000);
        pacer.want(10);
        assert_eq!(pacer.wanted.load(Ordering::Acquire), 5000);
    }
}
