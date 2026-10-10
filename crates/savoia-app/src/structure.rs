//! Structure of one table or view: columns, indexes, foreign keys and the
//! DDL rebuilt from them, from the session's catalog cache.

use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_core::{Engine, TableInfo, ddl};

use crate::data_sources::DataSources;
use crate::explorer::NodeRef;
use crate::{runtime, session};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Columns,
    Indexes,
    ForeignKeys,
    Ddl,
}

enum State {
    Loading,
    Failed(String),
    Ready(Arc<TableInfo>),
}

pub struct StructureView {
    data_sources: Entity<DataSources>,
    /// Always has a table.
    node: NodeRef,
    state: State,
    pane: Pane,
    _load: Option<Task<()>>,
}

impl StructureView {
    pub fn new(data_sources: Entity<DataSources>, node: NodeRef, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            data_sources,
            node,
            state: State::Loading,
            pane: Pane::Columns,
            _load: None,
        };
        this.reload(cx);
        this
    }

    pub fn shows(&self, node: &NodeRef) -> bool {
        self.node == *node
    }

    pub fn title(&self) -> String {
        format!(
            "{} structure",
            self.node.table.as_deref().unwrap_or_default()
        )
    }

    #[cfg(test)]
    pub fn loaded(&self) -> Option<Arc<TableInfo>> {
        match &self.state {
            State::Ready(info) => Some(info.clone()),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn ddl(&self, cx: &App) -> Option<String> {
        match &self.state {
            State::Ready(info) => Some(self.ddl_text(info, cx)),
            _ => None,
        }
    }

    fn engine(&self, cx: &App) -> Engine {
        self.data_sources
            .read(cx)
            .get(self.node.connection)
            .map_or(Engine::Postgres, |c| c.engine)
    }

    fn ddl_text(&self, info: &TableInfo, cx: &App) -> String {
        // The schema qualifies names on Postgres, the database on MySQL.
        ddl::create_table(self.engine(cx), &self.node.schema, info)
    }

    /// Loads the table's details: from the cache if the explorer has them,
    /// else from the server, after any running query.
    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.data_sources.read(cx).session(self.node.connection) else {
            self.state = State::Failed("The data source is not connected.".into());
            return;
        };
        let (database, schema) = (self.node.database.clone(), self.node.schema.clone());
        let table = self.node.table.clone().unwrap_or_default();
        if let Some(info) = session.table(&database, &schema, &table) {
            self.state = State::Ready(info);
            return;
        }
        self.state = State::Loading;
        let io =
            runtime::spawn(async move { session.describe_table(&database, &schema, &table).await });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = session::join(io).await;
            this.update(cx, |this, cx| {
                this.state = match result {
                    Ok(info) => State::Ready(info),
                    Err(err) => State::Failed(err.to_string()),
                };
                cx.notify();
            })
            .ok();
        }));
    }

    fn render_columns(&self, info: &TableInfo, cx: &App) -> AnyElement {
        let rows = info.columns.iter().map(|c| {
            let key = if info.is_key_column(&c.name) {
                "PK"
            } else if info.is_foreign_column(&c.name) {
                "FK"
            } else {
                ""
            };
            vec![
                c.name.clone(),
                c.data_type.clone(),
                if c.nullable { "yes" } else { "no" }.into(),
                c.default.clone().unwrap_or_default(),
                key.into(),
            ]
        });
        grid(&["Name", "Type", "Nullable", "Default", "Key"], rows, cx)
    }

    fn render_indexes(&self, info: &TableInfo, cx: &App) -> AnyElement {
        let rows = info.indexes.iter().map(|i| {
            let kind = if i.primary {
                "primary"
            } else if i.unique {
                "unique"
            } else {
                ""
            };
            vec![i.name.clone(), i.columns.join(", "), kind.into()]
        });
        grid(&["Name", "Columns", "Kind"], rows, cx)
    }

    fn render_foreign_keys(&self, info: &TableInfo, cx: &App) -> AnyElement {
        let rows = info.foreign_keys.iter().map(|fk| {
            vec![
                fk.name.clone(),
                fk.columns.join(", "),
                format!(
                    "{}.{} ({})",
                    fk.ref_schema,
                    fk.ref_table,
                    fk.ref_columns.join(", ")
                ),
            ]
        });
        grid(&["Name", "Columns", "References"], rows, cx)
    }

    fn render_ddl(&self, info: &TableInfo, cx: &mut Context<Self>) -> AnyElement {
        let text = self.ddl_text(info, cx);
        let copy = text.clone();
        v_flex()
            .size_full()
            .child(
                h_flex().px_3().py_1().justify_end().child(
                    Button::new("copy-ddl")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::Copy))
                        .label("Copy")
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                        }),
                ),
            )
            .child(
                div()
                    .id("ddl")
                    .flex_1()
                    .min_h_0()
                    .px_3()
                    .pb_3()
                    .overflow_y_scroll()
                    .font_family("monospace")
                    .text_sm()
                    .whitespace_normal()
                    .children(text.lines().map(|line| div().child(line.to_owned()))),
            )
            .into_any_element()
    }
}

