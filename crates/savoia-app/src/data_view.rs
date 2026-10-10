//! Data view: browse a table without writing SQL. Pages come from the
//! server ([`PAGE_SIZE`] rows each, loaded as the grid scrolls), header
//! clicks sort on the server, and filter chips become the `WHERE` clause.
//! The generated statement is always one click away.
//!
//! On tables with a key (and connections that aren't read-only), cells
//! edit on double-click, rows are added and deleted, and the pending
//! changes are reviewed as SQL and committed in one transaction. See
//! `docs/adr/202610091908-write-data-edits-as-generated-sql-in-one-previewed-transaction.md`.
//!
//! "+ Column" adds columns of related tables: lookups through foreign keys
//! and summaries of child rows, so a row stays one row of this table. The
//! row menu opens referenced or child rows in another view. See
//! `docs/adr/202610091908-build-joins-from-foreign-key-relationship-paths.md`.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::select::{SearchableVec, Select, SelectState};
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, IndexPath, Sizable as _, WindowExt as _,
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_core::data_query::{Agg, Filter, Hop, Op, PAGE_SIZE, Source, Summary, TableQuery};
use savoia_core::edit::EditTarget;
use savoia_core::{ConnectionId, Engine, TableInfo, TableKind};

use crate::data_grid::{self, DataRows, GridColumn, Request, Source as RowSource};
use crate::data_pickers::{ColumnPicker, JoinDialog, RowPicker, SummarizeDialog};
use crate::data_sources::DataSources;
use crate::explorer::NodeRef;
use crate::relations::{self, Link, Tables};
use crate::{runtime, session};

pub enum DataViewEvent {
    /// Put this SQL in a console on the view's data source.
    Sql {
        connection: ConnectionId,
        sql: String,
    },
    /// Open another table's rows, filtered: related rows of a row here.
    Open { node: NodeRef, filters: Vec<Filter> },
}

enum State {
    Loading,
    Failed(String),
    Ready {
        info: Arc<TableInfo>,
        grid: Entity<TableState<DataRows>>,
        query: Box<TableQuery>,
    },
}

type Choice = SelectState<SearchableVec<SharedString>>;

