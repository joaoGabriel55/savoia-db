//! The built-in exporter, for when no external tool fits. It covers what most
//! schemas hold (tables, columns, keys, constraints, indexes, sequences, enum
//! types, views, data) but not routines, triggers, grants or partitioning,
//! so the UI labels it as lower fidelity. See
//! `docs/adr/202610091007-use-native-dump-tools-with-built-in-rust-fallback.md`.
//!
//! Everything is read inside one read-only snapshot transaction, so the dump
//! is consistent. The SQL output is plain statements (multi-row `INSERT`s, no
//! `COPY` blocks), so any client, and our own importer, can replay it.

mod mysql;
mod postgres;

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use savoia_core::export::{self, Format};
use savoia_core::{AppError, AppResult, ColumnMeta, Connection, Engine, QueryEvent, Row};

/// Rows per `INSERT` statement.
const INSERT_ROWS: usize = 100;
/// ...unless the statement grows past this many bytes first.
const INSERT_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// One `.sql` file.
    Sql,
    /// One gzip-compressed `.sql.gz` file.
    SqlGz,
    /// One `<table>.csv` per table, with a header, in a directory. Data only.
    Csv,
}

#[derive(Debug, Clone)]
pub struct ExportRequest {
    /// The Postgres schema, or the MySQL database, to export.
    pub schema: String,
    /// Tables and views to export, by name; empty means all of them.
    pub tables: Vec<String>,
    /// Write the `CREATE` statements. Ignored for CSV.
    pub ddl: bool,
    /// Write the rows.
    pub data: bool,
    /// Start the dump by dropping the objects it creates. Ignored for CSV.
    pub drop_existing: bool,
    pub format: OutputFormat,
    /// A file for SQL output, a directory for CSV.
    pub destination: PathBuf,
}

