//! ER diagram of one schema: tables as boxes, foreign keys as lines. See
//! `docs/adr/202610091437-draw-er-diagrams-natively-with-a-built-in-layered-layout.md`.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_core::{TableInfo, TableKind};

use crate::data_sources::DataSources;
use crate::erd_layout::{
    self, BOX_PADDING, BOX_WIDTH, Edge, HEADER_HEIGHT, ROW_HEIGHT, Viewport, box_height,
};
use crate::explorer::NodeRef;
use crate::runtime;
use crate::session;
use crate::theme::{self, BandDisabled as _};

/// Space left around the diagram when it is fitted or first shown.
const MARGIN: f32 = 32.;
/// Redraws from dragging, panning and zooming are capped at 60 per second.
const FRAME: Duration = Duration::from_micros(16_667);
/// Zoom steps of the toolbar buttons.
const ZOOM_STEP: f32 = 1.25;
/// Blueprint grid spacing, in diagram units: a minor line every 20, a major
/// one every 100.
const GRID_MINOR: f32 = 20.;
const GRID_MAJOR: f32 = 100.;
/// Minor lines are left out when they would be closer than this on screen.
const GRID_MIN_GAP: f32 = 6.;

/// Below this zoom, column rows are drawn as blank space: their text would
/// be unreadable, and laying it out is most of the cost of a big diagram.
const DETAIL_ZOOM: f32 = 0.45;

/// A screen rectangle: x, y, width, height.
type Rect = (f32, f32, f32, f32);

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

/// Holds redraws to one per [`FRAME`].
#[derive(Default)]
struct FrameLimiter {
    last_frame: Option<Instant>,
    /// A redraw waiting for the next frame slot.
    scheduled: Option<Task<()>>,
}

pub struct ErDiagram {
    data_sources: Entity<DataSources>,
    node: NodeRef,
    state: State,
    viewport: Viewport,
    /// Tables showing only their header, by name, so they survive a reload.
    collapsed: HashSet<String>,
    drag: Option<Drag>,
    /// Where the diagram area was last painted, in window coordinates:
    /// x, y, width, height. Written while painting.
    view: Rc<Cell<Option<Rect>>>,
    /// The first placement (fit, or the focused table) waits for the view's size.
    needs_placement: bool,
    limiter: FrameLimiter,
    _load: Option<Task<()>>,
}

