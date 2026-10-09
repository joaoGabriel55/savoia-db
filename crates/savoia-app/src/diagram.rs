//! ER diagram of one schema: tables as boxes, foreign keys as lines. See
//! `docs/adr/202610091437-draw-er-diagrams-natively-with-a-built-in-layered-layout.md`.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_core::{TableInfo, TableKind};

use crate::data_sources::DataSources;
use crate::erd_layout::{self, BOX_PADDING, BOX_WIDTH, Edge, HEADER_HEIGHT, ROW_HEIGHT};
use crate::explorer::NodeRef;
use crate::runtime;
use crate::session;
use crate::theme;

/// Space left around the diagram when it is first shown.
const MARGIN: f32 = 32.;

enum State {
    Loading,
    Failed(String),
    Ready(Model),
}

struct Model {
    tables: Vec<TableInfo>,
    edges: Vec<Edge>,
    /// Top-left corner of each box, in diagram units.
    positions: Vec<(f32, f32)>,
}

enum Drag {
    Pan { last: Point<Pixels> },
    Table { index: usize, last: Point<Pixels> },
}

pub struct ErDiagram {
    data_sources: Entity<DataSources>,
    node: NodeRef,
    state: State,
    /// Where the diagram's origin is drawn, relative to the view.
    offset: Point<Pixels>,
    drag: Option<Drag>,
    _load: Option<Task<()>>,
}

