//! What `Connection::execute` streams back: server-rendered text cells, in
//! pages, over a bounded channel. See
//! `docs/adr/202610091303-stream-query-results-as-server-rendered-text-pages.md`.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{AppError, AppResult};

/// A page is sent once it holds this many rows...
pub const PAGE_ROWS: usize = 500;
/// ...or once its first row has waited this long.
pub const PAGE_LATENCY: Duration = Duration::from_millis(50);
/// Events buffered between driver and UI. When full, the driver stops
/// reading and the server waits.
const CHANNEL_CAPACITY: usize = 2;

/// A value exactly as the server rendered it. `None` is SQL `NULL`.
pub type Cell = Option<Box<str>>;
pub type Row = Box<[Cell]>;

/// A coarse type category. Cosmetic only (alignment, formatting hints);
/// `Other` is always allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Bool,
    Integer,
    Decimal,
    Float,
    Text,
    Temporal,
    Json,
    Binary,
    Other,
}

impl ValueKind {
    pub fn is_numeric(self) -> bool {
        matches!(
            self,
            ValueKind::Integer | ValueKind::Decimal | ValueKind::Float
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMeta {
    pub name: String,
    /// The engine's own type name, e.g. `timestamptz`; empty when unknown.
    pub type_name: String,
    pub kind: ValueKind,
}

impl ColumnMeta {
    /// A column whose type the driver couldn't learn.
    pub fn untyped(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_name: String::new(),
            kind: ValueKind::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryEvent {
    /// A result set begins; `Rows` that follow belong to it.
    Columns(Arc<[ColumnMeta]>),
    Rows(Vec<Row>),
    /// One statement finished. `rows_affected` is `None` when the server
    /// doesn't report it.
    Done {
        rows_affected: Option<u64>,
        elapsed: Duration,
    },
}

/// Engine-specific way to stop a running query.
#[async_trait]
pub trait Cancel: Send + Sync {
    async fn cancel(&self) -> AppResult<()>;
}

/// Cancels the query it came from. Cheap to clone; usable from any task.
#[derive(Clone)]
pub struct CancelHandle(Arc<dyn Cancel>);

impl CancelHandle {
    pub fn new(cancel: impl Cancel + 'static) -> Self {
        Self(Arc::new(cancel))
    }

    pub async fn cancel(&self) -> AppResult<()> {
        self.0.cancel().await
    }
}

impl fmt::Debug for CancelHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CancelHandle")
    }
}

/// The UI's end of a running execution.
///
/// Dropping it before the end tells the driver to cancel the query and drain
/// what's left in the background.
#[derive(Debug)]
pub struct QueryHandle {
    events: mpsc::Receiver<AppResult<QueryEvent>>,
    cancel: CancelHandle,
}

impl QueryHandle {
    /// For drivers: a connected sender/handle pair.
    pub fn channel(cancel: CancelHandle) -> (QuerySender, QueryHandle) {
        let (tx, events) = mpsc::channel(CHANNEL_CAPACITY);
        let sender = QuerySender {
            tx,
            page: Vec::new(),
            page_started: None,
            statement_started: Instant::now(),
        };
        (sender, QueryHandle { events, cancel })
    }

    /// The next event, or `None` once the execution has ended (after the last
    /// `Done`, or after an `Err`). Doesn't need a Tokio runtime.
    pub async fn next(&mut self) -> Option<AppResult<QueryEvent>> {
        self.events.recv().await
    }

    pub fn cancel_handle(&self) -> CancelHandle {
        self.cancel.clone()
    }
}

/// The UI dropped its `QueryHandle`. The driver should cancel and drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed;

/// The driver's end of an execution: batches rows into pages and times
/// statements. Dropping it ends the execution.
#[derive(Debug)]
pub struct QuerySender {
    tx: mpsc::Sender<AppResult<QueryEvent>>,
    page: Vec<Row>,
    page_started: Option<Instant>,
    statement_started: Instant,
}

impl QuerySender {
    /// Starts a result set.
    pub async fn columns(&mut self, columns: Vec<ColumnMeta>) -> Result<(), Closed> {
        self.flush().await?;
        self.send(Ok(QueryEvent::Columns(columns.into()))).await
    }

    /// Adds a row to the current page, sending the page once it's full.
    pub async fn row(&mut self, row: Row) -> Result<(), Closed> {
        self.page_started.get_or_insert_with(Instant::now);
        self.page.push(row);
        if self.page.len() >= PAGE_ROWS {
            self.flush().await?;
        }
        Ok(())
    }

    /// When the pending page must be sent even if no more rows arrive.
    /// Drivers wait on this alongside the server's stream.
    pub fn flush_deadline(&self) -> Option<Instant> {
        self.page_started.map(|started| started + PAGE_LATENCY)
    }

    /// Sends the pending page, if any.
    pub async fn flush(&mut self) -> Result<(), Closed> {
        self.page_started = None;
        if self.page.is_empty() {
            return Ok(());
        }
        let page = std::mem::take(&mut self.page);
        self.send(Ok(QueryEvent::Rows(page))).await
    }

    /// Ends the current statement; the next one is timed from now.
    pub async fn done(&mut self, rows_affected: Option<u64>) -> Result<(), Closed> {
        self.flush().await?;
        let elapsed = self.statement_started.elapsed();
        self.send(Ok(QueryEvent::Done {
            rows_affected,
            elapsed,
        }))
        .await?;
        self.statement_started = Instant::now();
        Ok(())
    }

    /// Ends the execution with `err`, after the rows already read.
    pub async fn fail(mut self, err: AppError) {
        if self.flush().await.is_ok() {
            let _ = self.send(Err(err)).await;
        }
    }

    /// Whether the UI has dropped its handle.
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }

    async fn send(&self, event: AppResult<QueryEvent>) -> Result<(), Closed> {
        self.tx.send(event).await.map_err(|_| Closed)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct CountCancels(Arc<AtomicUsize>);

    #[async_trait]
    impl Cancel for CountCancels {
        async fn cancel(&self) -> AppResult<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn channel() -> (QuerySender, QueryHandle) {
        QueryHandle::channel(CancelHandle::new(CountCancels(Arc::default())))
    }

    fn row(text: &str) -> Row {
        Box::new([Some(text.into()), None])
    }

    fn rows_in(event: Option<AppResult<QueryEvent>>) -> Vec<Row> {
        match event {
            Some(Ok(QueryEvent::Rows(rows))) => rows,
            other => panic!("expected rows, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn batches_rows_into_full_pages() {
        let (mut tx, mut handle) = channel();
        let producer = tokio::spawn(async move {
            tx.columns(vec![ColumnMeta::untyped("n")]).await.unwrap();
            for i in 0..PAGE_ROWS + 1 {
                tx.row(row(&i.to_string())).await.unwrap();
            }
            tx.done(None).await.unwrap();
        });

        let Some(Ok(QueryEvent::Columns(columns))) = handle.next().await else {
            panic!("expected columns first");
        };
        assert_eq!(columns[0].name, "n");
        assert_eq!(rows_in(handle.next().await).len(), PAGE_ROWS);
        // `done` flushes the partial page before reporting.
        assert_eq!(
            rows_in(handle.next().await),
            vec![row(&PAGE_ROWS.to_string())]
        );
        assert!(matches!(
            handle.next().await,
            Some(Ok(QueryEvent::Done { .. }))
        ));
        producer.await.unwrap();
        assert!(handle.next().await.is_none());
    }

    #[tokio::test]
    async fn keeps_null_distinct_from_empty() {
        let (mut tx, mut handle) = channel();
        tx.row(Box::new([Some("".into()), None])).await.unwrap();
        tx.flush().await.unwrap();
        let rows = rows_in(handle.next().await);
        assert_eq!(rows[0][0].as_deref(), Some(""));
        assert_eq!(rows[0][1], None);
    }

    #[tokio::test]
    async fn deadline_tracks_the_oldest_pending_row() {
        let (mut tx, _handle) = channel();
        assert_eq!(tx.flush_deadline(), None);
        let before = Instant::now();
        tx.row(row("a")).await.unwrap();
        let deadline = tx.flush_deadline().unwrap();
        assert!(deadline >= before + PAGE_LATENCY);
        tx.row(row("b")).await.unwrap();
        assert_eq!(tx.flush_deadline(), Some(deadline));
        tx.flush().await.unwrap();
        assert_eq!(tx.flush_deadline(), None);
    }

    #[tokio::test]
    async fn reports_rows_affected_per_statement() {
        let (mut tx, mut handle) = channel();
        tx.done(Some(3)).await.unwrap();
        tx.done(None).await.unwrap();
        drop(tx);
        let mut affected = Vec::new();
        while let Some(event) = handle.next().await {
            if let Ok(QueryEvent::Done { rows_affected, .. }) = event {
                affected.push(rows_affected);
            }
        }
        assert_eq!(affected, [Some(3), None]);
    }

    #[tokio::test]
    async fn failure_comes_after_rows_already_read() {
        let (mut tx, mut handle) = channel();
        tx.row(row("kept")).await.unwrap();
        tx.fail(AppError::query("division by zero")).await;
        assert_eq!(rows_in(handle.next().await), vec![row("kept")]);
        assert!(matches!(
            handle.next().await,
            Some(Err(AppError::Query { .. }))
        ));
        assert!(handle.next().await.is_none());
    }

    #[tokio::test]
    async fn dropped_handle_closes_the_sender() {
        let (mut tx, handle) = channel();
        assert!(!tx.is_closed());
        drop(handle);
        assert!(tx.is_closed());
        assert_eq!(tx.done(None).await, Err(Closed));
    }

    #[tokio::test]
    async fn stops_the_driver_when_the_ui_stops_reading() {
        let (mut tx, mut handle) = channel();
        for _ in 0..CHANNEL_CAPACITY {
            tx.done(None).await.unwrap();
        }
        let blocked = tokio::time::timeout(Duration::from_millis(50), tx.done(None)).await;
        assert!(blocked.is_err(), "sender should wait for the reader");
        handle.next().await.unwrap().unwrap();
        tx.done(None).await.unwrap();
    }

    #[tokio::test]
    async fn cancel_handle_reaches_the_driver() {
        let count = Arc::new(AtomicUsize::new(0));
        let (_tx, handle) = QueryHandle::channel(CancelHandle::new(CountCancels(count.clone())));
        let cancel = handle.cancel_handle();
        cancel.clone().cancel().await.unwrap();
        cancel.cancel().await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
