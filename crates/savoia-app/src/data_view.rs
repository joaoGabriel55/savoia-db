//! Data view: browse a table without writing SQL. Pages come from the
//! server ([`PAGE_SIZE`] rows each, loaded as the grid scrolls), header
//! clicks sort on the server, and filter chips become the `WHERE` clause.
//! The generated statement is always one click away.
//! See `docs/adr/202610091908-add-a-no-sql-data-view-with-visual-joins-to-v1.md`.

use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{SearchableVec, Select, SelectState};
use gpui_kit::component::table::{Column, ColumnSort, DataTable, TableDelegate, TableState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, IndexPath, Sizable as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_core::data_query::{Filter, Op, PAGE_SIZE, TableQuery};
use savoia_core::{ConnectionId, Engine, Row, TableInfo};

use crate::data_sources::DataSources;
use crate::explorer::NodeRef;
use crate::{runtime, session};

/// What the grid asks of the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Request {
    /// Sort by a data column (0-based), descending or not; `None` resets.
    Sort(Option<(usize, bool)>),
    More,
}

/// The rows loaded so far, for the grid.
pub struct DataRows {
    columns: Vec<Column>,
    numeric: Vec<bool>,
    rows: Vec<Row>,
    /// Another page exists on the server.
    more: bool,
    loading: bool,
    request: Option<Request>,
}

impl DataRows {
    fn new(info: &TableInfo) -> Self {
        let numeric: Vec<bool> = info
            .columns
            .iter()
            .map(|c| is_numeric_type(&c.data_type))
            .collect();
        let columns = std::iter::once(
            Column::new("#", "")
                .width(px(52.))
                .text_right()
                .fixed_left()
                .resizable(false)
                .selectable(false),
        )
        .chain(info.columns.iter().zip(&numeric).map(|(c, numeric)| {
            let column = Column::new(SharedString::from(c.name.clone()), c.name.clone()).sortable();
            if *numeric {
                column.text_right()
            } else {
                column
            }
        }))
        .collect();
        Self {
            columns,
            numeric,
            rows: Vec::new(),
            more: false,
            loading: true,
            request: None,
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    #[cfg(test)]
    pub fn row(&self, ix: usize) -> &Row {
        &self.rows[ix]
    }
}

/// A coarse guess from the type the server prints, for alignment.
fn is_numeric_type(data_type: &str) -> bool {
    let t = data_type.to_ascii_lowercase();
    [
        "int", "numeric", "decimal", "real", "double", "float", "serial", "money",
    ]
    .iter()
    .any(|n| t.contains(n))
        && !t.contains("interval")
        && !t.contains("point")
}

impl TableDelegate for DataRows {
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
        if col_ix == 0 {
            return cell
                .justify_end()
                .text_color(muted)
                .child((row_ix + 1).to_string());
        }
        let cell = cell.when(self.numeric[col_ix - 1], |c| c.justify_end());
        match self.rows[row_ix][col_ix - 1].as_deref() {
            Some(text) => cell.child(SharedString::from(text.to_owned())),
            None => cell.text_color(muted).child("NULL"),
        }
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let sort = match (col_ix, sort) {
            (0, _) | (_, ColumnSort::Default) => None,
            (col, ColumnSort::Ascending) => Some((col - 1, false)),
            (col, ColumnSort::Descending) => Some((col - 1, true)),
        };
        self.request = Some(Request::Sort(sort));
        cx.notify();
    }

    fn has_more(&self, _: &App) -> bool {
        self.more && !self.loading
    }

    fn load_more(&mut self, _: &mut Window, cx: &mut Context<TableState<Self>>) {
        if self.more && !self.loading {
            self.request = Some(Request::More);
            cx.notify();
        }
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
        match col_ix {
            0 => (row_ix + 1).to_string(),
            _ => self.rows[row_ix][col_ix - 1]
                .as_deref()
                .unwrap_or("NULL")
                .to_owned(),
        }
    }
}

pub enum DataViewEvent {
    /// Put this SQL in a console on the view's data source.
    Sql {
        connection: ConnectionId,
        sql: String,
    },
}

enum State {
    Loading,
    Failed(String),
    Ready {
        info: Arc<TableInfo>,
        grid: Entity<TableState<DataRows>>,
        query: TableQuery,
    },
}

type Choice = SelectState<SearchableVec<SharedString>>;

pub struct DataView {
    data_sources: Entity<DataSources>,
    /// Always has a table.
    node: NodeRef,
    state: State,
    /// Shown above the grid when a page fails to load.
    error: Option<String>,
    filter_column: Option<Entity<Choice>>,
    filter_op: Entity<Choice>,
    filter_value: Entity<InputState>,
    _load: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DataViewEvent> for DataView {}

impl DataView {
    pub fn new(
        data_sources: Entity<DataSources>,
        node: NodeRef,
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
            filter_column: None,
            filter_op,
            filter_value,
            _load: None,
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
            columns: info.columns.iter().map(|c| c.name.clone()).collect(),
            filters: Vec::new(),
            sort: None,
            key,
        };
        let names = SearchableVec::new(
            info.columns
                .iter()
                .map(|c| SharedString::from(c.name.clone()))
                .collect::<Vec<_>>(),
        );
        self.filter_column =
            Some(cx.new(|cx| SelectState::new(names, Some(IndexPath::new(0)), window, cx)));
        let grid = cx.new(|cx| TableState::new(DataRows::new(&info), window, cx));
        self._subscriptions
            .push(cx.observe_in(&grid, window, Self::on_grid));
        self.state = State::Ready { info, grid, query };
        self.reload(cx);
    }

    /// Takes the grid's request: a sort or the next page.
    fn on_grid(
        &mut self,
        grid: Entity<TableState<DataRows>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = grid.update(cx, |g, _| g.delegate_mut().request.take()) else {
            return;
        };
        match request {
            Request::Sort(sort) => {
                if let State::Ready { info, query, .. } = &mut self.state {
                    query.sort = sort.map(|(col, desc)| (info.columns[col].name.clone(), desc));
                }
                self.reload(cx);
            }
            Request::More => self.load_page(false, cx),
        }
    }

    /// Starts over from the first page.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
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
        let State::Ready { info, query, .. } = &mut self.state else {
            return;
        };
        let column = column_choice
            .read(cx)
            .selected_index(cx)
            .and_then(|ix| info.columns.get(ix.row))
            .map(|c| c.name.clone());
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
        if let State::Ready { info, query, .. } = &mut self.state {
            query.sort = column.map(|(col, desc)| (info.columns[col].name.clone(), desc));
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
        let count = if loading {
            "loading…".to_string()
        } else if more {
            format!("{loaded}+ rows")
        } else {
            format!("{loaded} rows")
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
            .child(div().flex_1())
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

    fn render_filters(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let query = self.query()?;
        if query.filters.is_empty() {
            return None;
        }
        let theme = cx.theme().clone();
        let chips = query.filters.iter().enumerate().map(|(ix, f)| {
            let text = if f.op.takes_value() {
                format!("{} {} '{}'", f.column, f.op.label(), f.value)
            } else {
                format!("{} {}", f.column, f.op.label())
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

#[cfg(test)]
mod tests {
    use super::is_numeric_type;

    #[test]
    fn numeric_types() {
        for t in [
            "integer",
            "bigint",
            "numeric(10,2)",
            "double precision",
            "int unsigned",
        ] {
            assert!(is_numeric_type(t), "{t}");
        }
        for t in ["text", "interval", "timestamp with time zone", "point"] {
            assert!(!is_numeric_type(t), "{t}");
        }
    }
}