impl ErDiagram {
    pub fn new(data_sources: Entity<DataSources>, node: NodeRef, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            data_sources,
            node,
            state: State::Loading,
            offset: point(px(MARGIN), px(MARGIN)),
            drag: None,
            _load: None,
        };
        this.reload(cx);
        this
    }

    /// Which schema this shows, ignoring the table it was opened from.
    pub fn shows(&self, node: &NodeRef) -> bool {
        (self.node.connection, &self.node.database, &self.node.schema)
            == (node.connection, &node.database, &node.schema)
    }

    pub fn title(&self) -> String {
        format!("{} diagram", self.node.schema)
    }

    /// Highlights `table` and brings it into view.
    pub fn focus(&mut self, table: Option<String>, cx: &mut Context<Self>) {
        self.node.table = table;
        self.scroll_to_focus();
        cx.notify();
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.data_sources.read(cx).session(self.node.connection) else {
            self.state = State::Failed("The data source is not connected.".into());
            return;
        };
        self.state = State::Loading;
        let (database, schema) = (self.node.database.clone(), self.node.schema.clone());
        let io = runtime::spawn(async move { session.describe_schema(&database, &schema).await });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = session::join(io).await;
            this.update(cx, |this, cx| {
                this.state = match result {
                    Ok(tables) => {
                        let edges = erd_layout::edges(&this.node.schema, &tables);
                        let positions = erd_layout::layout(&tables, &edges);
                        State::Ready(Model {
                            tables,
                            edges,
                            positions,
                        })
                    }
                    Err(err) => State::Failed(err.to_string()),
                };
                this.scroll_to_focus();
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn reset_layout(&mut self, cx: &mut Context<Self>) {
        if let State::Ready(model) = &mut self.state {
            model.positions = erd_layout::layout(&model.tables, &model.edges);
        }
        self.offset = point(px(MARGIN), px(MARGIN));
        self.scroll_to_focus();
        cx.notify();
    }

    fn scroll_to_focus(&mut self) {
        let State::Ready(model) = &self.state else {
            return;
        };
        let Some(table) = &self.node.table else {
            return;
        };
        if let Some(i) = model.tables.iter().position(|t| t.name == *table) {
            let (x, y) = model.positions[i];
            self.offset = point(px(MARGIN - x), px(MARGIN - y));
        }
    }

    fn on_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        match &mut self.drag {
            Some(Drag::Pan { last }) => {
                self.offset += position - *last;
                *last = position;
            }
            Some(Drag::Table { index, last }) => {
                let delta = position - *last;
                *last = position;
                if let State::Ready(model) = &mut self.state {
                    let (x, y) = &mut model.positions[*index];
                    *x += f32::from(delta.x);
                    *y += f32::from(delta.y);
                }
            }
            None => return,
        }
        cx.notify();
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let band = theme::band_button(cx);
        let summary = match &self.state {
            State::Ready(model) => format!(
                "{} · {} tables · {} relationships",
                self.node.schema,
                model.tables.len(),
                model.edges.len()
            ),
            _ => self.node.schema.clone(),
        };
        h_flex()
            .h(px(34.))
            .px_2()
            .gap_0p5()
            .bg(theme::c(theme::IVREA_GREEN))
            .text_color(theme::c(theme::BAND_INK))
            .child(
                Button::new("erd-reload")
                    .custom(band)
                    .small()
                    .icon(Icon::new(IconName::RefreshCw))
                    .tooltip("Reload from the database")
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            )
            .child(
                Button::new("erd-reset")
                    .custom(band)
                    .small()
                    .icon(Icon::new(Lucide::RotateCcw))
                    .tooltip("Reset layout")
                    .on_click(cx.listener(|this, _, _, cx| this.reset_layout(cx))),
            )
            .child(div().ml_2().text_sm().child(summary))
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .text_color(theme::c(theme::BAND_INK_MUTED))
                    .child("Drag to pan · drag a header to move a table"),
            )
    }

    fn render_table(
        &self,
        index: usize,
        table: &TableInfo,
        (x, y): (f32, f32),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let focused = self.node.table.as_deref() == Some(table.name.as_str());
        let border = if focused {
            theme::c(theme::IVREA_LINE)
        } else {
            theme.border
        };
        let icon = match table.kind {
            TableKind::Table => Icon::new(Lucide::Table),
            TableKind::View => Icon::new(IconName::Eye),
        };
        let muted = theme.muted_foreground;
        let foreign_elsewhere = table
            .foreign_keys
            .iter()
            .filter(|fk| fk.ref_schema != self.node.schema)
            .count();

        v_flex()
            .id(("erd-table", index))
            .absolute()
            .left(self.offset.x + px(x))
            .top(self.offset.y + px(y))
            .w(px(BOX_WIDTH))
            .pb(px(BOX_PADDING))
            .rounded(px(4.))
            .border_1()
            .when(focused, |el| el.border_2())
            .border_color(border)
            .bg(theme.background)
            .shadow_sm()
            .text_xs()
            .child(
                h_flex()
                    .id(("erd-header", index))
                    .h(px(HEADER_HEIGHT))
                    .px_2()
                    .gap_1p5()
                    .rounded_t(px(4.))
                    .bg(theme.secondary)
                    .border_b_1()
                    .border_color(theme.border)
                    .cursor_grab()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                            this.drag = Some(Drag::Table {
                                index,
                                last: e.position,
                            });
                            cx.stop_propagation();
                        }),
                    )
                    .child(icon.small().text_color(muted))
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .font_semibold()
                            .text_sm()
                            .child(table.name.clone()),
                    )
                    .when(foreign_elsewhere > 0, |el| {
                        el.child(
                            div()
                                .text_color(muted)
                                .child(format!("+{foreign_elsewhere} external")),
                        )
                    }),
            )
            .children(table.columns.iter().map(|c| {
                let marker = if table.is_key_column(&c.name) {
                    Some(Icon::new(Lucide::KeyRound))
                } else if table.is_foreign_column(&c.name) {
                    Some(Icon::new(Lucide::Link2))
                } else {
                    None
                };
                h_flex()
                    .h(px(ROW_HEIGHT))
                    .px_2()
                    .gap_1p5()
                    .child(
                        div()
                            .w(px(12.))
                            .children(marker.map(|m| m.xsmall().text_color(muted))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .when(!c.nullable, |el| el.font_medium())
                            .child(c.name.clone()),
                    )
                    .child(
                        div()
                            .max_w(px(110.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(muted)
                            .child(c.data_type.clone()),
                    )
            }))
    }
}

/// Where a line meets a box: the middle of a column row, on one side.
fn anchor(position: (f32, f32), row: usize, right: bool) -> (f32, f32) {
    let x = if right {
        position.0 + BOX_WIDTH
    } else {
        position.0
    };
    let y = position.1 + HEADER_HEIGHT + (row as f32 + 0.5) * ROW_HEIGHT;
    (x, y)
}

/// The line of one foreign key, from the referencing column to the
/// referenced one, with a crow's foot on the referencing ("many") end.
fn paint_edge(
    origin: Point<Pixels>,
    from: (f32, f32),
    to: (f32, f32),
    edge: &Edge,
    color: Hsla,
    window: &mut Window,
) {
    // Leave from the side facing the other box; side by side, loop out right.
    let (from_right, to_right) = if from.0 + BOX_WIDTH < to.0 {
        (true, false)
    } else if to.0 + BOX_WIDTH < from.0 {
        (false, true)
    } else {
        (true, true)
    };
    let a = anchor(from, edge.from_row, from_right);
    let b = anchor(to, edge.to_row, to_right);
    let p = |(x, y): (f32, f32)| origin + point(px(x), px(y));
    let dir = |right: bool| if right { 1. } else { -1. };
    let (da, db) = (dir(from_right), dir(to_right));
    let reach = ((b.0 - a.0).abs() / 2.).max(48.);
    // The foot's prongs start a little out from the box.
    let foot = 12.;
    let start = (a.0 + da * foot, a.1);

    let mut line = PathBuilder::stroke(px(1.25));
    line.move_to(p(start));
    line.cubic_bezier_to(
        p((b.0 + db * 8., b.1)),
        p((start.0 + da * reach, start.1)),
        p((b.0 + db * (reach + 8.), b.1)),
    );
    line.line_to(p(b));
    if let Ok(path) = line.build() {
        window.paint_path(path, color);
    }

    let mut marks = PathBuilder::stroke(px(1.25));
    for dy in [-5., 0., 5.] {
        marks.move_to(p(start));
        marks.line_to(p((a.0, a.1 + dy)));
    }
    // A bar across the "one" end.
    marks.move_to(p((b.0 + db * 6., b.1 - 5.)));
    marks.line_to(p((b.0 + db * 6., b.1 + 5.)));
    if let Ok(path) = marks.build() {
        window.paint_path(path, color);
    }
}

impl Render for ErDiagram {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let body = match &self.state {
            State::Loading => div()
                .p_4()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(
                    if self
                        .data_sources
                        .read(cx)
                        .session(self.node.connection)
                        .is_some_and(|s| s.is_busy())
                    {
                        "Waiting for the running query…"
                    } else {
                        "Loading tables…"
                    },
                )
                .into_any_element(),
            State::Failed(err) => div()
                .p_4()
                .text_sm()
                .text_color(theme.danger)
                .child(err.clone())
                .into_any_element(),
            State::Ready(model) if model.tables.is_empty() => div()
                .p_4()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("This schema has no tables.")
                .into_any_element(),
            State::Ready(model) => {
                let lines: Vec<_> = model
                    .edges
                    .iter()
                    .map(|e| (model.positions[e.from], model.positions[e.to], *e))
                    .collect();
                let offset = self.offset;
                let color = theme.muted_foreground;
                let boxes: Vec<_> = model
                    .tables
                    .iter()
                    .zip(&model.positions)
                    .enumerate()
                    .map(|(i, (table, at))| self.render_table(i, table, *at, cx).into_any_element())
                    .collect();
                div()
                    .size_full()
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                let origin = bounds.origin + offset;
                                for (from, to, edge) in &lines {
                                    paint_edge(origin, *from, *to, edge, color, window);
                                }
                            },
                        )
                        .absolute()
                        .size_full(),
                    )
                    .children(boxes)
                    .into_any_element()
            }
        };

        v_flex().size_full().child(self.render_toolbar(cx)).child(
            div()
                .id("erd-canvas")
                .relative()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .bg(theme.muted)
                .cursor_grab()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, e: &MouseDownEvent, _, _| {
                        this.drag = Some(Drag::Pan { last: e.position });
                    }),
                )
                .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                    if e.pressed_button == Some(MouseButton::Left) {
                        this.on_drag(e.position, cx);
                    } else {
                        this.drag = None;
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| this.drag = None),
                )
                .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, window, cx| {
                    this.offset += e.delta.pixel_delta(window.line_height());
                    cx.notify();
                }))
                .child(body),
        )
    }
}

#[cfg(test)]
impl ErDiagram {
    /// Table names once loaded, and how many lines join them.
    pub fn loaded(&self) -> Option<(Vec<String>, usize)> {
        match &self.state {
            State::Ready(model) => Some((
                model.tables.iter().map(|t| t.name.clone()).collect(),
                model.edges.len(),
            )),
            State::Failed(err) => panic!("diagram failed: {err}"),
            State::Loading => None,
        }
    }

    pub fn focused(&self) -> Option<&str> {
        self.node.table.as_deref()
    }
}