impl ExportRequest {
    fn includes(&self, name: &str) -> bool {
        self.tables.is_empty() || self.tables.iter().any(|t| t == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportEvent {
    /// Started on a table's rows.
    Table(String),
    /// Rows written so far for the current table.
    Rows(u64),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportSummary {
    pub tables: usize,
    pub rows: u64,
    /// Objects the built-in exporter can't write, with the reason.
    pub skipped: Vec<String>,
}

/// What an engine module reads from the catalog: everything except the rows.
#[derive(Debug, Default)]
struct Plan {
    /// `SET` statements that open the dump file.
    header: Vec<String>,
    /// Statements that close it.
    footer: Vec<String>,
    /// For `drop_existing`, in order.
    drops: Vec<String>,
    /// DDL before the tables: schema, types, sequences.
    before: Vec<String>,
    tables: Vec<TablePlan>,
    /// DDL after the rows: sequence values, constraints, indexes, views.
    after: Vec<String>,
    skipped: Vec<String>,
}

#[derive(Debug)]
struct TablePlan {
    name: String,
    /// `CREATE TABLE`, or `None` for a view (whose DDL is in `Plan::after`).
    create: Option<String>,
    /// Reads the rows, listing only the columns that can be inserted.
    select: String,
    /// `INSERT INTO t (a, b) VALUES`.
    insert: String,
}

/// Exports `request` from `conn`, reporting progress to `progress`.
///
/// Opens a transaction and changes session settings, so give it a
/// connection of its own. Dropping the future stops the export, leaving
/// only a `.part` file behind.
pub async fn export(
    conn: &dyn Connection,
    engine: Engine,
    request: &ExportRequest,
    mut progress: impl FnMut(ExportEvent) + Send,
) -> AppResult<ExportSummary> {
    let (begin, end) = match engine {
        Engine::Postgres => (postgres::BEGIN, postgres::END),
        Engine::Mysql => (mysql::BEGIN, mysql::END),
    };
    run(conn, begin).await?;
    let result = export_in_snapshot(conn, engine, request, &mut progress).await;
    let finish = run(conn, end).await;
    let summary = result?;
    finish?;
    Ok(summary)
}

async fn export_in_snapshot(
    conn: &dyn Connection,
    engine: Engine,
    request: &ExportRequest,
    progress: &mut (impl FnMut(ExportEvent) + Send),
) -> AppResult<ExportSummary> {
    let plan = match engine {
        Engine::Postgres => postgres::plan(conn, request).await?,
        Engine::Mysql => mysql::plan(conn, request).await?,
    };
    match request.format {
        OutputFormat::Sql | OutputFormat::SqlGz => {
            write_sql(conn, engine, request, plan, progress).await
        }
        OutputFormat::Csv => write_csv(conn, engine, request, plan, progress).await,
    }
}

async fn write_sql(
    conn: &dyn Connection,
    engine: Engine,
    request: &ExportRequest,
    plan: Plan,
    progress: &mut (impl FnMut(ExportEvent) + Send),
) -> AppResult<ExportSummary> {
    let part = PartFile::create(&request.destination)?;
    let mut out: Box<dyn Write + Send> = match request.format {
        OutputFormat::SqlGz => Box::new(flate2::write::GzEncoder::new(
            BufWriter::new(part.file()?),
            flate2::Compression::default(),
        )),
        _ => Box::new(BufWriter::new(part.file()?)),
    };
    let io = |err| part.error(err);

    writeln!(
        out,
        "-- {} dump of {} written by Savoia Studio's built-in exporter.\n\
         -- Not included: routines, triggers, grants, partitioned tables.\n",
        engine.label(),
        request.schema
    )
    .map_err(io)?;
    let statements = |out: &mut dyn Write, list: &[String]| -> io::Result<()> {
        for statement in list {
            writeln!(out, "{statement}")?;
        }
        if !list.is_empty() {
            writeln!(out)?;
        }
        Ok(())
    };
    statements(&mut out, &plan.header).map_err(io)?;
    if request.ddl {
        if request.drop_existing {
            statements(&mut out, &plan.drops).map_err(io)?;
        }
        statements(&mut out, &plan.before).map_err(io)?;
    }

    let mut summary = ExportSummary {
        skipped: plan.skipped,
        ..ExportSummary::default()
    };
    for table in &plan.tables {
        if request.ddl
            && let Some(create) = &table.create
        {
            writeln!(out, "{create}\n").map_err(io)?;
        }
        if !request.data || table.create.is_none() {
            continue;
        }
        progress(ExportEvent::Table(table.name.clone()));
        let mut batch = InsertBatch::new(&table.insert);
        let rows = stream_rows(conn, &table.select, |columns, rows, total| {
            for row in rows {
                batch.push(&export::insert_values(engine, columns, row));
                if batch.is_full() {
                    batch.flush(&mut out)?;
                }
            }
            progress(ExportEvent::Rows(total));
            Ok(())
        })
        .await
        .map_err(|err| err.into_app(&part))?;
        batch.flush(&mut out).map_err(io)?;
        if rows > 0 {
            writeln!(out).map_err(io)?;
        }
        summary.tables += 1;
        summary.rows += rows;
    }

    if request.ddl {
        statements(&mut out, &plan.after).map_err(io)?;
    }
    statements(&mut out, &plan.footer).map_err(io)?;
    out.flush().map_err(io)?;
    // Dropping the encoder writes the gzip trailer; `flush` alone doesn't.
    drop(out);
    part.commit()?;
    Ok(summary)
}

async fn write_csv(
    conn: &dyn Connection,
    engine: Engine,
    request: &ExportRequest,
    plan: Plan,
    progress: &mut (impl FnMut(ExportEvent) + Send),
) -> AppResult<ExportSummary> {
    fs::create_dir_all(&request.destination)
        .map_err(|err| AppError::storage(format!("{}: {err}", request.destination.display())))?;
    let mut summary = ExportSummary {
        skipped: plan.skipped,
        ..ExportSummary::default()
    };
    for table in &plan.tables {
        // Views only when asked for by name: their rows come from the tables.
        if table.create.is_none() && !request.tables.contains(&table.name) {
            continue;
        }
        progress(ExportEvent::Table(table.name.clone()));
        let part = PartFile::create(
            &request
                .destination
                .join(format!("{}.csv", file_name(&table.name))),
        )?;
        let mut out = BufWriter::new(part.file()?);
        let mut header = true;
        let rows = stream_rows(conn, &table.select, |columns, rows, total| {
            let text = export::write(Format::Csv, engine, "", columns, rows);
            // `write` starts every chunk with the header; keep only the first.
            let body = if header {
                header = false;
                text.as_str()
            } else {
                text.split_once('\n').map_or("", |(_, rest)| rest)
            };
            out.write_all(body.as_bytes())?;
            progress(ExportEvent::Rows(total));
            Ok(())
        })
        .await
        .map_err(|err| err.into_app(&part))?;
        out.flush().map_err(|err| part.error(err))?;
        drop(out);
        part.commit()?;
        summary.tables += 1;
        summary.rows += rows;
    }
    Ok(summary)
}

/// Builds multi-row `INSERT`s.
struct InsertBatch<'a> {
    head: &'a str,
    rows: Vec<String>,
    bytes: usize,
}

impl<'a> InsertBatch<'a> {
    fn new(head: &'a str) -> Self {
        Self {
            head,
            rows: Vec::new(),
            bytes: 0,
        }
    }

    fn push(&mut self, values: &str) {
        self.bytes += values.len();
        self.rows.push(values.to_owned());
    }

    fn is_full(&self) -> bool {
        self.rows.len() >= INSERT_ROWS || self.bytes >= INSERT_BYTES
    }

    fn flush(&mut self, out: &mut dyn Write) -> io::Result<()> {
        if self.rows.is_empty() {
            return Ok(());
        }
        writeln!(out, "{}\n{};", self.head, self.rows.join(",\n"))?;
        self.rows.clear();
        self.bytes = 0;
        Ok(())
    }
}

/// A failure while streaming rows: from the server, or from writing them.
enum StreamError {
    Query(AppError),
    Write(io::Error),
}

impl StreamError {
    fn into_app(self, part: &PartFile) -> AppError {
        match self {
            StreamError::Query(err) => err,
            StreamError::Write(err) => part.error(err),
        }
    }
}

/// Runs `select`, handing each page of rows to `write` with the running total.
async fn stream_rows(
    conn: &dyn Connection,
    select: &str,
    mut write: impl FnMut(&[ColumnMeta], &[Row], u64) -> io::Result<()> + Send,
) -> Result<u64, StreamError> {
    let mut handle = conn
        .execute(select.to_owned())
        .await
        .map_err(StreamError::Query)?;
    let mut columns: Vec<ColumnMeta> = Vec::new();
    let mut total = 0u64;
    while let Some(event) = handle.next().await {
        match event.map_err(StreamError::Query)? {
            QueryEvent::Columns(meta) => columns = meta.to_vec(),
            QueryEvent::Rows(rows) => {
                total += rows.len() as u64;
                write(&columns, &rows, total).map_err(StreamError::Write)?;
            }
            QueryEvent::Done { .. } => {}
        }
    }
    Ok(total)
}

/// Runs statements that return nothing worth keeping.
async fn run(conn: &dyn Connection, sql: &str) -> AppResult<()> {
    query(conn, sql).await.map(drop)
}

/// The rows of the last result set of `sql`.
async fn query(conn: &dyn Connection, sql: &str) -> AppResult<Vec<Row>> {
    let mut handle = conn.execute(sql.to_owned()).await?;
    let mut rows = Vec::new();
    while let Some(event) = handle.next().await {
        match event? {
            QueryEvent::Columns(_) => rows.clear(),
            QueryEvent::Rows(page) => rows.extend(page),
            QueryEvent::Done { .. } => {}
        }
    }
    Ok(rows)
}

/// Cell `i` of `row` as text; NULL and missing cells are `""`.
fn text(row: &Row, i: usize) -> &str {
    row.get(i).and_then(|cell| cell.as_deref()).unwrap_or("")
}

/// A Postgres boolean or MySQL tinyint rendered as text.
fn truthy(row: &Row, i: usize) -> bool {
    matches!(text(row, i), "t" | "true" | "1")
}

/// A table name made safe for a file name.
fn file_name(table: &str) -> String {
    table
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Output written to `<path>.part` and renamed into place on success, so a
/// failed or cancelled export never leaves a file that looks complete.
struct PartFile {
    part: PathBuf,
    path: PathBuf,
    committed: bool,
}

impl PartFile {
    fn create(path: &Path) -> AppResult<Self> {
        let mut part = path.as_os_str().to_owned();
        part.push(".part");
        let part = PathBuf::from(part);
        File::create(&part)
            .map_err(|err| AppError::storage(format!("{}: {err}", part.display())))?;
        Ok(Self {
            part,
            path: path.to_owned(),
            committed: false,
        })
    }

    fn file(&self) -> AppResult<File> {
        File::options()
            .write(true)
            .open(&self.part)
            .map_err(|err| self.error(err))
    }

    fn error(&self, err: io::Error) -> AppError {
        AppError::storage(format!("{}: {err}", self.path.display()))
    }

    fn commit(mut self) -> AppResult<()> {
        fs::rename(&self.part, &self.path).map_err(|err| self.error(err))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for PartFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.part);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_batches_split_by_rows() {
        let mut out = Vec::new();
        let mut batch = InsertBatch::new("INSERT INTO t (a) VALUES");
        for i in 0..INSERT_ROWS + 1 {
            batch.push(&format!("({i})"));
            if batch.is_full() {
                batch.flush(&mut out).unwrap();
            }
        }
        batch.flush(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("INSERT INTO").count(), 2);
        assert!(text.starts_with("INSERT INTO t (a) VALUES\n(0),\n(1),"));
        assert!(text.ends_with("INSERT INTO t (a) VALUES\n(100);\n"));
    }

    #[test]
    fn part_files_are_renamed_on_commit_and_removed_otherwise() {
        let dir = std::env::temp_dir().join(format!("savoia-part-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let done = dir.join("done.sql");
        let part = PartFile::create(&done).unwrap();
        part.file().unwrap().write_all(b"ok").unwrap();
        part.commit().unwrap();
        assert_eq!(fs::read_to_string(&done).unwrap(), "ok");

        let failed = dir.join("failed.sql");
        drop(PartFile::create(&failed).unwrap());
        assert!(!failed.exists());
        assert!(!dir.join("failed.sql.part").exists());
    }

    #[test]
    fn file_names_are_sanitized() {
        assert_eq!(file_name("public.orders"), "public.orders");
        assert_eq!(file_name("a/b c"), "a_b_c");
    }
}
