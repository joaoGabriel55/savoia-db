//! Built-in import, for when `psql`/`mysql` aren't available: SQL scripts
//! run statement by statement, and CSV files inserted into a table.
//!
//! Both read their file in pieces, so size isn't limited by memory, and
//! accept gzip-compressed files.

mod csv;
mod sql;

use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use savoia_core::{AppError, AppResult, Connection, Engine};

pub use csv::{
    ColumnMapping, CsvImport, CsvOptions, CsvPreview, auto_map, import_csv, preview_csv,
};
pub use sql::{SqlImport, import_sql};

/// Errors kept in [`ImportSummary::errors`]; later ones are only counted.
const KEPT_ERRORS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnError {
    #[default]
    Stop,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportOptions {
    pub on_error: OnError,
    /// Run everything in one transaction, rolled back on the first error
    /// (so it implies [`OnError::Stop`]). MySQL commits DDL implicitly, so
    /// there it only protects data.
    pub single_transaction: bool,
}

/// One statement or row the server rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportError {
    /// Line in the file where the statement or row starts, from 1.
    pub line: u64,
    /// The start of the statement, or the row's values.
    pub what: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportEvent {
    Progress {
        /// Bytes of the file read so far (compressed bytes for `.gz`).
        read: u64,
        total: u64,
        /// Statements run, or rows inserted.
        done: u64,
    },
    Error(ImportError),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportSummary {
    /// Statements run, or rows inserted, successfully.
    pub done: u64,
    pub failed: u64,
    /// The first [`KEPT_ERRORS`] errors.
    pub errors: Vec<ImportError>,
    /// Stopped early on an error.
    pub stopped: bool,
    /// The single transaction was rolled back, so nothing was kept.
    pub rolled_back: bool,
}

/// Counts results and errors, and decides when to stop.
struct Tracker<F: FnMut(ImportEvent)> {
    options: ImportOptions,
    read: Arc<AtomicU64>,
    total: u64,
    progress: F,
    summary: ImportSummary,
}

impl<F: FnMut(ImportEvent)> Tracker<F> {
    fn new(options: ImportOptions, input: &Input, progress: F) -> Self {
        Self {
            options,
            read: input.read.clone(),
            total: input.total,
            progress,
            summary: ImportSummary::default(),
        }
    }

    fn succeeded(&mut self, count: u64) {
        self.summary.done += count;
        (self.progress)(ImportEvent::Progress {
            read: self.read.load(Ordering::Relaxed),
            total: self.total,
            done: self.summary.done,
        });
    }

    /// Records `error`; true when the import must stop.
    fn failed(&mut self, error: ImportError) -> bool {
        self.summary.failed += 1;
        if self.summary.errors.len() < KEPT_ERRORS {
            self.summary.errors.push(error.clone());
        }
        (self.progress)(ImportEvent::Error(error));
        let stop = self.options.on_error == OnError::Stop || self.options.single_transaction;
        self.summary.stopped = stop;
        stop
    }
}

/// The file being imported, decompressed if it is gzip.
struct Input {
    reader: Box<dyn Read + Send>,
    read: Arc<AtomicU64>,
    total: u64,
}

impl Input {
    fn open(path: &Path) -> AppResult<Self> {
        let storage = |err: io::Error| AppError::storage(format!("{}: {err}", path.display()));
        let file = File::open(path).map_err(storage)?;
        let total = file.metadata().map_err(storage)?.len();
        let read = Arc::new(AtomicU64::new(0));
        let mut counted = BufReader::new(Counted {
            inner: file,
            read: read.clone(),
        });
        let gzip = io::BufRead::fill_buf(&mut counted)
            .map_err(storage)?
            .starts_with(&[0x1f, 0x8b]);
        let reader: Box<dyn Read + Send> = if gzip {
            Box::new(flate2::bufread::MultiGzDecoder::new(counted))
        } else {
            Box::new(counted)
        };
        Ok(Self {
            reader,
            read,
            total,
        })
    }
}

/// Counts the bytes read through it.
struct Counted<R> {
    inner: R,
    read: Arc<AtomicU64>,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// Runs `sql`, draining any results. `Err` holds the server's message.
async fn run(conn: &dyn Connection, sql: String) -> Result<(), String> {
    let mut handle = conn.execute(sql).await.map_err(|err| err.to_string())?;
    while let Some(event) = handle.next().await {
        event.map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn begin(engine: Engine) -> &'static str {
    match engine {
        Engine::Postgres => "BEGIN",
        Engine::Mysql => "START TRANSACTION",
    }
}

/// Opens the single transaction, if asked for.
async fn start(conn: &dyn Connection, engine: Engine, options: ImportOptions) -> AppResult<()> {
    if options.single_transaction {
        run(conn, begin(engine).into())
            .await
            .map_err(AppError::query)?;
    }
    Ok(())
}

/// Commits the single transaction, or rolls it back after an error.
async fn end(
    conn: &dyn Connection,
    options: ImportOptions,
    summary: &mut ImportSummary,
) -> AppResult<()> {
    if !options.single_transaction {
        return Ok(());
    }
    if summary.failed > 0 {
        run(conn, "ROLLBACK".into())
            .await
            .map_err(AppError::query)?;
        summary.rolled_back = true;
        return Ok(());
    }
    run(conn, "COMMIT".into()).await.map_err(AppError::query)
}

/// Rolls back the single transaction after a failure that ends the import.
async fn abort(conn: &dyn Connection, options: ImportOptions) {
    if options.single_transaction {
        let _ = run(conn, "ROLLBACK".into()).await;
    }
}

/// `text` on one line, cut to `max` characters.
fn preview(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &flat[..cut]),
        None => flat,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_are_flat_and_short() {
        assert_eq!(preview("SELECT\n  1,\n  2", 100), "SELECT 1, 2");
        assert_eq!(preview("abcdef", 3), "abc…");
    }

    #[test]
    fn reads_gzip_transparently() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("savoia-input-{}.gz", uuid::Uuid::new_v4()));
        let mut gz = flate2::write::GzEncoder::new(
            File::create(&path).unwrap(),
            flate2::Compression::default(),
        );
        gz.write_all(b"SELECT 1;").unwrap();
        gz.finish().unwrap();
        let mut input = Input::open(&path).unwrap();
        let mut text = String::new();
        input.reader.read_to_string(&mut text).unwrap();
        assert_eq!(text, "SELECT 1;");
        assert_eq!(input.read.load(Ordering::Relaxed), input.total);
    }
}