impl ErDiagram {
    pub fn new(data_sources: Entity<DataSources>, node: NodeRef, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            data_sources,
            node,
            state: State::Loading,
            viewport: Viewport::new((MARGIN, MARGIN)),
            collapsed: HashSet::new(),
            drag: None,
            view: Rc::default(),
            needs_placement: false,
            limiter: FrameLimiter::default(),
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
        self.needs_placement = true;
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
                        let mut model = Model {
                            tables,
                            edges,
                            positions: Vec::new(),
                        };
                        model.positions = erd_layout::layout(&this.heights(&model), &model.edges);
                        State::Ready(model)
                    }
                    Err(err) => State::Failed(err.to_string()),
                };
                this.needs_placement = true;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn heights(&self, model: &Model) -> Vec<f32> {
        model
            .tables
            .iter()
            .map(|t| box_height(t, self.collapsed.contains(&t.name)))
            .collect()
    }

    /// Width and height of the diagram area, once it has been painted.
    fn view_size(&self) -> Option<(f32, f32)> {
        self.view.get().map(|(_, _, w, h)| (w, h))
    }

    /// Fits the diagram, or centers the focused table at 100%. `false` until
    /// the view's size is known.
    fn place(&mut self) -> bool {
        let Some((w, h)) = self.view_size() else {
            return false;
        };
        let State::Ready(model) = &self.state else {
            return true;
        };
        let focus = self
            .node
            .table
            .as_ref()
            .and_then(|t| model.tables.iter().position(|m| m.name == *t));
        let heights = self.heights(model);
        self.viewport = match focus {
            Some(i) => {
                let (x, y) = model.positions[i];
                let mut v = Viewport::new((0., 0.));
                v.offset = (w / 2. - x - BOX_WIDTH / 2., h / 2. - y - heights[i] / 2.);
                v
            }
            None => match erd_layout::extent(&model.positions, &heights) {
                Some(extent) => Viewport::fit(extent, (w, h), MARGIN),
                None => Viewport::new((MARGIN, MARGIN)),
            },
        };
        true
    }

    fn fit(&mut self, cx: &mut Context<Self>) {
        let focus = self.node.table.take();
        self.place();
        self.node.table = focus;
        cx.notify();
    }

    fn reset_layout(&mut self, cx: &mut Context<Self>) {
        if let State::Ready(model) = &self.state {
            let positions = erd_layout::layout(&self.heights(model), &model.edges);
            if let State::Ready(model) = &mut self.state {
                model.positions = positions;
            }
        }
        self.fit(cx);
    }

    /// Zooms around the middle of the view.
    fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let (w, h) = self.view_size().unwrap_or((0., 0.));
        self.viewport.zoom_at(factor, (w / 2., h / 2.));
        self.redraw(cx);
    }

    fn toggle_collapsed(&mut self, table: String, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&table) {
            self.collapsed.insert(table);
        }
        cx.notify();
    }

    fn all_collapsed(&self) -> bool {
        match &self.state {
            State::Ready(model) => model
                .tables
                .iter()
                .all(|t| self.collapsed.contains(&t.name)),
            _ => false,
        }
    }

    fn set_all_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        self.collapsed.clear();
        if collapsed && let State::Ready(model) = &self.state {
            self.collapsed
                .extend(model.tables.iter().map(|t| t.name.clone()));
        }
        cx.notify();
    }

    /// Asks for a redraw, at most one per [`FRAME`]; state changes right
    /// away, only the drawing waits.
    fn redraw(&mut self, cx: &mut Context<Self>) {
        if self.limiter.scheduled.is_some() {
            return;
        }
        let wait = self
            .limiter
            .last_frame
            .map_or(Duration::ZERO, |t| FRAME.saturating_sub(t.elapsed()));
        if wait.is_zero() {
            cx.notify();
            return;
        }
        self.limiter.scheduled = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            this.update(cx, |this, cx| {
                this.limiter.scheduled = None;
                cx.notify();
            })
            .ok();
        }));
    }

    fn on_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let zoom = self.viewport.zoom;
        match &mut self.drag {
            Some(Drag::Pan { last }) => {
                let delta = position - *last;
                *last = position;
                self.viewport.offset.0 += f32::from(delta.x);
                self.viewport.offset.1 += f32::from(delta.y);
            }
            Some(Drag::Table { index, last }) => {
                let delta = position - *last;
                *last = position;
                if let State::Ready(model) = &mut self.state {
                    let (x, y) = &mut model.positions[*index];
                    *x += f32::from(delta.x) / zoom;
                    *y += f32::from(delta.y) / zoom;
                }
            }
            None => return,
        }
        self.redraw(cx);
    }

    fn on_scroll(&mut self, e: &ScrollWheelEvent, window: &Window, cx: &mut Context<Self>) {
        let delta = e.delta.pixel_delta(window.line_height());
        if e.modifiers.platform || e.modifiers.control {
            let (x, y, _, _) = self.view.get().unwrap_or_default();
            let anchor = (f32::from(e.position.x) - x, f32::from(e.position.y) - y);
            self.viewport
                .zoom_at((f32::from(delta.y) * 0.005).exp(), anchor);
        } else {
            self.viewport.offset.0 += f32::from(delta.x);
            self.viewport.offset.1 += f32::from(delta.y);
        }
        self.redraw(cx);
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let band = theme::band_button(cx);
        let ready = matches!(self.state, State::Ready(_));
        let button = move |id: &'static str, icon: Icon, tooltip: &'static str| {
            Button::new(id)
                .custom(band)
                .small()
                .icon(icon)
                .tooltip(tooltip)
                .band_disabled(!ready)
        };
        let summary = match &self.state {
            State::Ready(model) => format!(
                "{} · {} tables · {} relationships",
                self.node.schema,
                model.tables.len(),
                model.edges.len()
            ),
            _ => self.node.schema.clone(),
        };
        let all_collapsed = self.all_collapsed();
        let rule = || {
            div()
                .w(px(1.))
                .h(px(16.))
                .mx_1()
                .bg(theme::c(theme::BAND_RULE))
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
                button("erd-reset", Icon::new(Lucide::RotateCcw), "Reset layout")
                    .on_click(cx.listener(|this, _, _, cx| this.reset_layout(cx))),
            )
            .child(rule())
            .child(
                button("erd-zoom-out", Icon::new(Lucide::ZoomOut), "Zoom out")
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_by(1. / ZOOM_STEP, cx))),
            )
            .child(
                Button::new("erd-zoom-reset")
                    .custom(band)
                    .small()
                    .label(format!("{:.0}%", self.viewport.zoom * 100.))
                    .tooltip("Zoom to 100%")
                    .band_disabled(!ready)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let zoom = this.viewport.zoom;
                        this.zoom_by(1. / zoom, cx);
                    })),
            )
            .child(
                button("erd-zoom-in", Icon::new(Lucide::ZoomIn), "Zoom in")
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_by(ZOOM_STEP, cx))),
            )
            .child(
                button("erd-fit", Icon::new(Lucide::Scan), "Fit to view")
                    .on_click(cx.listener(|this, _, _, cx| this.fit(cx))),
            )
            .child(rule())
            .child(
                if all_collapsed {
                    button(
                        "erd-collapse-all",
                        Icon::new(Lucide::ChevronsUpDown),
                        "Show all columns",
                    )
                } else {
                    button(
                        "erd-collapse-all",
                        Icon::new(Lucide::ChevronsDownUp),
                        "Collapse all tables to their names",
                    )
                }
                .on_click(
                    cx.listener(move |this, _, _, cx| this.set_all_collapsed(!all_collapsed, cx)),
                ),
            )
            .child(div().ml_2().text_sm().child(summary))
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .text_color(theme::c(theme::BAND_INK_MUTED))
                    .child("Drag to pan · ⌘/Ctrl + scroll to zoom"),
            )
    }

    fn render_table(
        &self,
        index: usize,
        table: &TableInfo,
        (x, y): (f32, f32),
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let z = self.viewport.zoom;
        let s = |v: f32| px(v * z);
        let focused = self.node.table.as_deref() == Some(table.name.as_str());
        let border = if focused {
            cx.theme().ring
        } else {
            theme.border
        };
        let icon = match table.kind {
            TableKind::Table => Icon::new(Lucide::Table),
            TableKind::View => Icon::new(IconName::Eye),
        };
        let chevron = if collapsed {
            IconName::ChevronRight
        } else {
            IconName::ChevronDown
        };
        let muted = theme.muted_foreground;
        let foreign_elsewhere = table
            .foreign_keys
            .iter()
            .filter(|fk| fk.ref_schema != self.node.schema)
            .count();
        let name = table.name.clone();
        let rows = table.columns.len().max(1) as f32;

        v_flex()
            .id(("erd-table", index))
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(s(BOX_WIDTH))
            .when(!collapsed, |el| el.pb(s(BOX_PADDING)))
            .rounded(s(4.))
            .border_1()
            .when(focused, |el| el.border_2())
            .border_color(border)
            .bg(theme.background)
            .shadow_sm()
            .text_size(s(12.))
            .child(
                h_flex()
                    .id(("erd-header", index))
                    .h(s(HEADER_HEIGHT))
                    .px(s(6.))
                    .gap(s(5.))
                    .when(collapsed, |el| el.rounded(s(4.)))
                    .when(!collapsed, |el| {
                        el.rounded_t(s(4.)).border_b_1().border_color(theme.border)
                    })
                    .bg(theme.secondary)
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
                    .child(
                        div()
                            .id(("erd-collapse", index))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_collapsed(name.clone(), cx)
                            }))
                            .child(Icon::new(chevron).size(s(12.)).text_color(muted)),
                    )
                    .child(icon.size(s(13.)).text_color(muted))
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .text_ellipsis()
                            .font_semibold()
                            .text_size(s(13.))
                            .child(table.name.clone()),
                    )
                    .when(foreign_elsewhere > 0, |el| {
                        el.child(
                            div()
                                .text_color(muted)
                                .child(format!("+{foreign_elsewhere} external")),
                        )
                    })
                    .when(collapsed, |el| {
                        el.child(
                            div()
                                .text_color(muted)
                                .child(table.columns.len().to_string()),
                        )
                    }),
            )
            .when(!collapsed && z < DETAIL_ZOOM, |el| {
                el.child(div().h(s(rows * ROW_HEIGHT)))
            })
            .when(!collapsed && z >= DETAIL_ZOOM, |el| {
                el.children(table.columns.iter().map(|c| {
                    let marker = if table.is_key_column(&c.name) {
                        Some(Icon::new(Lucide::KeyRound))
                    } else if table.is_foreign_column(&c.name) {
                        Some(Icon::new(Lucide::Link2))
                    } else {
                        None
                    };
                    h_flex()
                        .h(s(ROW_HEIGHT))
                        .px(s(8.))
                        .gap(s(6.))
                        .child(
                            div()
                                .w(s(12.))
                                .children(marker.map(|m| m.size(s(11.)).text_color(muted))),
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
                                .max_w(s(110.))
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_color(muted)
                                .child(c.data_type.clone()),
                        )
                }))
            })
    }
}