/// A plain read-only grid; structure lists are short.
fn grid(headers: &[&str], rows: impl Iterator<Item = Vec<String>>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let row = |cells: Vec<SharedString>, header: bool| {
        h_flex()
            .w_full()
            .px_3()
            .py_1()
            .gap_3()
            .border_b_1()
            .border_color(theme.border)
            .when(header, |el| el.text_color(theme.muted_foreground).text_xs())
            .when(!header, |el| el.font_family("monospace").text_sm())
            .children(
                cells
                    .into_iter()
                    .map(|cell| div().flex_1().min_w_0().truncate().child(cell)),
            )
    };
    let body: Vec<_> = rows
        .map(|cells| row(cells.into_iter().map(Into::into).collect(), false))
        .collect();
    let empty = body.is_empty();
    v_flex()
        .id("structure-grid")
        .size_full()
        .overflow_y_scroll()
        .child(row(headers.iter().map(|h| (*h).into()).collect(), true))
        .children(body)
        .when(empty, |el| {
            el.child(
                div()
                    .p_3()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("None."),
            )
        })
        .into_any_element()
}

impl Render for StructureView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let info = match &self.state {
            State::Ready(info) => info.clone(),
            State::Loading => {
                return div()
                    .p_3()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Loading… (waits for a running query)")
                    .into_any_element();
            }
            State::Failed(err) => {
                return div()
                    .p_3()
                    .text_sm()
                    .text_color(theme.danger)
                    .child(err.clone())
                    .into_any_element();
            }
        };
        let body = match self.pane {
            Pane::Columns => self.render_columns(&info, cx),
            Pane::Indexes => self.render_indexes(&info, cx),
            Pane::ForeignKeys => self.render_foreign_keys(&info, cx),
            Pane::Ddl => self.render_ddl(&info, cx),
        };
        let tab = |id: &'static str, label: String, pane: Pane| {
            let selected = self.pane == pane;
            div()
                .id(id)
                .cursor_pointer()
                .border_b_2()
                .border_color(if selected {
                    cx.theme().ring
                } else {
                    transparent_black()
                })
                .when(!selected, |el| el.text_color(theme.muted_foreground))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.pane = pane;
                    cx.notify();
                }))
        };
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(px(28.))
                    .px_3()
                    .gap_3()
                    .text_xs()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        Icon::new(Lucide::TableProperties)
                            .xsmall()
                            .text_color(theme.muted_foreground),
                    )
                    .child(tab(
                        "structure-columns",
                        format!("Columns {}", info.columns.len()),
                        Pane::Columns,
                    ))
                    .child(tab(
                        "structure-indexes",
                        format!("Indexes {}", info.indexes.len()),
                        Pane::Indexes,
                    ))
                    .child(tab(
                        "structure-fks",
                        format!("Foreign keys {}", info.foreign_keys.len()),
                        Pane::ForeignKeys,
                    ))
                    .child(tab("structure-ddl", "DDL".into(), Pane::Ddl)),
            )
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }
}
