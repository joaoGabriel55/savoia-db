//! Dialogs of the data view: the "+ Column" picker over relationships, the
//! row picker that fills a foreign-key cell, and "Join another table…" for
//! relations the catalog doesn't declare.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, IndexPath, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_core::data_query::{Agg, Hop, Source, Summary};
use savoia_core::sql_text::{quote_ident, quote_literal};
use savoia_core::{Engine, TableInfo};

use crate::relations::Section;
use crate::session::Session;
use crate::{runtime, session};

type Toggle = Rc<dyn Fn(Source, &mut Window, &mut App)>;
type Action = Rc<dyn Fn(&mut Window, &mut App)>;
type OnValue = Rc<dyn Fn(Option<String>, &mut Window, &mut App)>;
type OnHop = Rc<dyn Fn(Hop, &mut Window, &mut App)>;

/// "+ Column": every column reachable through relationships, grouped by
/// path. Clicking one adds it to the view, or removes it.
pub struct ColumnPicker {
    sections: Vec<Section>,
    chosen: HashSet<Source>,
    search: Entity<InputState>,
    on_toggle: Toggle,
    on_join: Action,
}

impl ColumnPicker {
    pub fn new(
        sections: Vec<Section>,
        chosen: HashSet<Source>,
        on_toggle: Toggle,
        on_join: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search columns"));
        cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        search.update(cx, |input, cx| input.focus(window, cx));
        Self {
            sections,
            chosen,
            search,
            on_toggle,
            on_join,
        }
    }

    pub fn toggle(&mut self, source: Source, window: &mut Window, cx: &mut Context<Self>) {
        if !self.chosen.remove(&source) {
            self.chosen.insert(source.clone());
        }
        (self.on_toggle)(source, window, cx);
        cx.notify();
    }
}

impl Render for ColumnPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let search = self.search.read(cx).value().to_lowercase();
        let mut list = v_flex()
            .id("column-choices")
            .h(px(440.))
            .overflow_y_scroll();
        let mut n: usize = 0;
        for section in &self.sections {
            let matching: Vec<&(String, Source)> = section
                .choices
                .iter()
                .filter(|(label, _)| {
                    search.is_empty()
                        || label.to_lowercase().contains(&search)
                        || section.title.to_lowercase().contains(&search)
                })
                .collect();
            if matching.is_empty() {
                continue;
            }
            list = list.child(
                div()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(section.title.clone()),
            );
            for (label, source) in matching {
                let chosen = self.chosen.contains(source);
                let source = source.clone();
                n += 1;
                list = list.child(
                    h_flex()
                        .id(("column-choice", n))
                        .px_2()
                        .py_1()
                        .gap_2()
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(|el| el.bg(theme.accent))
                        .child(
                            div()
                                .w(px(16.))
                                .when(chosen, |el| el.child(Icon::new(IconName::Check).xsmall())),
                        )
                        .child(div().text_sm().child(label.clone()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.toggle(source.clone(), window, cx)
                        })),
                );
            }
        }
        let on_join = self.on_join.clone();
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.search).small()))
                    .child(
                        Button::new("join-another")
                            .ghost()
                            .small()
                            .label("Join another table…")
                            .tooltip(
                                "Match columns with a table that has no foreign key to this one",
                            )
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                on_join(window, cx);
                            }),
                    ),
            )
            .child(list)
    }
}

/// Rows of a referenced table, searched by their display column; picking
/// one hands back its key value.
pub struct RowPicker {
    session: Arc<Session>,
    engine: Engine,
    schema: String,
    table: String,
    key: String,
    display: String,
    search: Entity<InputState>,
    rows: Vec<(Option<String>, Option<String>)>,
    error: Option<String>,
    on_pick: OnValue,
    _load: Option<Task<()>>,
}