/// Where a line meets a box, in diagram units: the middle of a column row on
/// one side, or of the header when the box is collapsed.
fn anchor(position: (f32, f32), row: usize, collapsed: bool, right: bool) -> (f32, f32) {
    let x = if right {
        position.0 + BOX_WIDTH
    } else {
        position.0
    };
    let y = if collapsed {
        position.1 + HEADER_HEIGHT / 2.
    } else {
        position.1 + HEADER_HEIGHT + (row as f32 + 0.5) * ROW_HEIGHT
    };
    (x, y)
}

/// One foreign key's line, ready to paint: its ends in diagram units.
struct Line {
    from: (f32, f32),
    to: (f32, f32),
    from_collapsed: bool,
    to_collapsed: bool,
    edge: Edge,
}

/// The line of one foreign key, from the referencing column to the
/// referenced one, with a crow's foot on the referencing ("many") end.
fn paint_line(origin: Point<Pixels>, v: Viewport, line: &Line, color: Hsla, window: &mut Window) {
    // Leave from the side facing the other box; side by side, loop out right.
    let (from_right, to_right) = if line.from.0 + BOX_WIDTH < line.to.0 {
        (true, false)
    } else if line.to.0 + BOX_WIDTH < line.from.0 {
        (false, true)
    } else {
        (true, true)
    };
    let a = v.to_screen(anchor(
        line.from,
        line.edge.from_row,
        line.from_collapsed,
        from_right,
    ));
    let b = v.to_screen(anchor(
        line.to,
        line.edge.to_row,
        line.to_collapsed,
        to_right,
    ));
    let z = v.zoom;
    let p = |(x, y): (f32, f32)| origin + point(px(x), px(y));
    let dir = |right: bool| if right { 1. } else { -1. };
    let (da, db) = (dir(from_right), dir(to_right));
    let reach = ((b.0 - a.0).abs() / 2.).max(48. * z);
    // The foot's prongs start a little out from the box.
    let start = (a.0 + da * 12. * z, a.1);
    let stroke = px(1.25_f32.min(1.25 * z.max(0.6)));

    let mut path = PathBuilder::stroke(stroke);
    path.move_to(p(start));
    path.cubic_bezier_to(
        p((b.0 + db * 8. * z, b.1)),
        p((start.0 + da * reach, start.1)),
        p((b.0 + db * (reach + 8. * z), b.1)),
    );
    path.line_to(p(b));
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }

    let mut marks = PathBuilder::stroke(stroke);
    for dy in [-5., 0., 5.] {
        marks.move_to(p(start));
        marks.line_to(p((a.0, a.1 + dy * z)));
    }
    // A bar across the "one" end.
    marks.move_to(p((b.0 + db * 6. * z, b.1 - 5. * z)));
    marks.line_to(p((b.0 + db * 6. * z, b.1 + 5. * z)));
    if let Ok(path) = marks.build() {
        window.paint_path(path, color);
    }
}

