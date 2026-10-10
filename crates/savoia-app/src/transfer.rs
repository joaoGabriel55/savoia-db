//! The dump and import wizard: a tab that walks through what to transfer,
//! how, and where, then shows the job's progress and log. Jobs run on the
//! session's endpoint, so through its SSH tunnel when it has one, with the
//! session kept alive until they end.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::radio::Radio;
use gpui_kit::component::select::{SearchableVec, Select, SelectState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, IndexPath, Sizable as _, StyledExt as _,
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_core::{ColumnInfo, Engine};
use savoia_transfer::import::{
    CsvOptions, CsvPreview, ImportOptions, OnError, auto_map, preview_csv,
};
pub use savoia_transfer::job::JobEnd;
use savoia_transfer::job::{
    self, Content, DumpFormat, DumpPlan, Job, JobEvent, RestorePlan, RestoreSource, Runner,
};
use savoia_transfer::tools::{
    Compatibility, FoundTool, Tool, ToolSearch, check, detect, parse_server_version,
};

use crate::data_sources::DataSources;
use crate::explorer::NodeRef;
use crate::session::{self, Session};
use crate::{runtime, theme};

/// The preference holding the directory the user picked for dump tools.
pub const TOOLS_DIR_SETTING: &str = "transfer.tools_dir";
/// Log lines kept on screen.
const LOG_LINES: usize = 2000;

type Choice = SelectState<SearchableVec<String>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Export,
    Import,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Objects,
    Source,
    Options,
    Destination,
    Run,
}

impl Step {
    fn label(self) -> &'static str {
        match self {
            Step::Objects => "Objects",
            Step::Source => "File",
            Step::Options => "Options",
            Step::Destination => "Destination",
            Step::Run => "Progress",
        }
    }
}

/// What a tool's detection found.
#[derive(Clone)]
struct ToolStatus {
    tool: Tool,
    found: Option<FoundTool>,
    compatibility: Compatibility,
}

impl ToolStatus {
    fn usable(&self) -> bool {
        self.found.is_some() && !matches!(self.compatibility, Compatibility::Incompatible(_))
    }
}

/// The kind of file an import reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Sql,
    PgArchive,
    Csv,
}

struct Object {
    name: String,
    view: bool,
    checked: bool,
}

enum RunState {
    Idle,
    Running {
        fraction: Option<f32>,
        detail: String,
    },
    Done(JobEnd),
}

pub struct TransferView {
    data_sources: Entity<DataSources>,
    node: NodeRef,
    engine: Engine,
    direction: Direction,
    read_only: bool,
    step: Step,
    error: Option<String>,

    tools: Option<Vec<ToolStatus>>,
    tools_dir: Entity<InputState>,
    /// Use the external tool rather than the built-in engine.
    use_tool: bool,

    // Export.
    objects: Option<Vec<Object>>,
    format: DumpFormat,
    content: Content,
    drop_existing: bool,
    skip_owners: bool,
    destination: Entity<InputState>,

    // Import.
    source_path: Entity<InputState>,
    source_kind: SourceKind,
    /// The source is gzip-compressed, which only the built-in engine reads.
    source_gz: bool,
    continue_on_error: bool,
    single_transaction: bool,
    csv: CsvOptions,
    csv_table: Entity<Choice>,
    csv_tables: Vec<String>,
    csv_columns: Vec<ColumnInfo>,
    csv_preview: Option<CsvPreview>,
    /// Per CSV column: the table column it fills, as a choice of
    /// "(skip)" followed by the table's columns.
    csv_mapping: Vec<Entity<Choice>>,

    run: RunState,
    log: Vec<String>,
    /// The output of a finished export, for "Show in Finder".
    output: Option<PathBuf>,
    /// Kept while a job runs: its tunnel must outlive the job.
    session: Option<Arc<Session>>,
    _task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

fn choice(
    items: Vec<String>,
    selected: usize,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Choice> {
    cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(items),
            Some(IndexPath::new(selected)),
            window,
            cx,
        )
    })
}

fn selected(choice: &Entity<Choice>, cx: &App) -> usize {
    choice.read(cx).selected_index(cx).map_or(0, |ix| ix.row)
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}

