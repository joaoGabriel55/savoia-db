//! A dump or restore as one cancellable job: picks the external tool or the
//! built-in engine, builds the tool's arguments, and reports log lines,
//! progress and the end as events. UI-free; runs on a Tokio runtime.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use savoia_core::{AppError, Connection, Endpoint, Engine};
use tokio::sync::{Notify, mpsc};

use crate::builtin::{self, ExportEvent, ExportRequest, OutputFormat};
use crate::import::{
    ColumnMapping, CsvImport, CsvOptions, ImportEvent, ImportOptions, ImportSummary, OnError,
    SqlImport,
};
use crate::runner::{RunEnd, RunEvent, ToolCommand};
use crate::tools::{Flavor, FoundTool, Tool};

const CHANNEL_CAPACITY: usize = 256;

/// How a job reaches the server.
pub enum Runner {
    /// An external tool, connecting to `endpoint` (a tunnel's local end, if any).
    Tool {
        tool: FoundTool,
        endpoint: Endpoint,
        password: Option<String>,
    },
    /// The built-in engine, on a connection of its own.
    BuiltIn(Arc<dyn Connection>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Content {
    SchemaAndData,
    SchemaOnly,
    DataOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpFormat {
    Sql,
    SqlGz,
    /// One CSV per table in a directory; built-in only.
    Csv,
    /// `pg_dump --format=custom`, restored with `pg_restore`.
    PgArchive,
}

impl DumpFormat {
    pub fn label(self) -> &'static str {
        match self {
            DumpFormat::Sql => "SQL (.sql)",
            DumpFormat::SqlGz => "Compressed SQL (.sql.gz)",
            DumpFormat::Csv => "CSV, one file per table",
            DumpFormat::PgArchive => "pg_dump archive (.dump)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            DumpFormat::Sql => "sql",
            DumpFormat::SqlGz => "sql.gz",
            DumpFormat::Csv => "",
            DumpFormat::PgArchive => "dump",
        }
    }

    /// The formats each way of dumping can write.
    pub fn available(engine: Engine, tool: bool) -> &'static [DumpFormat] {
        match (engine, tool) {
            (Engine::Postgres, true) => {
                &[DumpFormat::Sql, DumpFormat::SqlGz, DumpFormat::PgArchive]
            }
            (Engine::Mysql, true) => &[DumpFormat::Sql],
            (_, false) => &[DumpFormat::Sql, DumpFormat::SqlGz, DumpFormat::Csv],
        }
    }
}

#[derive(Debug, Clone)]
pub struct DumpPlan {
    pub engine: Engine,
    /// The database to connect to.
    pub database: String,
    /// The Postgres schema, or the MySQL database again.
    pub schema: String,
    /// Tables and views to dump; empty means the whole schema.
    pub tables: Vec<String>,
    /// How many tables will be dumped, for progress; 0 when unknown.
    pub table_count: usize,
    pub content: Content,
    pub drop_existing: bool,
    /// Leave out `OWNER TO` and grants (Postgres tools), which fail on a
    /// server without the same roles.
    pub skip_owners: bool,
    pub format: DumpFormat,
    /// A file, or a directory for CSV.
    pub destination: PathBuf,
}

#[derive(Debug, Clone)]
pub enum RestoreSource {
    /// A SQL script, optionally gzip-compressed (built-in only then).
    Sql,
    /// A `pg_dump --format=custom` archive; `pg_restore` only.
    PgArchive,
    /// CSV into an existing table; built-in only.
    Csv {
        csv: CsvOptions,
        table: String,
        mapping: Vec<ColumnMapping>,
    },
}

#[derive(Debug, Clone)]
pub struct RestorePlan {
    pub engine: Engine,
    pub database: String,
    /// The Postgres schema (or MySQL database) of a CSV's table.
    pub schema: String,
    pub path: PathBuf,
    pub source: RestoreSource,
    pub options: ImportOptions,
    /// `pg_restore --clean`: drop objects before recreating them.
    pub drop_existing: bool,
    pub skip_owners: bool,
    /// The target server's major version, if known: pg_restore 17+ can't
    /// restore into older servers directly.
    pub server_major: Option<u32>,
}

impl RestoreSource {
    /// What a file holds, guessed from its name.
    pub fn guess(path: &Path) -> RestoreSource {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if name.ends_with(".csv") || name.ends_with(".csv.gz") {
            RestoreSource::Csv {
                csv: CsvOptions::default(),
                table: String::new(),
                mapping: Vec::new(),
            }
        } else if name.ends_with(".dump") || name.ends_with(".backup") {
            RestoreSource::PgArchive
        } else {
            RestoreSource::Sql
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobEvent {
    Log(String),
    /// `fraction` is 0..=1, or `None` while the total is unknown.
    Progress {
        fraction: Option<f32>,
        detail: String,
    },
    /// Always the last event.
    Finished(JobEnd),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobEnd {
    Succeeded(String),
    /// Finished, but some statements or rows failed.
    Partial(String),
    Failed(String),
    Cancelled,
}

/// The UI's end of a running job. Dropping it cancels the job.
#[derive(Debug)]
pub struct Job {
    events: mpsc::Receiver<JobEvent>,
    cancel: Arc<Notify>,
}

impl Job {
    /// The next event; `None` after [`JobEvent::Finished`]. Doesn't need a
    /// Tokio runtime.
    pub async fn next(&mut self) -> Option<JobEvent> {
        self.events.recv().await
    }

    pub fn cancel(&self) {
        self.cancel.notify_one();
    }

    /// Waits for the end, with every log line; for tests and scripts.
    pub async fn wait(mut self) -> (JobEnd, Vec<String>) {
        let mut log = Vec::new();
        while let Some(event) = self.next().await {
            match event {
                JobEvent::Log(line) => log.push(line),
                JobEvent::Progress { .. } => {}
                JobEvent::Finished(end) => return (end, log),
            }
        }
        (JobEnd::Failed("the job stopped unexpectedly".into()), log)
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.notify_one();
    }
}

/// Where a job reports to.
#[derive(Clone)]
struct Reporter(mpsc::Sender<JobEvent>);

impl Reporter {
    async fn log(&self, line: impl Into<String>) {
        let _ = self.0.send(JobEvent::Log(line.into())).await;
    }

    async fn progress(&self, fraction: Option<f32>, detail: impl Into<String>) {
        let _ = self
            .0
            .send(JobEvent::Progress {
                fraction: fraction.map(|f| f.clamp(0., 1.)),
                detail: detail.into(),
            })
            .await;
    }
}

fn start<F>(work: impl FnOnce(Reporter, Arc<Notify>) -> F) -> Job
where
    F: Future<Output = JobEnd> + Send + 'static,
{
    let (tx, events) = mpsc::channel(CHANNEL_CAPACITY);
    let cancel = Arc::new(Notify::new());
    let reporter = Reporter(tx.clone());
    let future = work(reporter, cancel.clone());
    tokio::spawn(async move {
        let end = future.await;
        let _ = tx.send(JobEvent::Finished(end)).await;
    });
    Job { events, cancel }
}

/// Starts dumping `plan`. Needs a Tokio runtime.
pub fn dump(plan: DumpPlan, runner: Runner) -> Job {
    start(move |reporter, cancel| async move {
        match runner {
            Runner::Tool {
                tool,
                endpoint,
                password,
            } => dump_with_tool(plan, tool, endpoint, password, reporter, cancel).await,
            Runner::BuiltIn(conn) => dump_built_in(plan, conn, reporter, cancel).await,
        }
    })
}

/// Starts restoring `plan`. Needs a Tokio runtime.
pub fn restore(plan: RestorePlan, runner: Runner) -> Job {
    start(move |reporter, cancel| async move {
        match runner {
            Runner::Tool {
                tool,
                endpoint,
                password,
            } => restore_with_tool(plan, tool, endpoint, password, reporter, cancel).await,
            Runner::BuiltIn(conn) => restore_built_in(plan, conn, reporter, cancel).await,
        }
    })
}

/// `name` as an exact-match pg_dump pattern.
fn pg_pattern(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The arguments after the connection options for dumping `plan` with `tool`.
pub fn dump_args(plan: &DumpPlan, tool: &FoundTool, part: &Path) -> Vec<String> {
    let mut args = vec!["--verbose".to_owned()];
    match plan.engine {
        Engine::Postgres => {
            if plan.tables.is_empty() {
                args.push(format!("--schema={}", pg_pattern(&plan.schema)));
            } else {
                // `--schema` has no effect once `--table` is given.
                for table in &plan.tables {
                    args.push(format!(
                        "--table={}.{}",
                        pg_pattern(&plan.schema),
                        pg_pattern(table)
                    ));
                }
            }
            match plan.content {
                Content::SchemaAndData => {}
                Content::SchemaOnly => args.push("--schema-only".into()),
                Content::DataOnly => args.push("--data-only".into()),
            }
            if plan.drop_existing && plan.content != Content::DataOnly {
                args.extend(["--clean".into(), "--if-exists".into()]);
            }
            if plan.skip_owners {
                args.extend(["--no-owner".into(), "--no-privileges".into()]);
            }
            match plan.format {
                DumpFormat::PgArchive => args.push("--format=custom".into()),
                DumpFormat::SqlGz => args.extend(["--format=plain".into(), "--compress=6".into()]),
                _ => args.push("--format=plain".into()),
            }
            args.push(format!("--file={}", part.display()));
        }
        Engine::Mysql => {
            // A consistent snapshot of InnoDB tables without locking them.
            args.extend([
                "--single-transaction".into(),
                "--no-tablespaces".into(),
                "--triggers".into(),
                // Binary columns as hex literals, so the dump is valid text.
                "--hex-blob".into(),
            ]);
            // Routines belong to the database, not to tables: only a whole
            // database dump takes them.
            if plan.tables.is_empty() {
                args.push("--routines".into());
            }
            if tool.version.flavor == Flavor::Mysql {
                // GTID statements need SUPER on restore and are rarely wanted.
                args.push("--set-gtid-purged=OFF".into());
            }
            match plan.content {
                Content::SchemaAndData => {}
                Content::SchemaOnly => args.push("--no-data".into()),
                Content::DataOnly => args.push("--no-create-info".into()),
            }
            if !plan.drop_existing {
                args.push("--skip-add-drop-table".into());
            }
            args.push(plan.schema.clone());
            args.extend(plan.tables.iter().cloned());
        }
    }
    args
}

async fn dump_with_tool(
    plan: DumpPlan,
    tool: FoundTool,
    mut endpoint: Endpoint,
    password: Option<String>,
    reporter: Reporter,
    cancel: Arc<Notify>,
) -> JobEnd {
    endpoint.database = Some(plan.database.clone());
    let part = part_path(&plan.destination);
    let mut command = match ToolCommand::connect(&tool, &endpoint, password.as_deref()) {
        Ok(command) => command,
        Err(err) => return JobEnd::Failed(format!("could not prepare {}: {err}", tool.tool)),
    };
    command.args(dump_args(&plan, &tool, &part));
    if plan.engine == Engine::Mysql {
        command.stdout_file(&part);
    }
    let end = run_tool(command, plan.table_count, &reporter, cancel).await;
    if matches!(end, JobEnd::Succeeded(_))
        && plan.engine == Engine::Postgres
        && plan.format == DumpFormat::Sql
    {
        match fix_old_server_header(&part, false) {
            Ok(true) => {
                reporter
                    .log("Commented out `SET transaction_timeout`, which servers before 17 reject.")
                    .await
            }
            Ok(false) => {}
            Err(err) => return JobEnd::Failed(format!("{}: {err}", part.display())),
        }
    }
    finish_file(end, &part, &plan.destination)
}

/// pg_dump 17+ starts plain dumps with `SET transaction_timeout = 0;`,
/// which servers before 17 reject, so restoring with `ON_ERROR_STOP` fails
/// at once. When the dump's header says the source server is older, the
/// line is commented out in place (same length, so the file isn't
/// rewritten). `force` comments it out whatever the source server was, for
/// a script about to run on an older server. True if it was.
fn fix_old_server_header(path: &Path, force: bool) -> std::io::Result<bool> {
    use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
    const LINE: &[u8] = b"\nSET transaction_timeout = 0;\n";
    const COMMENTED: &[u8] = b"\n--  transaction_timeout = 0;\n";
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let mut head = vec![0; 8192];
    let n = file.read(&mut head)?;
    head.truncate(n);
    let text = String::from_utf8_lossy(&head);
    let old_server = text
        .lines()
        .find_map(|line| line.strip_prefix("-- Dumped from database version "))
        .and_then(|version| version.split(['.', ' ']).next()?.parse::<u32>().ok())
        .is_some_and(|major| major < 17)
        || force;
    let Some(at) = head
        .windows(LINE.len())
        .position(|w| w == LINE)
        .filter(|_| old_server)
    else {
        return Ok(false);
    };
    file.seek(SeekFrom::Start(at as u64))?;
    file.write_all(COMMENTED)?;
    Ok(true)
}

async fn restore_with_tool(
    plan: RestorePlan,
    tool: FoundTool,
    mut endpoint: Endpoint,
    password: Option<String>,
    reporter: Reporter,
    cancel: Arc<Notify>,
) -> JobEnd {
    let new_tool_old_server =
        tool.version.version.major >= 17 && plan.server_major.is_some_and(|major| major < 17);
    if matches!(plan.source, RestoreSource::PgArchive) && new_tool_old_server {
        return Box::pin(restore_archive_via_psql(
            plan, tool, endpoint, password, reporter, cancel,
        ))
        .await;
    }
    endpoint.database = Some(plan.database.clone());
    let mut command = match ToolCommand::connect(&tool, &endpoint, password.as_deref()) {
        Ok(command) => command,
        Err(err) => return JobEnd::Failed(format!("could not prepare {}: {err}", tool.tool)),
    };
    let stop = plan.options.on_error == OnError::Stop || plan.options.single_transaction;
    match (&plan.source, tool.tool) {
        (RestoreSource::PgArchive, Tool::PgRestore) => {
            command.arg("--verbose");
            if plan.drop_existing {
                command.args(["--clean", "--if-exists"]);
            }
            if plan.skip_owners {
                command.args(["--no-owner", "--no-privileges"]);
            }
            if plan.options.single_transaction {
                command.arg("--single-transaction");
            }
            if stop {
                command.arg("--exit-on-error");
            }
            command.arg(&plan.path);
        }
        (RestoreSource::Sql, Tool::Psql) => {
            if stop {
                command.arg("--set=ON_ERROR_STOP=1");
            }
            if plan.options.single_transaction {
                command.arg("--single-transaction");
            }
            command.arg("--file").arg(&plan.path);
        }
        (RestoreSource::Sql, Tool::Mysql) => {
            if !stop {
                command.arg("--force");
            }
            command.stdin_file(&plan.path);
        }
        (source, tool) => {
            return JobEnd::Failed(format!("{tool} can't restore {source:?}"));
        }
    }
    run_tool(command, 0, &reporter, cancel).await
}

/// pg_restore 17+ starts every session with `SET transaction_timeout`,
/// which servers before 17 reject, so it can neither stop on the first error
/// nor restore in one transaction there. Instead the archive becomes a
/// script (no connection needed), that line is commented out, and psql
/// runs it with the user's options.
async fn restore_archive_via_psql(
    plan: RestorePlan,
    tool: FoundTool,
    endpoint: Endpoint,
    password: Option<String>,
    reporter: Reporter,
    cancel: Arc<Notify>,
) -> JobEnd {
    let psql = FoundTool {
        tool: Tool::Psql,
        path: tool
            .path
            .with_file_name(format!("psql{}", std::env::consts::EXE_SUFFIX)),
        version: tool.version,
    };
    if !psql.path.is_file() {
        return JobEnd::Failed(format!(
            "pg_restore {} can't restore into PostgreSQL {} directly, and psql wasn't found next to it",
            tool.version.version.major,
            plan.server_major.unwrap_or_default()
        ));
    }
    reporter
        .log(format!(
            "pg_restore {} can't restore into PostgreSQL {} directly; converting the archive to SQL for psql.",
            tool.version.version.major,
            plan.server_major.unwrap_or_default()
        ))
        .await;
    let script = std::env::temp_dir().join(format!("savoia-restore-{}.sql", uuid::Uuid::new_v4()));
    let mut convert = ToolCommand::local(&tool);
    convert.arg(format!("--file={}", script.display()));
    if plan.drop_existing {
        convert.args(["--clean", "--if-exists"]);
    }
    if plan.skip_owners {
        convert.args(["--no-owner", "--no-privileges"]);
    }
    convert.arg(&plan.path);
    let end = run_tool(convert, 0, &reporter, cancel.clone()).await;
    if !matches!(end, JobEnd::Succeeded(_)) {
        let _ = std::fs::remove_file(&script);
        return end;
    }
    if let Err(err) = fix_old_server_header(&script, true) {
        let _ = std::fs::remove_file(&script);
        return JobEnd::Failed(format!("{}: {err}", script.display()));
    }
    let as_script = RestorePlan {
        source: RestoreSource::Sql,
        path: script.clone(),
        ..plan
    };
    let end = Box::pin(restore_with_tool(
        as_script, psql, endpoint, password, reporter, cancel,
    ))
    .await;
    let _ = std::fs::remove_file(&script);
    end
}

/// Runs `command`, relaying its log and table progress.
async fn run_tool(
    command: ToolCommand,
    table_count: usize,
    reporter: &Reporter,
    cancel: Arc<Notify>,
) -> JobEnd {
    let mut run = match command.spawn() {
        Ok(run) => run,
        Err(err) => return JobEnd::Failed(format!("could not start the tool: {err}")),
    };
    let kill = run.cancel_handle();
    let mut tables = 0usize;
    loop {
        let event = tokio::select! {
            event = run.next() => event,
            () = cancel.notified() => {
                let _ = kill.cancel().await;
                continue;
            }
        };
        match event {
            Some(RunEvent::Log(line)) => reporter.log(line).await,
            Some(RunEvent::Table(table)) => {
                tables += 1;
                let fraction = (table_count > 0).then(|| tables as f32 / table_count as f32);
                reporter.progress(fraction, table).await;
            }
            Some(RunEvent::Finished(end)) => {
                return match end {
                    RunEnd::Succeeded => JobEnd::Succeeded("Finished.".into()),
                    RunEnd::Failed(message) => JobEnd::Failed(message),
                    RunEnd::Cancelled => JobEnd::Cancelled,
                };
            }
            None => return JobEnd::Failed("the tool's runner stopped unexpectedly".into()),
        }
    }
}

async fn dump_built_in(
    plan: DumpPlan,
    conn: Arc<dyn Connection>,
    reporter: Reporter,
    cancel: Arc<Notify>,
) -> JobEnd {
    let format = match plan.format {
        DumpFormat::Sql => OutputFormat::Sql,
        DumpFormat::SqlGz => OutputFormat::SqlGz,
        DumpFormat::Csv => OutputFormat::Csv,
        DumpFormat::PgArchive => {
            return JobEnd::Failed(
                "pg_dump archives need pg_dump; the built-in engine writes SQL or CSV".into(),
            );
        }
    };
    let request = ExportRequest {
        schema: plan.schema.clone(),
        tables: plan.tables.clone(),
        ddl: plan.content != Content::DataOnly,
        data: plan.content != Content::SchemaOnly,
        drop_existing: plan.drop_existing,
        format,
        destination: plan.destination.clone(),
    };
    reporter
        .log(format!(
            "Built-in export of {} to {} (lower fidelity: no routines, triggers or grants)",
            plan.schema,
            plan.destination.display()
        ))
        .await;

    // Progress events come from inside the export; relay them through a
    // channel so the export itself stays synchronous about reporting.
    let (tx, mut rx) = mpsc::unbounded_channel();
    let export = builtin::export(&*conn, plan.engine, &request, move |event| {
        let _ = tx.send(event);
    });
    tokio::pin!(export);
    let (mut tables, mut current) = (0usize, String::new());
    let result = loop {
        tokio::select! {
            result = &mut export => break result,
            Some(event) = rx.recv() => match event {
                ExportEvent::Table(table) => {
                    tables += 1;
                    reporter.log(format!("Exporting {table}")).await;
                    current = table;
                    let fraction = (plan.table_count > 0)
                        .then(|| (tables - 1) as f32 / plan.table_count as f32);
                    reporter.progress(fraction, current.clone()).await;
                }
                ExportEvent::Rows(rows) => {
                    let fraction = (plan.table_count > 0)
                        .then(|| (tables.saturating_sub(1)) as f32 / plan.table_count as f32);
                    reporter.progress(fraction, format!("{current}: {rows} rows")).await;
                }
            },
            () = cancel.notified() => return JobEnd::Cancelled,
        }
    };
    match result {
        Ok(summary) => {
            for skipped in &summary.skipped {
                reporter
                    .log(format!(
                        "Skipped {skipped}: not supported by the built-in exporter"
                    ))
                    .await;
            }
            reporter.progress(Some(1.), "Done").await;
            JobEnd::Succeeded(format!(
                "Exported {} table{} and {} row{}.",
                summary.tables,
                plural(summary.tables as u64),
                summary.rows,
                plural(summary.rows)
            ))
        }
        Err(err) => JobEnd::Failed(err.to_string()),
    }
}

async fn restore_built_in(
    plan: RestorePlan,
    conn: Arc<dyn Connection>,
    reporter: Reporter,
    cancel: Arc<Notify>,
) -> JobEnd {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let progress = move |event| {
        let _ = tx.send(event);
    };
    let unit = match plan.source {
        RestoreSource::Csv { .. } => "row",
        _ => "statement",
    };
    let import = async {
        match &plan.source {
            RestoreSource::Sql => {
                let request = SqlImport {
                    path: plan.path.clone(),
                    options: plan.options,
                };
                crate::import::import_sql(&*conn, plan.engine, &request, progress).await
            }
            RestoreSource::Csv {
                csv,
                table,
                mapping,
            } => {
                let request = CsvImport {
                    path: plan.path.clone(),
                    csv: *csv,
                    schema: plan.schema.clone(),
                    table: table.clone(),
                    mapping: mapping.clone(),
                    options: plan.options,
                };
                crate::import::import_csv(&*conn, plan.engine, &request, progress).await
            }
            RestoreSource::PgArchive => Err(AppError::invalid(
                "pg_dump archives can only be restored with pg_restore; install the PostgreSQL client tools",
            )),
        }
    };
    tokio::pin!(import);
    reporter
        .log(format!("Built-in import of {}", plan.path.display()))
        .await;
    let result = loop {
        tokio::select! {
            result = &mut import => break result,
            Some(event) = rx.recv() => match event {
                ImportEvent::Progress { read, total, done } => {
                    let fraction = (total > 0).then(|| read as f32 / total as f32);
                    reporter.progress(fraction, format!("{done} {unit}{}", plural(done))).await;
                }
                ImportEvent::Error(error) => {
                    reporter
                        .log(format!("line {}: {}\n  {}", error.line, error.message, error.what))
                        .await;
                }
            },
            () = cancel.notified() => return JobEnd::Cancelled,
        }
    };
    // Errors reported after the import future finished.
    while let Ok(ImportEvent::Error(error)) = rx.try_recv() {
        reporter
            .log(format!(
                "line {}: {}\n  {}",
                error.line, error.message, error.what
            ))
            .await;
    }
    match result {
        Ok(summary) => import_end(&summary, unit),
        Err(err) => JobEnd::Failed(err.to_string()),
    }
}

fn import_end(summary: &ImportSummary, unit: &str) -> JobEnd {
    let done = format!("{} {unit}{} imported", summary.done, plural(summary.done));
    if summary.rolled_back {
        return JobEnd::Failed(format!(
            "Rolled back after {} error{}; nothing was kept.",
            summary.failed,
            plural(summary.failed)
        ));
    }
    if summary.failed == 0 {
        return JobEnd::Succeeded(format!("{done}."));
    }
    let failed = format!("{} failed", summary.failed);
    if summary.stopped {
        JobEnd::Failed(format!("Stopped at the first error: {done}, {failed}."))
    } else {
        JobEnd::Partial(format!("{done}, {failed}."))
    }
}

fn plural(n: u64) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn part_path(destination: &Path) -> PathBuf {
    let mut part = destination.as_os_str().to_owned();
    part.push(".part");
    PathBuf::from(part)
}

/// Moves a tool's output into place on success; removes it otherwise.
fn finish_file(end: JobEnd, part: &Path, destination: &Path) -> JobEnd {
    match end {
        JobEnd::Succeeded(_) => match std::fs::rename(part, destination) {
            Ok(()) => {
                let size = std::fs::metadata(destination).map(|m| m.len()).unwrap_or(0);
                JobEnd::Succeeded(format!(
                    "Wrote {} ({}).",
                    destination.display(),
                    human_size(size)
                ))
            }
            Err(err) => JobEnd::Failed(format!("{}: {err}", destination.display())),
        },
        other => {
            let _ = std::fs::remove_file(part);
            other
        }
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024. && unit + 1 < UNITS.len() {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ToolVersion, Version};

    fn tool(tool: Tool, flavor: Flavor) -> FoundTool {
        FoundTool {
            tool,
            path: PathBuf::from(tool.name()),
            version: ToolVersion {
                flavor,
                version: Version {
                    major: 17,
                    minor: 0,
                    patch: 0,
                },
            },
        }
    }

    fn plan(engine: Engine) -> DumpPlan {
        DumpPlan {
            engine,
            database: "shop".into(),
            schema: "Sales".into(),
            tables: Vec::new(),
            table_count: 0,
            content: Content::SchemaAndData,
            drop_existing: false,
            skip_owners: false,
            format: DumpFormat::Sql,
            destination: PathBuf::from("/tmp/out.sql"),
        }
    }

    #[test]
    fn pg_dump_args() {
        let pg = tool(Tool::PgDump, Flavor::Postgres);
        let part = Path::new("/tmp/out.sql.part");
        assert_eq!(
            dump_args(&plan(Engine::Postgres), &pg, part),
            [
                "--verbose",
                "--schema=\"Sales\"",
                "--format=plain",
                "--file=/tmp/out.sql.part"
            ]
        );
        let mut picked = plan(Engine::Postgres);
        picked.tables = vec!["Order \"x\"".into()];
        picked.content = Content::SchemaOnly;
        picked.drop_existing = true;
        picked.skip_owners = true;
        picked.format = DumpFormat::PgArchive;
        assert_eq!(
            dump_args(&picked, &pg, part),
            [
                "--verbose",
                "--table=\"Sales\".\"Order \"\"x\"\"\"",
                "--schema-only",
                "--clean",
                "--if-exists",
                "--no-owner",
                "--no-privileges",
                "--format=custom",
                "--file=/tmp/out.sql.part"
            ]
        );
    }

    #[test]
    fn mysqldump_args() {
        let mut picked = plan(Engine::Mysql);
        picked.schema = "shop".into();
        picked.tables = vec!["orders".into()];
        picked.content = Content::DataOnly;
        let args = dump_args(
            &picked,
            &tool(Tool::Mysqldump, Flavor::Mysql),
            Path::new("x"),
        );
        assert_eq!(
            args,
            [
                "--verbose",
                "--single-transaction",
                "--no-tablespaces",
                "--triggers",
                "--hex-blob",
                "--set-gtid-purged=OFF",
                "--no-create-info",
                "--skip-add-drop-table",
                "shop",
                "orders"
            ]
        );
        let mariadb = dump_args(
            &picked,
            &tool(Tool::Mysqldump, Flavor::Mariadb),
            Path::new("x"),
        );
        assert!(!mariadb.iter().any(|a| a.contains("gtid")));
    }

    #[test]
    fn comments_out_transaction_timeout_for_old_servers_only() {
        let dump = |version: &str| {
            let path =
                std::env::temp_dir().join(format!("savoia-header-{}.sql", uuid::Uuid::new_v4()));
            std::fs::write(
                &path,
                format!("--\n-- Dumped from database version {version}\n\nSET statement_timeout = 0;\nSET transaction_timeout = 0;\nSET x = 1;\n"),
            )
            .unwrap();
            path
        };
        let old = dump("13.22 (Debian 13.22-1)");
        assert!(fix_old_server_header(&old, false).unwrap());
        let text = std::fs::read_to_string(&old).unwrap();
        assert!(
            text.contains("\n--  transaction_timeout = 0;\nSET x = 1;\n"),
            "{text}"
        );
        let new = dump("17.2");
        assert!(!fix_old_server_header(&new, false).unwrap());
        assert!(
            std::fs::read_to_string(&new)
                .unwrap()
                .contains("\nSET transaction_timeout = 0;\n")
        );
        assert!(fix_old_server_header(&new, true).unwrap());
    }

    #[test]
    fn guesses_sources_from_names() {
        assert!(matches!(
            RestoreSource::guess(Path::new("a.CSV")),
            RestoreSource::Csv { .. }
        ));
        assert!(matches!(
            RestoreSource::guess(Path::new("a.dump")),
            RestoreSource::PgArchive
        ));
        assert!(matches!(
            RestoreSource::guess(Path::new("a.sql.gz")),
            RestoreSource::Sql
        ));
    }

    #[test]
    fn import_ends_read_well() {
        let summary = ImportSummary {
            done: 3,
            failed: 1,
            ..ImportSummary::default()
        };
        assert_eq!(
            import_end(&summary, "row"),
            JobEnd::Partial("3 rows imported, 1 failed.".into())
        );
        assert_eq!(human_size(1536), "1.5 KB");
    }
}