impl RowPicker {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session: Arc<Session>,
        engine: Engine,
        schema: String,
        table: String,
        key: String,
        display: String,
        on_pick: OnValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(format!("Search {display}")));
        cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.reload(cx);
            }
        })
        .detach();
        search.update(cx, |input, cx| input.focus(window, cx));
        let mut this = Self {
            session,
            engine,
            schema,
            table,
            key,
            display,
            search,
            rows: Vec::new(),
            error: None,
            on_pick,
            _load: None,
        };
        this.reload(cx);
        this
    }

    /// The first 50 rows whose display column contains the search text.
    pub fn sql(&self, search: &str) -> String {
        let q = |ident: &str| quote_ident(self.engine, ident);
        let (key, display) = (q(&self.key), q(&self.display));
        let filter = if search.is_empty() {
            String::new()
        } else {
            let pattern = format!(
                "%{}%",
                search
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            );
            let cast = match self.engine {
                Engine::Postgres => format!("CAST({display} AS text) ILIKE"),
                Engine::Mysql => format!("CAST({display} AS CHAR) LIKE"),
            };
            format!(" WHERE {cast} {}", quote_literal(self.engine, &pattern))
        };
        format!(
            "SELECT {key}, {display} FROM {}.{}{filter} ORDER BY {display} LIMIT 50",
            q(&self.schema),
            q(&self.table)
        )
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let sql = self.sql(self.search.read(cx).value().trim());
        let session = self.session.clone();
        let io = runtime::spawn(async move { session.fetch(sql).await });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = session::join(io).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((_, rows)) => {
                        this.rows = rows
                            .into_iter()
                            .map(|r| {
                                let cell = |i: usize| r.get(i).cloned().flatten().map(String::from);
                                (cell(0), cell(1))
                            })
                            .collect();
                        this.error = None;
                    }
                    Err(err) => this.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    #[cfg(test)]
    pub fn rows(&self) -> &[(Option<String>, Option<String>)] {
        &self.rows
    }

    pub fn pick(&self, ix: usize, window: &mut Window, cx: &mut App) {
        if let Some((key, _)) = self.rows.get(ix) {
            (self.on_pick)(key.clone(), window, cx);
            window.close_dialog(cx);
        }
    }
}

impl Render for RowPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let rows = self.rows.iter().enumerate().map(|(ix, (key, display))| {
            h_flex()
                .id(("pick-row", ix))
                .px_2()
                .py_1()
                .gap_3()
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|el| el.bg(theme.accent))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .child(display.clone().unwrap_or_else(|| "NULL".into())),
                )
                .child(
                    div()
                        .font_family("monospace")
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(key.clone().unwrap_or_else(|| "NULL".into())),
                )
                .on_click(cx.listener(move |this, _, window, cx| this.pick(ix, window, cx)))
        });
        v_flex()
            .gap_2()
            .child(Input::new(&self.search).small())
            .when_some(self.error.clone(), |el, err| {
                el.child(div().text_sm().text_color(theme.danger).child(err))
            })
            .child(
                v_flex()
                    .id("pick-rows")
                    .h(px(360.))
                    .overflow_y_scroll()
                    .text_sm()
                    .children(rows),
            )
    }
}

type Choice = SelectState<SearchableVec<SharedString>>;

/// "Join another table…": pick a table and the column pair that relates a
/// row of this table to one of it. Becomes a many-to-one step like an FK.
pub struct JoinDialog {
    base: Arc<TableInfo>,
    /// Candidate tables, as (schema, details).
    tables: Vec<(String, Arc<TableInfo>)>,
    table: Entity<Choice>,
    base_column: Entity<Choice>,
    target_column: Entity<Choice>,
    on_add: OnHop,
    _subscriptions: Vec<Subscription>,
}

fn choice(items: Vec<String>, window: &mut Window, cx: &mut App) -> Entity<Choice> {
    let items = SearchableVec::new(
        items
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    );
    cx.new(|cx| SelectState::new(items, Some(IndexPath::new(0)), window, cx))
}

/// A likely column pair between `base` and `target`: `customer_id` and
/// `customers.id`, or a shared column name; else the first columns.
pub fn suggest(base: &TableInfo, target: &TableInfo) -> (usize, usize) {
    let target_key = target
        .columns
        .iter()
        .position(|c| target.is_key_column(&c.name))
        .unwrap_or(0);
    let stem = target.name.trim_end_matches('s');
    let named = base.columns.iter().position(|c| {
        let n = c.name.to_lowercase();
        n == format!("{stem}_id") || n == format!("{}_id", target.name)
    });
    if let Some(b) = named {
        return (b, target_key);
    }
    for (b, column) in base.columns.iter().enumerate() {
        if let Some(t) = target.columns.iter().position(|c| c.name == column.name) {
            return (b, t);
        }
    }
    (0, target_key)
}

