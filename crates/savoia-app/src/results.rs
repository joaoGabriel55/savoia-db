//! Result grid: a virtualized `DataTable` over the rows streamed so far.
//!
//! The console's reader stops pulling pages once the grid holds what the
//! user has scrolled near ([`Pacer`]); the driver then stops reading and the
//! server waits, so a huge result never piles up in memory.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use gpui_kit::component::table::{Column, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::*;
use savoia_core::{ColumnMeta, PAGE_ROWS, Row};
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

pub struct ResultSet {
    columns: Vec<Column>,
    meta: Arc<[ColumnMeta]>,
    rows: Vec<Row>,
    /// Set while the result is still streaming.
    pacer: Option<Arc<Pacer>>,
}

impl ResultSet {
    pub fn empty() -> Self {
        Self {
            columns: vec![row_number_column()],
            meta: Arc::new([]),
            rows: Vec::new(),
            pacer: None,
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
    }

    pub fn extend(&mut self, rows: Vec<Row>) {
        self.rows.extend(rows);
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

    /// Whether a statement has produced a result set (beyond the row-number column).
    pub fn has_result(&self) -> bool {
        self.columns.len() > 1
    }

    fn cell(&self, row_ix: usize, col_ix: usize) -> Option<&str> {
        self.rows[row_ix][col_ix - 1].as_deref()
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

fn data_column(meta: &ColumnMeta) -> Column {
    let column = Column::new(SharedString::from(meta.name.clone()), meta.name.clone());
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
        self.rows.len()
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

    use super::{LOAD_AHEAD, Next, Pacer};

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