pub struct DataView {
    data_sources: Entity<DataSources>,
    /// Always has a table.
    node: NodeRef,
    state: State,
    /// Shown above the grid when a page fails to load or a commit fails.
    error: Option<String>,
    /// Why the view can't edit, when it can't.
    read_only: Option<&'static str>,
    committing: bool,
    filter_column: Option<Entity<Choice>>,
    filter_op: Entity<Choice>,
    filter_value: Entity<InputState>,
    /// Filters the view opens with.
    initial_filters: Vec<Filter>,
    /// Details of the tables around this one: the rest of its schema, and
    /// the tables its foreign keys reach.
    related: Tables,
    /// Relations the user added with "Join another table…".
    custom: Vec<Hop>,
    /// Related rows the row menu opens.
    links: Vec<Link>,
    _load: Option<Task<()>>,
    _related: Option<Task<()>>,
    _grid: Vec<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DataViewEvent> for DataView {}

impl DataView {
    pub fn new(
        data_sources: Entity<DataSources>,
        node: NodeRef,
        filters: Vec<Filter>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let ops = SearchableVec::new(
            Op::ALL
                .iter()
                .map(|op| op.label().into())
                .collect::<Vec<_>>(),
        );
        let filter_op = cx.new(|cx| SelectState::new(ops, Some(IndexPath::new(0)), window, cx));
        let filter_value = cx.new(|cx| InputState::new(window, cx).placeholder("value"));
        let subscriptions = vec![cx.subscribe_in(
            &filter_value,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.add_filter(window, cx);
                }
            },
        )];
        let mut this = Self {
            data_sources,
            node,
            state: State::Loading,
            error: None,
            read_only: None,
            committing: false,
            filter_column: None,
            filter_op,
            filter_value,
            initial_filters: filters,
            related: Tables::new(),
            custom: Vec::new(),
            links: Vec::new(),
            _load: None,
            _related: None,
            _grid: Vec::new(),
            _subscriptions: subscriptions,
        };
        this.load_table(window, cx);
        this
    }

    pub fn shows(&self, node: &NodeRef) -> bool {
        self.node == *node
    }

    pub fn title(&self) -> String {
        self.node.table.clone().unwrap_or_default()
    }

    pub fn connection(&self) -> ConnectionId {
        self.node.connection
    }

    #[cfg(test)]
    pub fn rows(&self, cx: &App) -> Option<(usize, bool)> {
        match &self.state {
            State::Ready { grid, .. } => {
                let rows = grid.read(cx).delegate();
                (!rows.loading).then(|| (rows.len(), rows.more))
            }
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn grid(&self) -> Option<Entity<TableState<DataRows>>> {
        match &self.state {
            State::Ready { grid, .. } => Some(grid.clone()),
            _ => None,
        }
    }

    pub fn query(&self) -> Option<&TableQuery> {
        match &self.state {
            State::Ready { query, .. } => Some(query),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn engine(&self, cx: &App) -> Engine {
        self.data_sources
            .read(cx)
            .get(self.node.connection)
            .map_or(Engine::Postgres, |c| c.engine)
    }

    /// Loads the table's columns and keys, then its first page.
    fn load_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let engine = self.engine(cx);
        let Some(session) = self.data_sources.read(cx).session(self.node.connection) else {
            self.state = State::Failed("The data source is not connected.".into());
            return;
        };
        if !session.runs_in(&self.node.database, engine) {
            self.state = State::Failed(format!(
                "Data views open in the database this data source connects to. \
                 Connect a data source to “{}” to browse its tables.",
                self.node.database
            ));
            return;
        }
        let (database, schema) = (self.node.database.clone(), self.node.schema.clone());
        let table = self.node.table.clone().unwrap_or_default();
        let cached = session.table(&database, &schema, &table);
        let io = runtime::spawn(async move {
            match cached {
                Some(info) => Ok(info),
                None => session.describe_table(&database, &schema, &table).await,
            }
        });
        self._load = Some(cx.spawn_in(window, async move |this, cx| {
            let result = session::join(io).await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(info) => this.ready(info, engine, window, cx),
                Err(err) => {
                    this.state = State::Failed(err.to_string());
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn ready(
        &mut self,
        info: Arc<TableInfo>,
        engine: Engine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = if info.primary_key.is_empty() {
            info.indexes
                .iter()
                .find(|i| {
                    i.unique
                        && i.columns
                            .iter()
                            .all(|c| info.columns.iter().any(|col| &col.name == c))
                })
                .map(|i| i.columns.clone())
                .unwrap_or_default()
        } else {
            info.primary_key.clone()
        };
        let query = TableQuery {
            engine,
            container: self.node.schema.clone(),
            table: info.name.clone(),
            columns: info
                .columns
                .iter()
                .map(|c| Source::Base(c.name.clone()))
                .collect(),
            filters: std::mem::take(&mut self.initial_filters),
            sort: None,
            key,
            summary: None,
        };
        let read_only_source = self
            .data_sources
            .read(cx)
            .get(self.node.connection)
            .is_some_and(|c| c.read_only);
        self.read_only = if read_only_source {
            Some("read-only connection")
        } else if info.kind == TableKind::View {
            Some("views are read-only")
        } else if query.key.is_empty() {
            Some("read-only: no primary or unique key")
        } else {
            None
        };
        let grid = self.new_grid(&info, &query, window, cx);
        self.state = State::Ready {
            info,
            grid,
            query: Box::new(query),
        };
        self.load_related(window, cx);
        self.reload(cx);
    }

    /// A grid for `query`'s columns, with the filter column choices to match.
    fn new_grid(
        &mut self,
        info: &TableInfo,
        query: &TableQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TableState<DataRows>> {
        let columns: Vec<GridColumn> = match &query.summary {
            Some(summary) => self.summary_columns(info, summary),
            None => query
                .columns
                .iter()
                .map(|source| self.grid_column(info, source))
                .collect(),
        };
        // Filters apply to rows, so they choose among the row columns even
        // while the view shows groups.
        let labels = SearchableVec::new(
            query
                .columns
                .iter()
                .map(|s| SharedString::from(s.label()))
                .collect::<Vec<_>>(),
        );
        self.filter_column =
            Some(cx.new(|cx| SelectState::new(labels, Some(IndexPath::new(0)), window, cx)));
        let mut rows = if query.summary.is_some() {
            DataRows::new(columns, &[], false)
        } else {
            let mut rows = DataRows::new(columns, &query.key, self.read_only.is_none());
            rows.relations = self.row_relations(query);
            rows
        };
        rows.loading = true;
        let grid = cx.new(|cx| {
            TableState::new(rows, window, cx)
                .cell_selectable(true)
                .row_header(false)
        });
        self._grid = vec![
            cx.observe_in(&grid, window, Self::on_grid),
            cx.subscribe_in(&grid, window, Self::on_table_event),
        ];
        grid
    }

    /// Group columns as they are, then aggregates; all read-only.
    fn summary_columns(&self, info: &TableInfo, summary: &Summary) -> Vec<GridColumn> {
        let by = summary.by.iter().map(|source| GridColumn {
            base: None,
            picks_from: None,
            ..self.grid_column(info, source)
        });
        let aggregates = summary.aggregates.iter().map(|(agg, source)| {
            let numeric_source = source
                .as_ref()
                .is_some_and(|s| self.grid_column(info, s).numeric);
            GridColumn {
                label: String::new(),
                base: None,
                numeric: match agg {
                    Agg::Count | Agg::Sum | Agg::Avg => true,
                    Agg::Min | Agg::Max => numeric_source,
                    Agg::List => false,
                },
                picks_from: None,
            }
        });
        by.chain(aggregates)
            .zip(summary.labels())
            .map(|(column, label)| GridColumn { label, ..column })
            .collect()
    }

    /// Shows groups instead of rows, or rows again with `None`.
    pub fn summarize(
        &mut self,
        summary: Option<Summary>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .grid_entity()
            .is_some_and(|g| g.read(cx).delegate().has_changes())
        {
            self.error = Some("Commit or discard your changes first.".into());
            cx.notify();
            return;
        }
        let State::Ready { info, query, .. } = &mut self.state else {
            return;
        };
        query.summary = summary;
        let (info, query) = (info.clone(), query.clone());
        let grid = self.new_grid(&info, &query, window, cx);
        if let State::Ready { grid: slot, .. } = &mut self.state {
            *slot = grid;
        }
        self.load_page(true, cx);
    }

    fn show_summarize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let State::Ready { info, query, .. } = &self.state else {
            return;
        };
        // Summary columns can't be grouped or aggregated again.
        let columns: Vec<(Source, bool)> = query
            .columns
            .iter()
            .filter(|s| !matches!(s, Source::Summary { .. }))
            .map(|s| (s.clone(), self.grid_column(info, s).numeric))
            .collect();
        let view = cx.entity().downgrade();
        let dialog = cx.new(|_| {
            SummarizeDialog::new(
                columns,
                Rc::new(move |summary, window, cx| {
                    view.update(cx, |v, cx| v.summarize(Some(summary), window, cx))
                        .ok();
                }),
            )
        });
        window.open_dialog(cx, move |d, _, _| {
            d.title("Summarize").w(px(560.)).child(dialog.clone())
        });
    }

    fn grid_column(&self, info: &TableInfo, source: &Source) -> GridColumn {
        let column_type = |schema: &str, table: &str, column: &str| {
            self.related
                .get(&(schema.to_owned(), table.to_owned()))
                .and_then(|t| t.columns.iter().find(|c| c.name == column))
                .map(|c| c.data_type.clone())
        };
        let (numeric, picks_from) = match source {
            Source::Base(name) => {
                let numeric = info
                    .columns
                    .iter()
                    .find(|c| &c.name == name)
                    .is_some_and(|c| data_grid::is_numeric_type(&c.data_type));
                let picks = info
                    .foreign_keys
                    .iter()
                    .find(|fk| fk.columns == [name.clone()])
                    .map(|fk| fk.ref_table.clone());
                (numeric, picks)
            }
            Source::Lookup { path, column } => {
                let hop = path.last().expect("paths are not empty");
                let t = column_type(&hop.schema, &hop.table, column);
                (t.is_some_and(|t| data_grid::is_numeric_type(&t)), None)
            }
            Source::Summary {
                children,
                agg,
                column,
            } => {
                let numeric_column = column
                    .as_deref()
                    .and_then(|c| column_type(&children.schema, &children.table, c))
                    .is_some_and(|t| data_grid::is_numeric_type(&t));
                let numeric = match agg {
                    Agg::Count | Agg::Sum | Agg::Avg => true,
                    Agg::Min | Agg::Max => numeric_column,
                    Agg::List => false,
                };
                (numeric, None)
            }
        };
        GridColumn {
            label: source.label(),
            base: source.base().map(str::to_owned),
            numeric,
            picks_from,
        }
    }

    /// The links whose base columns are all shown, with their grid columns.
    fn row_relations(&self, query: &TableQuery) -> Vec<data_grid::Relation> {
        self.links
            .iter()
            .filter_map(|link| {
                let columns = link
                    .base
                    .iter()
                    .map(|b| {
                        query
                            .columns
                            .iter()
                            .position(|s| s.base() == Some(b.as_str()))
                    })
                    .collect::<Option<Vec<usize>>>()?;
                Some(data_grid::Relation {
                    label: link.label.clone(),
                    columns,
                })
            })
            .collect()
    }

    /// Loads the details of the tables around this one (its whole schema,
    /// and tables in other schemas its foreign keys reach), then refreshes
    /// the row menu's links.
    fn load_related(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let State::Ready { info, .. } = &self.state else {
            return;
        };
        let Some(session) = self.data_sources.read(cx).session(self.node.connection) else {
            return;
        };
        let (database, schema) = (self.node.database.clone(), self.node.schema.clone());
        let engine = self.engine(cx);
        let elsewhere: Vec<(String, String)> = info
            .foreign_keys
            .iter()
            .filter(|fk| fk.ref_schema != schema)
            .map(|fk| (fk.ref_schema.clone(), fk.ref_table.clone()))
            .collect();
        let io = runtime::spawn(async move {
            let mut tables = Tables::new();
            for table in session.describe_schema(&database, &schema).await? {
                tables.insert((schema.clone(), table.name.clone()), Arc::new(table));
            }
            for (other, table) in elsewhere {
                // On MySQL the schema is the database.
                let db = if engine == Engine::Mysql {
                    other.clone()
                } else {
                    database.clone()
                };
                if let Ok(info) = session.describe_table(&db, &other, &table).await {
                    tables.insert((other, table), info);
                }
            }
            Ok(tables)
        });
        self._related = Some(cx.spawn_in(window, async move |this, cx| {
            let result = session::join(io).await;
            this.update(cx, |this, cx| {
                if let Ok(tables) = result {
                    this.related = tables;
                    this.refresh_links(cx);
                }
            })
            .ok();
        }));
    }

    fn refresh_links(&mut self, cx: &mut Context<Self>) {
        let State::Ready { info, grid, query } = &self.state else {
            return;
        };
        self.links = relations::links(&self.node.schema, info, &self.related, &self.custom);
        let relations = self.row_relations(query);
        grid.update(cx, |g, cx| {
            g.delegate_mut().relations = relations;
            cx.notify();
        });
        cx.notify();
    }

    /// Adds a column to the view, or removes it. Key columns always stay.
    pub fn toggle_column(&mut self, source: Source, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .grid_entity()
            .is_some_and(|g| g.read(cx).delegate().has_changes())
        {
            self.error = Some("Commit or discard your changes first.".into());
            cx.notify();
            return;
        }
        let State::Ready { info, query, .. } = &mut self.state else {
            return;
        };
        match query.columns.iter().position(|s| *s == source) {
            Some(ix) => {
                let is_key = source
                    .base()
                    .is_some_and(|b| query.key.iter().any(|k| k == b));
                if is_key || query.columns.len() == 1 {
                    return;
                }
                query.columns.remove(ix);
                if query.sort.as_ref().is_some_and(|(s, _)| *s == source) {
                    query.sort = None;
                }
            }
            None => query.columns.push(source),
        }
        let (info, query) = (info.clone(), query.clone());
        let grid = self.new_grid(&info, &query, window, cx);
        if let State::Ready { grid: slot, .. } = &mut self.state {
            *slot = grid;
        }
        self.load_page(true, cx);
    }

    fn show_columns(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let State::Ready { info, query, .. } = &self.state else {
            return;
        };
        let sections = relations::sections(&self.node.schema, info, &self.related, &self.custom);
        let chosen: HashSet<Source> = query.columns.iter().cloned().collect();
        let view = cx.entity().downgrade();
        let join_view = view.clone();
        let picker = cx.new(|cx| {
            ColumnPicker::new(
                sections,
                chosen,
                Rc::new(move |source, window, cx| {
                    view.update(cx, |v, cx| v.toggle_column(source, window, cx))
                        .ok();
                }),
                Rc::new(move |window, cx| {
                    join_view.update(cx, |v, cx| v.show_join(window, cx)).ok();
                }),
                window,
                cx,
            )
        });
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title("Columns").w(px(520.)).child(picker.clone())
        });
    }

    fn show_join(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let State::Ready { info, .. } = &self.state else {
            return;
        };
        let mut tables: Vec<(String, Arc<TableInfo>)> = self
            .related
            .iter()
            .filter(|((_, t), _)| t != &info.name)
            .map(|((s, _), t)| (s.clone(), t.clone()))
            .collect();
        tables.sort_by(|a, b| (&a.0, &a.1.name).cmp(&(&b.0, &b.1.name)));
        let base = info.clone();
        let view = cx.entity().downgrade();
        let dialog = cx.new(|cx| {
            JoinDialog::new(
                base,
                tables,
                Rc::new(move |hop, window, cx| {
                    view.update(cx, |v, cx| v.add_join(hop, window, cx)).ok();
                }),
                window,
                cx,
            )
        });
        window.open_dialog(cx, move |d, _, _| {
            d.title("Join another table")
                .w(px(520.))
                .child(dialog.clone())
        });
    }

    /// Adds a relation the catalog doesn't declare, then reopens the
    /// column picker on it.
    pub fn add_join(&mut self, hop: Hop, window: &mut Window, cx: &mut Context<Self>) {
        if !self.custom.contains(&hop) {
            self.custom.push(hop);
        }
        self.refresh_links(cx);
        self.show_columns(window, cx);
    }

    /// Opens the rows related to a grid row in another view.
    pub(crate) fn open_related(&mut self, row: usize, relation: usize, cx: &mut Context<Self>) {
        let Some(grid) = self.grid_entity() else {
            return;
        };
        let rows = grid.read(cx).delegate();
        let Some(r) = rows.relations.get(relation) else {
            return;
        };
        let Some(link) = self.links.iter().find(|l| l.label == r.label).cloned() else {
            return;
        };
        let Some(values) = rows.values(row, &r.columns) else {
            return;
        };
        let filters = link
            .target
            .iter()
            .zip(values)
            .map(|(column, value)| match value {
                Some(value) => Filter {
                    column: Source::Base(column.clone()),
                    op: Op::Eq,
                    value,
                },
                None => Filter {
                    column: Source::Base(column.clone()),
                    op: Op::IsNull,
                    value: String::new(),
                },
            })
            .collect();
        let node = NodeRef {
            connection: self.node.connection,
            database: if self.engine(cx) == Engine::Mysql {
                link.schema.clone()
            } else {
                self.node.database.clone()
            },
            schema: link.schema,
            table: Some(link.table),
        };
        cx.emit(DataViewEvent::Open { node, filters });
    }

    /// Lets the user pick a foreign-key cell's value from the referenced
    /// table, by its display column.
    fn pick_value(&mut self, row: usize, col: usize, window: &mut Window, cx: &mut Context<Self>) {
        let State::Ready { info, query, grid } = &self.state else {
            return;
        };
        let Some(base) = query.columns.get(col).and_then(Source::base) else {
            return;
        };
        let Some(fk) = info
            .foreign_keys
            .iter()
            .find(|fk| fk.columns == [base.to_owned()])
        else {
            return;
        };
        let Some(session) = self.data_sources.read(cx).session(self.node.connection) else {
            return;
        };
        let target = self
            .related
            .get(&(fk.ref_schema.clone(), fk.ref_table.clone()))
            .cloned();
        let display = target
            .as_deref()
            .and_then(relations::display_column)
            .unwrap_or_else(|| fk.ref_columns[0].clone());
        let grid = grid.downgrade();
        let picker = cx.new(|cx| {
            RowPicker::new(
                session,
                query.engine,
                fk.ref_schema.clone(),
                fk.ref_table.clone(),
                fk.ref_columns[0].clone(),
                display,
                Rc::new(move |value, _, cx| {
                    grid.update(cx, |g, cx| {
                        g.delegate_mut().set(row, col, value);
                        g.refresh(cx);
                        cx.notify();
                    })
                    .ok();
                }),
                window,
                cx,
            )
        });
        let title = format!("Choose from {}", fk.ref_table);
        window.open_dialog(cx, move |d, _, _| {
            d.title(title.clone()).w(px(480.)).child(picker.clone())
        });
    }

    /// Takes the grid's request: a sort or the next page.
    fn on_grid(
        &mut self,
        grid: Entity<TableState<DataRows>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = grid.update(cx, |g, _| g.delegate_mut().request.take()) else {
            return;
        };
        match request {
            Request::Sort(sort) => {
                if let State::Ready { query, .. } = &mut self.state {
                    match &mut query.summary {
                        Some(summary) => summary.sort = sort,
                        None => {
                            query.sort = sort.map(|(col, desc)| (query.columns[col].clone(), desc))
                        }
                    }
                }
                self.reload(cx);
            }
            Request::More => self.load_page(false, cx),
            Request::Open { row, relation } => self.open_related(row, relation, cx),
            Request::Pick { row, col } => self.pick_value(row, col, window, cx),
        }
    }

    fn on_table_event(
        &mut self,
        grid: &Entity<TableState<DataRows>>,
        event: &TableEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match *event {
            TableEvent::SelectCell(row, col) if col > 0 => {
                grid.update(cx, |g, _| g.delegate_mut().selected = Some((row, col - 1)));
            }
            TableEvent::DoubleClickedCell(row, col) if col > 0 => {
                self.edit_cell(row, col - 1, window, cx);
            }
            _ => {}
        }
    }

    fn grid_entity(&self) -> Option<Entity<TableState<DataRows>>> {
        match &self.state {
            State::Ready { grid, .. } => Some(grid.clone()),
            _ => None,
        }
    }

    /// Opens an input over a cell; Enter or leaving it keeps the value.
    pub fn edit_cell(
        &mut self,
        row: usize,
        col: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(grid) = self.grid_entity() else {
            return;
        };
        let rows = grid.read(cx).delegate();
        let deleted = matches!(rows.source(row), RowSource::Loaded(i) if rows.deleted.contains(&i));
        if !rows.editable || deleted {
            return;
        }
        let current = rows.value(row, col).map(str::to_owned);
        let placeholder = if current.is_none() { "NULL" } else { "" };
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(current.clone().unwrap_or_default())
                .placeholder(placeholder)
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        self._subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    this.finish_edit(cx);
                }
            },
        ));
        grid.update(cx, |g, cx| {
            g.delegate_mut().editing = Some((row, col, input));
            cx.notify();
        });
    }

    /// Keeps the edited cell's value. Leaving a NULL cell empty keeps NULL.
    pub fn finish_edit(&mut self, cx: &mut Context<Self>) {
        let Some(grid) = self.grid_entity() else {
            return;
        };
        grid.update(cx, |g, cx| {
            let rows = g.delegate_mut();
            let Some((row, col, input)) = rows.editing.take() else {
                return;
            };
            let text = input.read(cx).value().to_string();
            let was_null = rows.value(row, col).is_none();
            if !(was_null && text.is_empty()) {
                rows.set(row, col, Some(text));
            }
            g.refresh(cx);
            cx.notify();
        });
        cx.notify();
    }

    #[cfg(test)]
    pub fn is_committing(&self) -> bool {
        self.committing
    }

    #[cfg(test)]
    pub fn links(&self) -> &[Link] {
        &self.links
    }

    pub fn add_row(&mut self, cx: &mut Context<Self>) {
        if self.committing {
            return;
        }
        if let Some(grid) = self.grid_entity() {
            grid.update(cx, |g, cx| {
                g.delegate_mut().add_row();
                g.refresh(cx);
                cx.notify();
            });
        }
    }

    fn pending(&self, cx: &App) -> usize {
        self.grid_entity()
            .map_or(0, |g| g.read(cx).delegate().changes().len())
    }

    fn edit_target(&self) -> Option<EditTarget> {
        let State::Ready { info, query, .. } = &self.state else {
            return None;
        };
        Some(EditTarget {
            engine: query.engine,
            container: query.container.clone(),
            table: query.table.clone(),
            columns: info
                .columns
                .iter()
                .map(|c| (c.name.clone(), c.data_type.clone()))
                .collect(),
        })
    }

    /// The pending changes as the script that commit runs.
    pub fn review_sql(&self, cx: &App) -> Option<String> {
        let target = self.edit_target()?;
        let changes = self.grid_entity()?.read(cx).delegate().changes();
        (!changes.is_empty()).then(|| target.script(&changes))
    }

    pub fn discard(&mut self, cx: &mut Context<Self>) {
        if let Some(grid) = self.grid_entity() {
            grid.update(cx, |g, cx| {
                g.delegate_mut().discard();
                g.refresh(cx);
                cx.notify();
            });
        }
        self.error = None;
        cx.notify();
    }

    /// Runs the pending changes in one transaction, then reloads. On a
    /// failure everything is rolled back and the changes stay pending.
    pub fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(grid), Some(target)) = (self.grid_entity(), self.edit_target()) else {
            return;
        };
        let changes = grid.read(cx).delegate().changes();
        if changes.is_empty() || self.committing {
            return;
        }
        let Some(session) = self.data_sources.read(cx).session(self.node.connection) else {
            self.error = Some("The data source is not connected.".into());
            cx.notify();
            return;
        };
        let statements: Vec<String> = changes.iter().map(|c| target.statement(c)).collect();
        let shown = statements.clone();
        let engine = target.engine;
        self.committing = true;
        // No edits while the commit runs: its success clears the buffer.
        grid.update(cx, |g, _| {
            let rows = g.delegate_mut();
            rows.editable = false;
            rows.editing = None;
        });
        let io = runtime::spawn(async move { Ok(session.transaction(engine, statements).await) });
        self._load = Some(cx.spawn_in(window, async move |this, cx| {
            let result = session::join(io).await;
            this.update_in(cx, |this, window, cx| {
                this.committing = false;
                if let Some(grid) = this.grid_entity() {
                    grid.update(cx, |g, _| g.delegate_mut().editable = true);
                }
                match result {
                    Ok(Ok(())) => {
                        let n = shown.len();
                        if let Some(grid) = this.grid_entity() {
                            grid.update(cx, |g, _| g.delegate_mut().discard());
                        }
                        this.error = None;
                        this.reload(cx);
                        window.push_notification(
                            Notification::success(format!(
                                "Committed {n} change{}",
                                if n == 1 { "" } else { "s" }
                            )),
                            cx,
                        );
                    }
                    Ok(Err((ix, err))) => {
                        this.error = Some(match ix {
                            Some(ix) => format!(
                                "Rolled back. Change {} failed: {err}\n{}",
                                ix + 1,
                                shown[ix]
                            ),
                            None => format!("Rolled back: {err}"),
                        });
                    }
                    Err(err) => this.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn show_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sql) = self.review_sql(cx) else {
            return;
        };
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let theme = cx.theme();
            let view = view.clone();
            alert
                .title("Review changes")
                .description(
                    div()
                        .id("review-sql")
                        .max_h(px(360.))
                        .overflow_y_scroll()
                        .p_2()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .font_family("monospace")
                        .text_sm()
                        .text_color(theme.foreground)
                        .children(sql.lines().map(|l| div().child(l.to_owned()))),
                )
                .show_cancel(true)
                .ok_text("Commit")
                .on_ok(move |_, window, cx| {
                    view.update(cx, |v, cx| v.commit(window, cx)).ok();
                    true
                })
        });
    }

    /// Starts over from the first page. Pending changes refer to loaded
    /// rows, so they have to be committed or discarded first.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self
            .grid_entity()
            .is_some_and(|g| g.read(cx).delegate().has_changes())
        {
            self.error = Some("Commit or discard your changes first.".into());
            cx.notify();
            return;
        }
        self.load_page(true, cx);
    }

    fn load_page(&mut self, fresh: bool, cx: &mut Context<Self>) {
        let State::Ready { grid, query, .. } = &self.state else {
            return;
        };
        let Some(session) = self.data_sources.read(cx).session(self.node.connection) else {
            self.error = Some("The data source is not connected.".into());
            cx.notify();
            return;
        };
        let offset = if fresh {
            0
        } else {
            grid.read(cx).delegate().len()
        };
        let sql = query.page_sql(offset, PAGE_SIZE);
        grid.update(cx, |g, cx| {
            g.delegate_mut().loading = true;
            cx.notify();
        });
        let grid = grid.clone();
        let io = runtime::spawn(async move { session.fetch(sql).await });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = session::join(io).await;
            this.update(cx, |this, cx| {
                grid.update(cx, |g, cx| {
                    let rows = g.delegate_mut();
                    rows.loading = false;
                    match result {
                        Ok((_, mut page)) => {
                            rows.more = page.len() > PAGE_SIZE;
                            page.truncate(PAGE_SIZE);
                            if fresh {
                                rows.rows = page;
                            } else {
                                rows.rows.extend(page);
                            }
                            this.error = None;
                        }
                        Err(err) => this.error = Some(err.to_string()),
                    }
                    g.refresh(cx);
                    cx.notify();
                });
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Adds the filter in the bar and reloads.
    pub fn add_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(column_choice) = &self.filter_column else {
            return;
        };
        let State::Ready { query, .. } = &mut self.state else {
            return;
        };
        let column = column_choice
            .read(cx)
            .selected_index(cx)
            .and_then(|ix| query.columns.get(ix.row))
            .cloned();
        let op = self
            .filter_op
            .read(cx)
            .selected_index(cx)
            .and_then(|ix| Op::ALL.get(ix.row).copied())
            .unwrap_or(Op::Eq);
        let value = self.filter_value.read(cx).value().to_string();
        let Some(column) = column else {
            return;
        };
        if op.takes_value() && value.is_empty() && op != Op::Eq && op != Op::Ne {
            return;
        }
        query.filters.push(Filter { column, op, value });
        self.filter_value
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.reload(cx);
    }

    pub fn remove_filter(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let State::Ready { query, .. } = &mut self.state
            && ix < query.filters.len()
        {
            query.filters.remove(ix);
            self.reload(cx);
        }
    }

    /// Sorts by a data column (0-based) on the server, as a header click does.
    #[cfg(test)]
    pub fn sort_by(&mut self, column: Option<(usize, bool)>, cx: &mut Context<Self>) {
        if let State::Ready { query, .. } = &mut self.state {
            query.sort = column.map(|(col, desc)| (query.columns[col].clone(), desc));
        }
        self.reload(cx);
    }

    /// Loads the next page, as scrolling to the end does.
    #[cfg(test)]
    pub fn load_next(&mut self, cx: &mut Context<Self>) {
        self.load_page(false, cx);
    }

    #[cfg(test)]
    pub fn push_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        if let State::Ready { query, .. } = &mut self.state {
            query.filters.push(filter);
            self.reload(cx);
        }
    }

    fn open_in_console(&mut self, cx: &mut Context<Self>) {
        if let Some(query) = self.query() {
            let sql = format!("{};", query.sql());
            cx.emit(DataViewEvent::Sql {
                connection: self.node.connection,
                sql,
            });
        }
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (loaded, more, loading) = match &self.state {
            State::Ready { grid, .. } => {
                let rows = grid.read(cx).delegate();
                (rows.len(), rows.more, rows.loading)
            }
            _ => (0, false, true),
        };
        let summarized = self.query().is_some_and(|q| q.summary.is_some());
        let noun = if summarized { "groups" } else { "rows" };
        let count = if loading {
            "loading…".to_string()
        } else if more {
            format!("{loaded}+ {noun}")
        } else {
            format!("{loaded} {noun}")
        };
        let path = format!("{} › {}", self.node.schema, self.title());
        h_flex()
            .h(px(34.))
            .px_2()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                Icon::new(Lucide::Table)
                    .small()
                    .text_color(theme.muted_foreground),
            )
            .child(div().text_sm().child(path))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(count),
            )
            .when_some(self.read_only, |bar, reason| {
                bar.child(
                    div()
                        .px_1p5()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(theme.warning)
                        .text_xs()
                        .text_color(theme.warning)
                        .child(reason),
                )
            })
            .child(div().flex_1())
            .when(summarized, |bar| {
                bar.child(
                    Button::new("back-to-rows")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::ArrowLeft))
                        .label("Back to rows")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.summarize(None, window, cx)),
                        ),
                )
            })
            .when(!summarized && self.filter_column.is_some(), |bar| {
                bar.child(
                    Button::new("summarize")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(Lucide::Sigma))
                        .label("Summarize")
                        .tooltip("Group rows and aggregate them")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.show_summarize(window, cx)),
                        ),
                )
            })
            .when(!summarized && self.filter_column.is_some(), |bar| {
                bar.child(
                    Button::new("add-column")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(Lucide::Columns3))
                        .label("Columns")
                        .tooltip("Add columns, from this table or related ones")
                        .on_click(cx.listener(|this, _, window, cx| this.show_columns(window, cx))),
                )
            })
            .when(
                !summarized && self.read_only.is_none() && self.filter_column.is_some(),
                |bar| {
                    bar.child(
                        Button::new("add-row")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Plus))
                            .tooltip("Add a row")
                            .on_click(cx.listener(|this, _, _, cx| this.add_row(cx))),
                    )
                },
            )
            .when_some(self.filter_column.clone(), |bar, column| {
                bar.child(div().w(px(150.)).child(Select::new(&column).xsmall()))
                    .child(
                        div()
                            .w(px(110.))
                            .child(Select::new(&self.filter_op).xsmall()),
                    )
                    .child(
                        div()
                            .w(px(150.))
                            .child(Input::new(&self.filter_value).xsmall()),
                    )
                    .child(
                        Button::new("add-filter")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(Lucide::Funnel))
                            .tooltip("Add filter (↩)")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_filter(window, cx)),
                            ),
                    )
            })
            .child(
                Button::new("reload-data")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(Lucide::RotateCcw))
                    .tooltip("Reload")
                    .disabled(loading)
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            )
            .child(
                Button::new("data-sql")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(Lucide::FileCode))
                    .tooltip("Open this view's SQL in a console")
                    .on_click(cx.listener(|this, _, _, cx| this.open_in_console(cx))),
            )
    }

    fn render_pending(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let pending = self.pending(cx);
        if pending == 0 {
            return None;
        }
        let theme = cx.theme().clone();
        Some(
            h_flex()
                .px_2()
                .py_1()
                .gap_2()
                .bg(theme.warning.opacity(0.12))
                .border_b_1()
                .border_color(theme.border)
                .text_sm()
                .child(format!(
                    "{pending} pending change{}",
                    if pending == 1 { "" } else { "s" }
                ))
                .child(div().flex_1())
                .child(
                    Button::new("review-changes")
                        .ghost()
                        .xsmall()
                        .label("Review SQL")
                        .on_click(cx.listener(|this, _, window, cx| this.show_review(window, cx))),
                )
                .child(
                    Button::new("discard-changes")
                        .ghost()
                        .xsmall()
                        .label("Discard")
                        .on_click(cx.listener(|this, _, _, cx| this.discard(cx))),
                )
                .child(
                    Button::new("commit-changes")
                        .primary()
                        .xsmall()
                        .label(if self.committing {
                            "Committing…"
                        } else {
                            "Commit"
                        })
                        .disabled(self.committing)
                        .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx))),
                ),
        )
    }

    fn render_filters(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let query = self.query()?;
        if query.filters.is_empty() {
            return None;
        }
        let theme = cx.theme().clone();
        let chips = query.filters.iter().enumerate().map(|(ix, f)| {
            let text = if f.op.takes_value() {
                format!("{} {} '{}'", f.column.label(), f.op.label(), f.value)
            } else {
                format!("{} {}", f.column.label(), f.op.label())
            };
            h_flex()
                .gap_1()
                .pl_2()
                .rounded(px(10.))
                .border_1()
                .border_color(theme.border)
                .text_xs()
                .child(text)
                .child(
                    Button::new(("remove-filter", ix))
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::Close))
                        .tooltip("Remove filter")
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_filter(ix, cx))),
                )
        });
        Some(
            h_flex()
                .px_2()
                .py_1()
                .gap_1()
                .flex_wrap()
                .border_b_1()
                .border_color(theme.border)
                .children(chips),
        )
    }
}

impl Render for DataView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let body = match &self.state {
            State::Loading => div()
                .p_3()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("Loading… (waits for a running query)")
                .into_any_element(),
            State::Failed(err) => div()
                .p_3()
                .text_sm()
                .text_color(theme.danger)
                .child(err.clone())
                .into_any_element(),
            State::Ready { grid, .. } => DataTable::new(grid).bordered(false).into_any_element(),
        };
        v_flex()
            .size_full()
            .child(self.render_toolbar(cx))
            .children(self.render_filters(cx))
            .children(self.render_pending(cx))
            .when_some(self.error.clone(), |el, err| {
                el.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_sm()
                        .text_color(theme.danger)
                        .child(err),
                )
            })
            .child(div().flex_1().min_h_0().child(body))
    }
}