impl JoinDialog {
    pub fn new(
        base: Arc<TableInfo>,
        tables: Vec<(String, Arc<TableInfo>)>,
        on_add: OnHop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let table = choice(
            tables
                .iter()
                .map(|(s, t)| format!("{s}.{}", t.name))
                .collect(),
            window,
            cx,
        );
        let base_column = choice(
            base.columns.iter().map(|c| c.name.clone()).collect(),
            window,
            cx,
        );
        let target_column = choice(Vec::new(), window, cx);
        let subscriptions = vec![cx.subscribe_in(
            &table,
            window,
            |this, _, _: &SelectEvent<SearchableVec<SharedString>>, window, cx| {
                this.table_changed(window, cx)
            },
        )];
        let mut this = Self {
            base,
            tables,
            table,
            base_column,
            target_column,
            on_add,
            _subscriptions: subscriptions,
        };
        this.table_changed(window, cx);
        this
    }

    fn target(&self, cx: &App) -> Option<&(String, Arc<TableInfo>)> {
        let ix = self.table.read(cx).selected_index(cx)?.row;
        self.tables.get(ix)
    }

    /// Lists the chosen table's columns and preselects a likely pair.
    fn table_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, target)) = self.target(cx).cloned() else {
            return;
        };
        let (b, t) = suggest(&self.base, &target);
        let items = SearchableVec::new(
            target
                .columns
                .iter()
                .map(|c| SharedString::from(c.name.clone()))
                .collect::<Vec<_>>(),
        );
        self.target_column.update(cx, |s, cx| {
            s.set_items(items, window, cx);
            s.set_selected_index(Some(IndexPath::new(t)), window, cx);
        });
        self.base_column.update(cx, |s, cx| {
            s.set_selected_index(Some(IndexPath::new(b)), window, cx)
        });
        cx.notify();
    }

    /// The step the dialog describes, and whether the target column is
    /// unique (else a row may match several).
    pub(crate) fn hop(&self, cx: &App) -> Option<(Hop, bool)> {
        let (schema, target) = self.target(cx)?;
        let b = self.base_column.read(cx).selected_index(cx)?.row;
        let t = self.target_column.read(cx).selected_index(cx)?.row;
        let column = target.columns.get(t)?.name.clone();
        let unique = target.primary_key == [column.clone()]
            || target
                .indexes
                .iter()
                .any(|i| i.unique && i.columns == [column.clone()]);
        Some((
            Hop {
                columns: vec![self.base.columns.get(b)?.name.clone()],
                schema: schema.clone(),
                table: target.name.clone(),
                ref_columns: vec![column],
            },
            unique,
        ))
    }
}

impl Render for JoinDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let hop = self.hop(cx);
        let field = |label: &str, el: AnyElement| {
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(label.to_owned()),
                )
                .child(el)
        };
        let add = hop.clone().map(|(hop, _)| hop);
        let on_add = self.on_add.clone();
        v_flex()
            .gap_3()
            .child(field(
                "Table",
                Select::new(&self.table).small().into_any_element(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(field(
                        &format!("{} column", self.base.name),
                        Select::new(&self.base_column).small().into_any_element(),
                    )))
                    .child(div().pt_5().child("="))
                    .child(div().flex_1().child(field(
                        "matches column",
                        Select::new(&self.target_column).small().into_any_element(),
                    ))),
            )
            .when(hop.as_ref().is_some_and(|(_, unique)| !unique), |el| {
                el.child(div().text_sm().text_color(theme.warning).child(
                    "That column isn't unique: a row may match several, and only one shows.",
                ))
            })
            .child(
                h_flex().justify_end().child(
                    Button::new("add-join")
                        .primary()
                        .small()
                        .label("Add join")
                        .on_click(move |_, window, cx| {
                            if let Some(hop) = add.clone() {
                                window.close_dialog(cx);
                                on_add(hop, window, cx);
                            }
                        }),
                ),
            )
    }
}

type OnSummary = Rc<dyn Fn(Summary, &mut Window, &mut App)>;

/// "Summarize": group the view's rows by some of its columns, with
/// aggregates per group.
pub struct SummarizeDialog {
    /// The view's columns that can group or be aggregated, and whether
    /// each is numeric.
    columns: Vec<(Source, bool)>,
    by: Vec<usize>,
    aggregates: Vec<(Agg, Option<usize>)>,
    on_apply: OnSummary,
}

impl SummarizeDialog {
    pub fn new(columns: Vec<(Source, bool)>, on_apply: OnSummary) -> Self {
        Self {
            columns,
            by: Vec::new(),
            aggregates: vec![(Agg::Count, None)],
            on_apply,
        }
    }