/// Rules the blueprint: vertical and horizontal hairlines that move and scale
/// with the diagram, major ones every [`GRID_MAJOR`] units.
fn paint_grid(bounds: &Bounds<Pixels>, v: Viewport, window: &mut Window) {
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let ink = |opacity| Hsla {
        a: opacity,
        ..theme::c(theme::BAND_INK)
    };
    let (minor, major) = (ink(0.07), ink(0.16));
    let step = GRID_MINOR * v.zoom;
    let every = if step >= GRID_MIN_GAP {
        1
    } else {
        (GRID_MAJOR / GRID_MINOR) as i64
    };
    // Index of each line in minor steps, so majors stay put while panning.
    let mut lines = |offset: f32, length: f32, vertical: bool| {
        let first = (-offset / step).floor() as i64;
        let last = ((length - offset) / step).ceil() as i64;
        for k in first..=last {
            if k % every != 0 {
                continue;
            }
            let at = offset + k as f32 * step;
            let color = if k % (GRID_MAJOR / GRID_MINOR) as i64 == 0 {
                major
            } else {
                minor
            };
            let rect = if vertical {
                Bounds::new(bounds.origin + point(px(at), px(0.)), size(px(1.), px(h)))
            } else {
                Bounds::new(bounds.origin + point(px(0.), px(at)), size(px(w), px(1.)))
            };
            window.paint_quad(fill(rect, color));
        }
    };
    lines(v.offset.0, w, true);
    lines(v.offset.1, h, false);
}