impl TransferView {
    /// A wizard over `node`'s schema; with a table in `node`, it starts with
    /// just that table (export) or that table as the CSV target (import).
    pub fn new(
        data_sources: Entity<DataSources>,
        node: NodeRef,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let config = data_sources.read(cx).get(node.connection).cloned();
        let engine = config.as_ref().map_or(Engine::Postgres, |c| c.engine);
        let read_only = config.as_ref().is_some_and(|c| c.read_only);
        let tools_dir_value = data_sources
            .read(cx)
            .setting(TOOLS_DIR_SETTING)
            .unwrap_or_default();
        let tools_dir = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search PATH and the usual install folders")
                .default_value(tools_dir_value)
        });
        let name = node.table.clone().unwrap_or_else(|| node.schema.clone());
        let destination = cx.new(|cx| {
            InputState::new(window, cx).default_value(
                home()
                    .join(format!("{name}.{}", DumpFormat::Sql.extension()))
                    .display()
                    .to_string(),
            )
        });
        let source_path = cx.new(|cx| {
            InputState::new(window, cx).placeholder("A .sql, .sql.gz, .dump or .csv file")
        });
        let csv_table = choice(Vec::new(), 0, window, cx);
        let subscriptions = vec![
            cx.subscribe_in(
                &csv_table,
                window,
                |this,
                 _,
                 _: &gpui_kit::component::select::SelectEvent<SearchableVec<String>>,
                 window,
                 cx| {
                    this.load_csv_columns(window, cx);
                },
            ),
            cx.subscribe_in(
                &source_path,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.source_changed(window, cx);
                    }
                },
            ),
        ];
        let mut this = Self {
            data_sources,
            engine,
            direction,
            read_only,
            step: match direction {
                Direction::Export => Step::Objects,
                Direction::Import => Step::Source,
            },
            error: None,
            tools: None,
            tools_dir,
            use_tool: true,
            objects: None,
            format: DumpFormat::Sql,
            content: Content::SchemaAndData,
            drop_existing: false,
            skip_owners: true,
            destination,
            source_path,
            source_kind: SourceKind::Sql,
            source_gz: false,
            continue_on_error: false,
            single_transaction: false,
            csv: CsvOptions::default(),
            csv_table,
            csv_tables: Vec::new(),
            csv_columns: Vec::new(),
            csv_preview: None,
            csv_mapping: Vec::new(),
            run: RunState::Idle,
            log: Vec::new(),
            output: None,
            session: None,
            _task: None,
            _subscriptions: subscriptions,
            node,
        };
        this.load_objects(window, cx);
        this.detect_tools(cx);
        this
    }

    pub fn title(&self) -> String {
        let what = self.node.table.as_deref().unwrap_or(&self.node.schema);
        match self.direction {
            Direction::Export => format!("Export {what}"),
            Direction::Import => format!("Import into {what}"),
        }
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }

    pub fn connection(&self) -> savoia_core::ConnectionId {
        self.node.connection
    }

    /// Whether a job is running, so closing the tab should cancel it.
    pub fn is_running(&self) -> bool {
        matches!(self.run, RunState::Running { .. })
    }

    /// Objects (for an export) and tools are loaded.
    #[cfg(test)]
    pub fn ready_for_test(&self) -> bool {
        self.tools.is_some() && (self.direction == Direction::Import || self.objects.is_some())
    }

    /// Types `value` into the destination (export) or source (import) field.
    #[cfg(test)]
    pub fn set_path_for_test(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = match self.direction {
            Direction::Export => self.destination.clone(),
            Direction::Import => self.source_path.clone(),
        };
        input.update(cx, |input, cx| {
            input.set_value(value.to_owned(), window, cx)
        });
        if self.direction == Direction::Import {
            self.source_changed(window, cx);
        }
    }

    /// The table column index each CSV column maps to (0 = skipped).
    #[cfg(test)]
    pub fn mapping_for_test(&self, cx: &App) -> Vec<usize> {
        self.csv_mapping.iter().map(|m| selected(m, cx)).collect()
    }

    #[cfg(test)]
    pub fn state_for_test(&self) -> (Option<JobEnd>, Vec<String>) {
        let end = match &self.run {
            RunState::Done(end) => Some(end.clone()),
            _ => None,
        };
        (end, self.log.clone())
    }

    fn steps(&self) -> &'static [Step] {
        match self.direction {
            Direction::Export => &[Step::Objects, Step::Options, Step::Destination, Step::Run],
            Direction::Import => &[Step::Source, Step::Options, Step::Run],
        }
    }

    fn session(&self, cx: &App) -> Option<Arc<Session>> {
        self.data_sources.read(cx).session(self.node.connection)
    }

    /// Loads the tables and views of the schema afresh: the catalog may
    /// predate tables created since.
    fn load_objects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session(cx) else {
            self.error = Some("The data source is not connected.".into());
            return;
        };
        let (database, schema) = (self.node.database.clone(), self.node.schema.clone());
        let io = runtime::spawn(async move { session.load_objects(&database, &schema).await });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = session::join(io).await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(objects) => this.set_objects(objects, window, cx),
                Err(err) => {
                    this.error = Some(err.to_string());
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn set_objects(
        &mut self,
        objects: savoia_core::SchemaObjects,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let only = self.node.table.clone();
        let list: Vec<Object> = objects
            .tables
            .iter()
            .map(|name| (name, false))
            .chain(objects.views.iter().map(|name| (name, true)))
            .map(|(name, view)| Object {
                name: name.clone(),
                view,
                checked: only.as_ref().is_none_or(|t| t == name),
            })
            .collect();
        self.csv_tables = objects.tables.clone();
        let target = only
            .as_ref()
            .and_then(|t| self.csv_tables.iter().position(|name| name == t))
            .unwrap_or(0);
        self.csv_table = choice(self.csv_tables.clone(), target, window, cx);
        self._subscriptions.push(cx.subscribe_in(
            &self.csv_table,
            window,
            |this,
             _,
             _: &gpui_kit::component::select::SelectEvent<SearchableVec<String>>,
             window,
             cx| {
                this.load_csv_columns(window, cx);
            },
        ));
        self.objects = Some(list);
        cx.notify();
    }

    /// Finds the tools for this engine and checks them against the server.
    fn detect_tools(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session(cx) else {
            return;
        };
        let server = parse_server_version(&session.catalog().server);
        let dir = self.tools_dir.read(cx).value().trim().to_owned();
        let custom = (!dir.is_empty()).then(|| PathBuf::from(&dir));
        let engine = self.engine;
        self.tools = None;
        let io = runtime::spawn_blocking(move || {
            let search = ToolSearch::system(custom);
            Tool::for_engine(engine)
                .iter()
                .map(|&tool| {
                    let found = detect(tool, &search);
                    let compatibility = match (&found, server) {
                        (Some(found), Some(server)) => check(tool, found.version, server),
                        (Some(_), None) => Compatibility::Compatible,
                        (None, _) => Compatibility::Incompatible(format!("{tool} was not found.")),
                    };
                    ToolStatus {
                        tool,
                        found,
                        compatibility,
                    }
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let Ok(tools) = io.await else { return };
            this.update(cx, |this, cx| {
                this.tools = Some(tools);
                this.use_tool = this.tool_for_job().is_some_and(|t| t.usable());
                this.fit_format();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The tool this wizard's job would run.
    fn wanted_tool(&self) -> Option<Tool> {
        match (self.direction, self.engine, self.source_kind) {
            (Direction::Export, Engine::Postgres, _) => Some(Tool::PgDump),
            (Direction::Export, Engine::Mysql, _) => Some(Tool::Mysqldump),
            (Direction::Import, _, SourceKind::Csv) => None,
            (Direction::Import, Engine::Postgres, SourceKind::PgArchive) => Some(Tool::PgRestore),
            (Direction::Import, Engine::Postgres, SourceKind::Sql) => Some(Tool::Psql),
            (Direction::Import, Engine::Mysql, _) => Some(Tool::Mysql),
        }
    }

    fn tool_for_job(&self) -> Option<&ToolStatus> {
        let wanted = self.wanted_tool()?;
        self.tools.as_ref()?.iter().find(|t| t.tool == wanted)
    }

    /// Whether the job will run the external tool.
    fn runs_tool(&self) -> bool {
        self.use_tool && self.tool_for_job().is_some_and(|t| t.usable()) && !self.needs_built_in()
    }

    /// Cases only the built-in engine handles.
    fn needs_built_in(&self) -> bool {
        match self.direction {
            Direction::Export => false,
            Direction::Import => self.source_kind == SourceKind::Csv || self.source_gz,
        }
    }

    fn source_path_text(&self, cx: &App) -> String {
        self.source_path.read(cx).value().trim().to_owned()
    }

    /// Keeps the export format one the chosen engine can write.
    fn fit_format(&mut self) {
        let available = DumpFormat::available(self.engine, self.runs_tool());
        if !available.contains(&self.format) {
            self.format = available[0];
        }
    }

    fn set_format(&mut self, format: DumpFormat, window: &mut Window, cx: &mut Context<Self>) {
        self.format = format;
        // Follow the format in the file name the user hasn't changed by hand.
        let current = self.destination.read(cx).value().to_string();
        let path = Path::new(&current);
        let stem = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .map(|n| {
                ["sql.gz", "sql", "dump"]
                    .iter()
                    .find_map(|ext| n.strip_suffix(&format!(".{ext}")).map(str::to_owned))
                    .unwrap_or(n)
            })
            .unwrap_or_default();
        let name = match format.extension() {
            "" => stem,
            ext => format!("{stem}.{ext}"),
        };
        let next = path.with_file_name(name).display().to_string();
        self.destination
            .update(cx, |input, cx| input.set_value(next, window, cx));
        cx.notify();
    }

    fn save_tools_dir(&mut self, cx: &mut Context<Self>) {
        let dir = self.tools_dir.read(cx).value().trim().to_owned();
        self.data_sources.update(cx, |ds, _| {
            ds.set_setting(TOOLS_DIR_SETTING, (!dir.is_empty()).then_some(dir.as_str()))
        });
        self.detect_tools(cx);
    }

    fn choose_destination(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = PathBuf::from(self.destination.read(cx).value().to_string());
        let dir = current.parent().map_or_else(home, Path::to_path_buf);
        if self.format == DumpFormat::Csv {
            let prompt = cx.prompt_for_paths(PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some("Export here".into()),
            });
            let paths = async move { prompt.await.ok()?.ok()?.and_then(|p| p.into_iter().next()) };
            self.await_path(paths, Target::Destination, window, cx);
        } else {
            let name = current.file_name().map(|n| n.to_string_lossy().to_string());
            let prompt = cx.prompt_for_new_path(&dir, name.as_deref());
            let path = async move { prompt.await.ok()?.ok()? };
            self.await_path(path, Target::Destination, window, cx);
        }
    }

    fn choose_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        let paths = async move { prompt.await.ok()?.ok()?.and_then(|p| p.into_iter().next()) };
        self.await_path(paths, Target::Source, window, cx);
    }

    /// Puts the path the dialog returns into the field it was opened for.
    fn await_path(
        &mut self,
        path: impl Future<Output = Option<PathBuf>> + 'static,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = path.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                let value = path.display().to_string();
                match target {
                    Target::Destination => this
                        .destination
                        .update(cx, |input, cx| input.set_value(value, window, cx)),
                    Target::Source => {
                        this.source_path
                            .update(cx, |input, cx| input.set_value(value, window, cx));
                        this.source_changed(window, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Picks the source kind from the file name and previews a CSV.
    fn source_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = PathBuf::from(self.source_path_text(cx));
        self.source_gz = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("gz"));
        self.source_kind = match RestoreSource::guess(&path) {
            RestoreSource::Csv { .. } => SourceKind::Csv,
            RestoreSource::PgArchive if self.engine == Engine::Postgres => SourceKind::PgArchive,
            _ => SourceKind::Sql,
        };
        self.use_tool = self.tool_for_job().is_some_and(|t| t.usable());
        if self.source_kind == SourceKind::Csv {
            self.preview_csv(window, cx);
        }
        cx.notify();
    }

    fn preview_csv(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = PathBuf::from(self.source_path_text(cx));
        match preview_csv(&path, self.csv, 5) {
            Ok(preview) => {
                self.csv_preview = Some(preview);
                self.error = None;
            }
            Err(err) => {
                self.csv_preview = None;
                self.error = Some(err.to_string());
            }
        }
        self.load_csv_columns(window, cx);
    }

    /// Loads the target table's columns, then maps CSV columns by name.
    fn load_csv_columns(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(table) = self.csv_tables.get(selected(&self.csv_table, cx)).cloned() else {
            return;
        };
        let Some(session) = self.session(cx) else {
            return;
        };
        let (database, schema) = (self.node.database.clone(), self.node.schema.clone());
        let io =
            runtime::spawn(async move { session.describe_table(&database, &schema, &table).await });
        cx.spawn_in(window, async move |this, cx| {
            let result = session::join(io).await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(info) => {
                        this.csv_columns = info.columns.clone();
                        this.remap(window, cx);
                    }
                    Err(err) => this.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn remap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = &self.csv_preview else {
            self.csv_mapping.clear();
            return;
        };
        let mapped = auto_map(&preview.headers, &self.csv_columns);
        let items: Vec<String> = std::iter::once("(skip)".to_owned())
            .chain(self.csv_columns.iter().map(|c| c.name.clone()))
            .collect();
        let headers = preview.headers.len();
        self.csv_mapping = (0..headers)
            .map(|source| {
                let target = mapped
                    .iter()
                    .find(|m| m.source == source)
                    .and_then(|m| self.csv_columns.iter().position(|c| c.name == m.target))
                    .map_or(0, |ix| ix + 1);
                choice(items.clone(), target, window, cx)
            })
            .collect();
    }

    fn can_advance(&self, cx: &App) -> Result<(), String> {
        match self.step {
            Step::Objects => match &self.objects {
                Some(objects) if objects.iter().any(|o| o.checked) => Ok(()),
                Some(_) => Err("Pick at least one table or view.".into()),
                None => Err("Loading the schema…".into()),
            },
            Step::Source => {
                let path = self.source_path_text(cx);
                if path.is_empty() {
                    Err("Choose a file to import.".into())
                } else if !Path::new(&path).is_file() {
                    Err(format!("{path} is not a file."))
                } else if self.read_only {
                    Err("This data source is read-only.".into())
                } else {
                    Ok(())
                }
            }
            Step::Options => {
                if self.direction == Direction::Import && self.source_kind == SourceKind::Csv {
                    if self.csv_tables.is_empty() {
                        return Err("The schema has no tables to import into.".into());
                    }
                    if !self.csv_mapping.iter().any(|m| selected(m, cx) > 0) {
                        return Err("Map at least one CSV column to a table column.".into());
                    }
                }
                if self.direction == Direction::Import
                    && self.source_kind == SourceKind::PgArchive
                    && !self.runs_tool()
                {
                    return Err("pg_dump archives need pg_restore, which wasn't found.".into());
                }
                Ok(())
            }
            Step::Destination => {
                let path = self.destination.read(cx).value().trim().to_owned();
                if path.is_empty() {
                    Err("Choose where to write the export.".into())
                } else {
                    Ok(())
                }
            }
            Step::Run => Err(String::new()),
        }
    }

    fn next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(message) = self.can_advance(cx) {
            self.error = Some(message);
            cx.notify();
            return;
        }
        self.error = None;
        let steps = self.steps();
        let ix = steps.iter().position(|s| *s == self.step).unwrap_or(0);
        self.step = steps[(ix + 1).min(steps.len() - 1)];
        if self.step == Step::Run {
            self.start(window, cx);
        }
        cx.notify();
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        let steps = self.steps();
        let ix = steps.iter().position(|s| *s == self.step).unwrap_or(0);
        self.step = steps[ix.saturating_sub(1)];
        self.error = None;
        self.run = RunState::Idle;
        cx.notify();
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        // Dropping the task drops the job, which cancels it.
        if self.is_running() {
            self._task = None;
            self.session = None;
            self.run = RunState::Done(JobEnd::Cancelled);
            self.log.push("Cancelled.".into());
            cx.notify();
        }
    }

    fn dump_plan(&self, cx: &App) -> DumpPlan {
        let objects = self.objects.as_deref().unwrap_or_default();
        let all = objects.iter().all(|o| o.checked);
        let tables = if all {
            Vec::new()
        } else {
            objects
                .iter()
                .filter(|o| o.checked)
                .map(|o| o.name.clone())
                .collect()
        };
        DumpPlan {
            engine: self.engine,
            database: self.node.database.clone(),
            schema: self.node.schema.clone(),
            tables,
            table_count: objects.iter().filter(|o| o.checked && !o.view).count(),
            content: self.content,
            drop_existing: self.drop_existing,
            skip_owners: self.skip_owners,
            format: self.format,
            destination: PathBuf::from(self.destination.read(cx).value().trim()),
        }
    }

    fn restore_plan(&self, cx: &App) -> RestorePlan {
        let source = match self.source_kind {
            SourceKind::Sql => RestoreSource::Sql,
            SourceKind::PgArchive => RestoreSource::PgArchive,
            SourceKind::Csv => RestoreSource::Csv {
                csv: self.csv,
                table: self
                    .csv_tables
                    .get(selected(&self.csv_table, cx))
                    .cloned()
                    .unwrap_or_default(),
                mapping: self
                    .csv_mapping
                    .iter()
                    .enumerate()
                    .filter_map(|(source, choice)| {
                        let ix = selected(choice, cx);
                        (ix > 0).then(|| savoia_transfer::import::ColumnMapping {
                            source,
                            target: self.csv_columns[ix - 1].name.clone(),
                        })
                    })
                    .collect(),
            },
        };
        RestorePlan {
            engine: self.engine,
            database: self.node.database.clone(),
            schema: self.node.schema.clone(),
            path: PathBuf::from(self.source_path_text(cx)),
            source,
            options: ImportOptions {
                on_error: if self.continue_on_error {
                    OnError::Continue
                } else {
                    OnError::Stop
                },
                single_transaction: self.single_transaction,
            },
            drop_existing: self.drop_existing,
            skip_owners: self.skip_owners,
            server_major: self
                .session(cx)
                .and_then(|s| parse_server_version(&s.catalog().server))
                .map(|v| v.version.major),
        }
    }

    /// Starts the job on the I/O runtime and follows its events.
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session(cx) else {
            self.run = RunState::Done(JobEnd::Failed("The data source is not connected.".into()));
            return;
        };
        self.log.clear();
        self.output = None;
        self.run = RunState::Running {
            fraction: None,
            detail: "Starting…".into(),
        };
        let tool = self
            .runs_tool()
            .then(|| self.tool_for_job().and_then(|t| t.found.clone()))
            .flatten();
        let database = self.node.database.clone();
        let direction = self.direction;
        let (dump, restore) = match direction {
            Direction::Export => (Some(self.dump_plan(cx)), None),
            Direction::Import => (None, Some(self.restore_plan(cx))),
        };
        if let Some(plan) = &dump {
            self.output = Some(plan.destination.clone());
        }
        if let Some(ssh) = self
            .data_sources
            .read(cx)
            .get(self.node.connection)
            .and_then(|c| c.ssh.as_ref())
        {
            self.log.push(format!(
                "Through the SSH tunnel to {}@{}",
                ssh.user, ssh.host
            ));
        }
        self.session = Some(session.clone());
        let io = runtime::spawn(async move {
            let runner = match tool {
                Some(tool) => Runner::Tool {
                    tool,
                    endpoint: session.tool_endpoint(&database),
                    password: session.password(),
                },
                None => Runner::BuiltIn(session.open_dedicated(&database).await?),
            };
            Ok(match (dump, restore) {
                (Some(plan), _) => job::dump(plan, runner),
                (_, Some(plan)) => job::restore(plan, runner),
                _ => unreachable!("one plan is set"),
            })
        });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut job: Job = match session::join(io).await {
                Ok(job) => job,
                Err(err) => {
                    this.update(cx, |this, cx| {
                        this.finish(JobEnd::Failed(err.to_string()), cx)
                    })
                    .ok();
                    return;
                }
            };
            while let Some(event) = job.next().await {
                let alive = this
                    .update(cx, |this, cx| {
                        match event {
                            JobEvent::Log(line) => this.push_log(line),
                            JobEvent::Progress { fraction, detail } => {
                                this.run = RunState::Running { fraction, detail }
                            }
                            JobEvent::Finished(end) => this.finish(end, cx),
                        }
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn push_log(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > LOG_LINES {
            self.log.drain(..self.log.len() - LOG_LINES);
        }
    }

    fn finish(&mut self, end: JobEnd, cx: &mut Context<Self>) {
        if !matches!(end, JobEnd::Succeeded(_)) {
            self.output = None;
        }
        self.run = RunState::Done(end);
        self.session = None;
        // Imports change the schema; show it in the explorer.
        if self.direction == Direction::Import {
            let id = self.node.connection;
            self.data_sources.update(cx, |ds, cx| ds.refresh(id, cx));
        }
        cx.notify();
    }
}

#[derive(Clone, Copy)]
enum Target {
    Destination,
    Source,
}

/// A small caption above a group of controls.
fn caption(text: &'static str, cx: &App) -> Div {
    div()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

impl TransferView {
    fn render_steps(&self, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let current = self
            .steps()
            .iter()
            .position(|s| *s == self.step)
            .unwrap_or(0);
        h_flex()
            .gap_2()
            .text_sm()
            .children(self.steps().iter().enumerate().map(|(i, step)| {
                let color = if i == current {
                    theme.foreground
                } else {
                    theme.muted_foreground
                };
                h_flex()
                    .gap_1()
                    .text_color(color)
                    .when(i == current, |this| this.font_semibold())
                    .child(format!("{}. {}", i + 1, step.label()))
                    .when(i + 1 < self.steps().len(), |this| {
                        this.child(
                            Icon::new(IconName::ChevronRight)
                                .xsmall()
                                .text_color(theme.muted_foreground),
                        )
                    })
            }))
    }

    fn render_objects(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(objects) = &self.objects else {
            return div().child("Loading the schema…").into_any_element();
        };
        let checked = objects.iter().filter(|o| o.checked).count();
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(caption("TABLES AND VIEWS", cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{checked} of {} selected", objects.len())),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("select-all")
                            .xsmall()
                            .ghost()
                            .label("All")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(objects) = &mut this.objects {
                                    objects.iter_mut().for_each(|o| o.checked = true);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("select-none")
                            .xsmall()
                            .ghost()
                            .label("None")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(objects) = &mut this.objects {
                                    objects.iter_mut().for_each(|o| o.checked = false);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .id("objects")
                    .gap_1()
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .children(objects.iter().enumerate().map(|(i, object)| {
                        let icon = if object.view {
                            IconName::Eye
                        } else {
                            IconName::Inbox
                        };
                        h_flex()
                            .gap_2()
                            .child(
                                Checkbox::new(("object", i))
                                    .checked(object.checked)
                                    .label(object.name.clone())
                                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                        if let Some(o) =
                                            this.objects.as_mut().and_then(|o| o.get_mut(i))
                                        {
                                            o.checked = *checked;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .when(object.view, |this| {
                                this.child(
                                    Icon::new(icon)
                                        .xsmall()
                                        .text_color(cx.theme().muted_foreground),
                                )
                            })
                    })),
            )
            .into_any_element()
    }

    fn render_tool_choice(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let built_in_only = self.needs_built_in();
        let status = self.tool_for_job().cloned();
        let wanted = self.wanted_tool();
        let tool_label = match (&status, wanted) {
            (_, None) => None,
            (None, Some(tool)) => Some(format!("{tool} (detecting…)")),
            (
                Some(ToolStatus {
                    found: Some(found), ..
                }),
                _,
            ) => Some(format!(
                "{} {} — {}",
                found.tool,
                found.version.version,
                found.path.display()
            )),
            (Some(status), _) => Some(format!("{} — not found", status.tool)),
        };
        let note = status.as_ref().and_then(|s| match &s.compatibility {
            Compatibility::Compatible => None,
            Compatibility::Warning(m) => Some((m.clone(), theme.warning)),
            Compatibility::Incompatible(m) => Some((m.clone(), theme.danger)),
        });
        let usable = status.as_ref().is_some_and(ToolStatus::usable) && !built_in_only;
        let install = match self.engine {
            Engine::Postgres => "brew install libpq · apt install postgresql-client",
            Engine::Mysql => "brew install mysql-client · apt install mysql-client",
        };

        v_flex()
            .gap_2()
            .child(caption("ENGINE", cx))
            .when_some(tool_label, |this, label| {
                this.child(
                    Radio::new("use-tool")
                        .label(label)
                        .checked(self.runs_tool())
                        .disabled(!usable)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.use_tool = true;
                            this.fit_format();
                            cx.notify();
                        })),
                )
            })
            .child(
                Radio::new("use-built-in")
                    .label("Built-in engine (lower fidelity: no routines, triggers or grants)")
                    .checked(!self.runs_tool())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.use_tool = false;
                        this.fit_format();
                        cx.notify();
                    })),
            )
            .when_some(note, |this, (message, color)| {
                this.child(div().text_xs().text_color(color).child(message))
            })
            .when(!usable && wanted.is_some() && !built_in_only, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("To install the client tools: {install}")),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .items_end()
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(caption("TOOLS FOLDER", cx))
                            .child(Input::new(&self.tools_dir).small()),
                    )
                    .child(
                        Button::new("detect-tools")
                            .small()
                            .label("Detect again")
                            .on_click(cx.listener(|this, _, _, cx| this.save_tools_dir(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_export_options(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let formats = DumpFormat::available(self.engine, self.runs_tool());
        let contents = [
            (Content::SchemaAndData, "Structure and data"),
            (Content::SchemaOnly, "Structure only"),
            (Content::DataOnly, "Data only"),
        ];
        v_flex()
            .gap_4()
            .child(self.render_tool_choice(cx))
            .child(v_flex().gap_2().child(caption("FORMAT", cx)).children(
                formats.iter().enumerate().map(|(i, &format)| {
                    Radio::new(("format", i))
                        .label(format.label())
                        .checked(self.format == format)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.set_format(format, window, cx)
                        }))
                }),
            ))
            .child(
                v_flex().gap_2().child(caption("CONTENT", cx)).children(
                    contents
                        .into_iter()
                        .enumerate()
                        .map(|(i, (content, label))| {
                            Radio::new(("content", i))
                                .label(label)
                                .checked(self.content == content)
                                .disabled(
                                    self.format == DumpFormat::Csv
                                        && content == Content::SchemaOnly,
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.content = content;
                                    cx.notify();
                                }))
                        }),
                ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        Checkbox::new("drop-existing")
                            .label("Drop existing objects first (DROP … IF EXISTS)")
                            .checked(self.drop_existing)
                            .disabled(self.format == DumpFormat::Csv)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.drop_existing = *checked;
                                cx.notify();
                            })),
                    )
                    .when(
                        self.engine == Engine::Postgres && self.runs_tool(),
                        |this| {
                            this.child(
                                Checkbox::new("skip-owners")
                                    .label("Leave out owners and grants")
                                    .checked(self.skip_owners)
                                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                        this.skip_owners = *checked;
                                        cx.notify();
                                    })),
                            )
                        },
                    ),
            )
            .into_any_element()
    }

    fn render_destination(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let label = if self.format == DumpFormat::Csv {
            "FOLDER (one .csv per table)"
        } else {
            "FILE"
        };
        let plan = self.dump_plan(cx);
        let summary = format!(
            "{} {} from {} › {} as {}{}.",
            if plan.tables.is_empty() {
                "All".to_owned()
            } else {
                plan.tables.len().to_string()
            },
            if plan.tables.len() == 1 {
                "object"
            } else {
                "objects"
            },
            plan.database,
            plan.schema,
            plan.format.label(),
            if self.runs_tool() {
                format!(
                    " with {}",
                    self.wanted_tool().map(|t| t.name()).unwrap_or_default()
                )
            } else {
                " with the built-in engine".into()
            },
        );
        v_flex()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.destination).small()))
                    .child(
                        Button::new("choose-destination")
                            .small()
                            .label("Choose…")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_destination(window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(summary),
            )
            .into_any_element()
    }

    fn render_source(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let kinds: &[(SourceKind, &str)] = match self.engine {
            Engine::Postgres => &[
                (SourceKind::Sql, "SQL script (.sql, .sql.gz)"),
                (
                    SourceKind::PgArchive,
                    "pg_dump archive (.dump), restored with pg_restore",
                ),
                (SourceKind::Csv, "CSV into a table"),
            ],
            Engine::Mysql => &[
                (SourceKind::Sql, "SQL script (.sql, .sql.gz)"),
                (SourceKind::Csv, "CSV into a table"),
            ],
        };
        v_flex()
            .gap_3()
            .child(caption("FILE", cx))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.source_path).small()))
                    .child(
                        Button::new("choose-source")
                            .small()
                            .label("Choose…")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.choose_source(window, cx)),
                            ),
                    ),
            )
            .child(caption("IT HOLDS", cx))
            .children(kinds.iter().enumerate().map(|(i, &(kind, label))| {
                Radio::new(("source-kind", i))
                    .label(label)
                    .checked(self.source_kind == kind)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.source_kind = kind;
                        this.use_tool = this.tool_for_job().is_some_and(|t| t.usable());
                        if kind == SourceKind::Csv {
                            this.preview_csv(window, cx);
                        }
                        cx.notify();
                    }))
            }))
            .when(self.read_only, |this| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child("This data source is read-only; imports are disabled."),
                )
            })
            .into_any_element()
    }

    fn render_import_options(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let csv = self.source_kind == SourceKind::Csv;
        let archive = self.source_kind == SourceKind::PgArchive;
        let mysql_tool = self.engine == Engine::Mysql && self.runs_tool();
        v_flex()
            .gap_4()
            .when(!csv, |this| this.child(self.render_tool_choice(cx)))
            .when(csv, |this| this.child(self.render_csv(cx)))
            .child(
                v_flex()
                    .gap_2()
                    .child(caption("ON ERROR", cx))
                    .child(
                        Radio::new("on-error-stop")
                            .label("Stop at the first error")
                            .checked(!self.continue_on_error)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.continue_on_error = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Radio::new("on-error-continue")
                            .label("Skip failing statements and rows, and report them")
                            .checked(self.continue_on_error)
                            .disabled(self.single_transaction)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.continue_on_error = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        Checkbox::new("single-transaction")
                            .label("All or nothing: one transaction, rolled back on error")
                            .checked(self.single_transaction)
                            .disabled(mysql_tool)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.single_transaction = *checked;
                                if *checked {
                                    this.continue_on_error = false;
                                }
                                cx.notify();
                            })),
                    )
                    .when(archive, |this| {
                        this.child(
                            Checkbox::new("restore-clean")
                                .label("Drop existing objects first (--clean)")
                                .checked(self.drop_existing)
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.drop_existing = *checked;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Checkbox::new("restore-skip-owners")
                                .label("Leave out owners and grants")
                                .checked(self.skip_owners)
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.skip_owners = *checked;
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_csv(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let delimiters = [
            (b',', "Comma"),
            (b';', "Semicolon"),
            (b'\t', "Tab"),
            (b'|', "Pipe"),
        ];
        let mut body = v_flex()
            .gap_2()
            .child(caption("TARGET TABLE", cx))
            .child(
                div()
                    .w(px(320.))
                    .child(Select::new(&self.csv_table).small()),
            )
            .child(caption("CSV", cx))
            .child(
                h_flex()
                    .gap_3()
                    .children(
                        delimiters
                            .into_iter()
                            .enumerate()
                            .map(|(i, (byte, label))| {
                                Radio::new(("delimiter", i))
                                    .label(label)
                                    .checked(self.csv.delimiter == byte)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.csv.delimiter = byte;
                                        this.preview_csv(window, cx);
                                    }))
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_4()
                    .child(
                        Checkbox::new("csv-header")
                            .label("First row names the columns")
                            .checked(self.csv.has_header)
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                this.csv.has_header = *checked;
                                this.preview_csv(window, cx);
                            })),
                    )
                    .child(
                        Checkbox::new("csv-null")
                            .label("Empty unquoted fields are NULL")
                            .checked(self.csv.empty_is_null)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.csv.empty_is_null = *checked;
                                cx.notify();
                            })),
                    ),
            );
        if let Some(preview) = &self.csv_preview {
            body = body.child(caption("COLUMNS", cx)).children(
                preview.headers.iter().enumerate().map(|(i, header)| {
                    let sample: Vec<String> = preview
                        .rows
                        .iter()
                        .filter_map(|row| row.get(i).cloned())
                        .map(|v| v.unwrap_or_else(|| "NULL".into()))
                        .take(3)
                        .collect();
                    h_flex()
                        .gap_3()
                        .text_sm()
                        .child(
                            div()
                                .w(px(160.))
                                .truncate()
                                .font_semibold()
                                .child(header.clone()),
                        )
                        .child(
                            Icon::new(IconName::ArrowRight)
                                .xsmall()
                                .text_color(theme.muted_foreground),
                        )
                        .children(
                            self.csv_mapping
                                .get(i)
                                .map(|m| div().w(px(200.)).child(Select::new(m).small())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(sample.join(" · ")),
                        )
                }),
            );
        }
        body.into_any_element()
    }

    fn render_run(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (value, loading, detail, color) = match &self.run {
            RunState::Idle => (0., false, String::new(), theme.muted_foreground),
            RunState::Running { fraction, detail } => (
                fraction.unwrap_or(0.) * 100.,
                fraction.is_none(),
                detail.clone(),
                theme.muted_foreground,
            ),
            RunState::Done(JobEnd::Succeeded(message)) => {
                (100., false, message.clone(), theme.success)
            }
            RunState::Done(JobEnd::Partial(message)) => {
                (100., false, message.clone(), theme.warning)
            }
            RunState::Done(JobEnd::Failed(message)) => (0., false, message.clone(), theme.danger),
            RunState::Done(JobEnd::Cancelled) => {
                (0., false, "Cancelled.".into(), theme.muted_foreground)
            }
        };
        v_flex()
            .gap_2()
            .flex_1()
            .min_h_0()
            .child(
                Progress::new("transfer-progress")
                    .value(value)
                    .loading(loading),
            )
            .child(div().text_sm().text_color(color).child(detail))
            .child(caption("LOG", cx))
            .child(
                v_flex()
                    .id("transfer-log")
                    .flex_1()
                    .min_h(px(160.))
                    .p_2()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .overflow_y_scroll()
                    .font_family("monospace")
                    .text_xs()
                    .children(self.log.iter().map(|line| div().child(line.clone()))),
            )
            .into_any_element()
    }

    fn render_footer(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let first = self.steps().first() == Some(&self.step);
        let last_input = self.steps().iter().rev().nth(1) == Some(&self.step);
        let start_label = match self.direction {
            Direction::Export => "Export",
            Direction::Import => "Import",
        };
        h_flex()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(theme.border)
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_sm().text_color(theme.danger).child(error))
            })
            .child(div().flex_1())
            .map(|this| match &self.run {
                RunState::Running { .. } => this.child(
                    Button::new("transfer-cancel")
                        .small()
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                ),
                RunState::Done(_) => this
                    .when_some(self.output.clone(), |this, path| {
                        this.child(
                            Button::new("transfer-reveal")
                                .small()
                                .icon(Icon::new(Lucide::FolderOpen))
                                .label("Show in Finder")
                                .on_click(move |_, _, cx| cx.reveal_path(&path)),
                        )
                    })
                    .child(
                        Button::new("transfer-again")
                            .small()
                            .label("Back to options")
                            .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                    ),
                RunState::Idle => this
                    .child(
                        Button::new("transfer-back")
                            .small()
                            .ghost()
                            .label("Back")
                            .disabled(first)
                            .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                    )
                    .child(
                        Button::new("transfer-next")
                            .small()
                            .primary()
                            .label(if last_input { start_label } else { "Next" })
                            .on_click(cx.listener(|this, _, window, cx| this.next(window, cx))),
                    ),
            })
    }
}

impl Render for TransferView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let body = match self.step {
            Step::Objects => self.render_objects(cx),
            Step::Source => self.render_source(cx),
            Step::Options => match self.direction {
                Direction::Export => self.render_export_options(cx),
                Direction::Import => self.render_import_options(cx),
            },
            Step::Destination => self.render_destination(cx),
            Step::Run => self.render_run(cx),
        };
        let where_ = match self.engine {
            Engine::Postgres => format!("{} › {}", self.node.database, self.node.schema),
            Engine::Mysql => self.node.database.clone(),
        };
        v_flex()
            .size_full()
            .p_4()
            .gap_4()
            .bg(theme.background)
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        Icon::new(match self.direction {
                            Direction::Export => Lucide::Download,
                            Direction::Import => Lucide::Upload,
                        })
                        .text_color(theme::c(theme::IVREA_GREEN)),
                    )
                    .child(div().font_semibold().child(self.title()))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(where_),
                    )
                    .child(div().flex_1())
                    .child(self.render_steps(cx)),
            )
            .child(
                v_flex()
                    .id("transfer-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .child(self.render_footer(cx))
    }
}