    pub fn toggle_by(&mut self, ix: usize, cx: &mut Context<Self>) {
        match self.by.iter().position(|&b| b == ix) {
            Some(at) => {
                self.by.remove(at);
            }
            None => self.by.push(ix),
        }
        cx.notify();
    }

    pub fn toggle_aggregate(&mut self, agg: Agg, column: Option<usize>, cx: &mut Context<Self>) {
        match self.aggregates.iter().position(|a| *a == (agg, column)) {
            Some(at) => {
                self.aggregates.remove(at);
            }
            None => self.aggregates.push((agg, column)),
        }
        cx.notify();
    }

    pub fn summary(&self) -> Option<Summary> {
        if self.by.is_empty() && self.aggregates.is_empty() {
            return None;
        }
        Some(Summary {
            by: self.by.iter().map(|&i| self.columns[i].0.clone()).collect(),
            aggregates: self
                .aggregates
                .iter()
                .map(|(agg, col)| (*agg, col.map(|i| self.columns[i].0.clone())))
                .collect(),
            sort: None,
        })
    }
}

impl Render for SummarizeDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let chip = |id: ElementId, label: String, on: bool| {
            div()
                .id(id)
                .px_2()
                .py_0p5()
                .rounded(px(10.))
                .border_1()
                .border_color(if on { theme.primary } else { theme.border })
                .when(on, |el| el.bg(theme.primary.opacity(0.15)))
                .text_xs()
                .cursor_pointer()
                .child(label)
        };
        let heading = |text: &str| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(text.to_owned())
        };
        let by = h_flex()
            .gap_1()
            .flex_wrap()
            .children(self.columns.iter().enumerate().map(|(ix, (source, _))| {
                chip(
                    ("group-by", ix).into(),
                    source.label(),
                    self.by.contains(&ix),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_by(ix, cx)))
            }));
        let mut aggregates = vec![
            chip(
                "agg-count".into(),
                "count rows".into(),
                self.aggregates.contains(&(Agg::Count, None)),
            )
            .on_click(cx.listener(|this, _, _, cx| this.toggle_aggregate(Agg::Count, None, cx))),
        ];
        let mut n: usize = 0;
        for (ix, (source, numeric)) in self.columns.iter().enumerate() {
            let aggs: &[Agg] = if *numeric {
                &[Agg::Sum, Agg::Avg, Agg::Min, Agg::Max]
            } else {
                &[Agg::Min, Agg::Max, Agg::List]
            };
            for agg in aggs {
                n += 1;
                let agg = *agg;
                aggregates.push(
                    chip(
                        ("agg", n).into(),
                        format!("{}({})", agg.label(), source.label()),
                        self.aggregates.contains(&(agg, Some(ix))),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.toggle_aggregate(agg, Some(ix), cx)),
                    ),
                );
            }
        }
        let summary = self.summary();
        let on_apply = self.on_apply.clone();
        v_flex()
            .gap_3()
            .child(heading("Group by"))
            .child(by)
            .child(heading("Aggregates"))
            .child(
                div()
                    .id("aggregates")
                    .max_h(px(240.))
                    .overflow_y_scroll()
                    .child(h_flex().gap_1().flex_wrap().children(aggregates)),
            )
            .child(
                h_flex().justify_end().child(
                    Button::new("apply-summary")
                        .primary()
                        .small()
                        .label("Summarize")
                        .on_click(move |_, window, cx| {
                            if let Some(summary) = summary.clone() {
                                window.close_dialog(cx);
                                on_apply(summary, window, cx);
                            }
                        }),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use savoia_core::{ColumnInfo, TableInfo, TableKind};

    use super::suggest;

    fn table(name: &str, columns: &[&str], pk: &str) -> TableInfo {
        let mut t = TableInfo::new(name, TableKind::Table);
        t.columns = columns
            .iter()
            .map(|c| ColumnInfo {
                name: (*c).into(),
                data_type: "integer".into(),
                nullable: true,
                default: None,
            })
            .collect();
        t.primary_key = vec![pk.into()];
        t
    }

    #[test]
    fn suggests_column_pairs() {
        let orders = table("orders", &["id", "customer_id", "region"], "id");
        assert_eq!(
            suggest(&orders, &table("customers", &["id", "name"], "id")),
            (1, 0)
        );
        assert_eq!(
            suggest(&orders, &table("regions", &["code", "region"], "code")),
            (2, 1)
        );
        assert_eq!(suggest(&orders, &table("misc", &["x"], "x")), (0, 0));
    }
}