impl Render for ErDiagram {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.limiter.last_frame = Some(Instant::now());
        if self.needs_placement {
            if self.place() {
                self.needs_placement = false;
            } else {
                // Placed on the next frame, once this one gives the view a size.
                cx.on_next_frame(window, |_, _, cx| cx.notify());
            }
        }

        let theme = cx.theme().clone();
        let body = match &self.state {
            State::Loading => div()
                .p_4()
                .text_sm()
                .text_color(theme::c(theme::BAND_INK_MUTED))
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
                .text_color(theme::c(theme::BAND_INK_MUTED))
                .child("This schema has no tables.")
                .into_any_element(),
            State::Ready(model) => {
                let shut = |i: usize| self.collapsed.contains(&model.tables[i].name);
                let lines: Vec<Line> = model
                    .edges
                    .iter()
                    .map(|e| Line {
                        from: model.positions[e.from],
                        to: model.positions[e.to],
                        from_collapsed: shut(e.from),
                        to_collapsed: shut(e.to),
                        edge: *e,
                    })
                    .collect();
                let viewport = self.viewport;
                let color = theme::c(theme::BAND_INK_MUTED);
                let size = self.view_size();
                let z = viewport.zoom;
                // Only boxes that overlap the view are laid out.
                let boxes: Vec<_> = (0..model.tables.len())
                    .filter_map(|i| {
                        let table = &model.tables[i];
                        let collapsed = shut(i);
                        let (x, y) = viewport.to_screen(model.positions[i]);
                        let (w, h) = (BOX_WIDTH * z, box_height(table, collapsed) * z);
                        let visible = size
                            .is_none_or(|(vw, vh)| x < vw && y < vh && x + w > 0. && y + h > 0.);
                        visible.then(|| {
                            self.render_table(i, table, (x, y), collapsed, cx)
                                .into_any_element()
                        })
                    })
                    .collect();
                div()
                    .size_full()
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                for line in &lines {
                                    paint_line(bounds.origin, viewport, line, color, window);
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

        let view = self.view.clone();
        let viewport = self.viewport;
        v_flex().size_full().child(self.render_toolbar(cx)).child(
            div()
                .id("erd-canvas")
                .relative()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .bg(theme::c(theme::BLUEPRINT))
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
                    this.on_scroll(e, window, cx)
                }))
                // Rules the board, and records the area's bounds for placement,
                // zoom anchors and culling.
                .child(
                    canvas(
                        move |bounds, _, _| {
                            view.set(Some((
                                f32::from(bounds.origin.x),
                                f32::from(bounds.origin.y),
                                f32::from(bounds.size.width),
                                f32::from(bounds.size.height),
                            )))
                        },
                        move |bounds, _, window, _| paint_grid(&bounds, viewport, window),
                    )
                    .absolute()
                    .size_full(),
                )
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

    pub fn zoom(&self) -> f32 {
        self.viewport.zoom
    }

    pub fn collapsed_count(&self) -> usize {
        self.collapsed.len()
    }

    /// Feeds `n` 1px pan moves as fast as possible; `true` if the redraw
    /// cap is holding a redraw back for the next frame slot.
    pub fn drag_moves(&mut self, n: usize, cx: &mut Context<Self>) -> bool {
        self.drag = Some(Drag::Pan {
            last: point(px(0.), px(0.)),
        });
        for i in 1..=n {
            self.on_drag(point(px(i as f32), px(0.)), cx);
        }
        self.drag = None;
        self.limiter.scheduled.is_some()
    }
}
