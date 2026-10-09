//! Query console: toolbar, SQL editor and the output/result panes below it.
//!
//! Run sends the selection (or the whole editor) to the session selected in
//! the explorer. A reader task moves the query's events into the grid, pausing
//! while the grid has enough rows (see [`Pacer`]).

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::resizable::{resizable_panel, v_resizable};
use gpui_kit::component::table::{DataTable, TableState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_core::{AppError, QueryEvent};

use crate::data_sources::{DataSources, SourceState};
use crate::explorer::Explorer;
use crate::results::{Next, Pacer, ResultSet};
use crate::session::{self, Session};
use crate::theme::BandDisabled as _;
use crate::{runtime, theme};

actions!(console, [RunQuery]);

const CONTEXT: &str = "QueryConsole";

pub fn init(cx: &mut App) {
    // Bound inside the editor too, where ⌘↩ would otherwise be its own Enter.
    cx.bind_keys([
        KeyBinding::new("secondary-enter", RunQuery, Some(CONTEXT)),
        KeyBinding::new(
            "secondary-enter",
            RunQuery,
            Some(&format!("{CONTEXT} > Input")),
        ),
    ]);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Output,
    Result,
}

struct OutputLine {
    text: SharedString,
    error: bool,
}

/// The query in flight. Dropping it drops the reader task and with it the
/// `RunningQuery`, which cancels and drains in the background.
struct Running {
    started: Instant,
    _reader: Task<()>,
}

pub struct QueryConsole {
    data_sources: Entity<DataSources>,
    explorer: Entity<Explorer>,
    editor: Entity<EditorState>,
    results: Entity<TableState<ResultSet>>,
    output: Vec<OutputLine>,
    pane: Pane,
    running: Option<Running>,
    /// Rows and time of the last run, for the pane header.
    summary: Option<SharedString>,
}

impl QueryConsole {
    pub fn new(
        data_sources: Entity<DataSources>,
        explorer: Entity<Explorer>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|cx| EditorState::new(window, cx).language("sql"));
        let results = cx.new(|cx| TableState::new(ResultSet::empty(), window, cx));
        cx.observe(&explorer, |_, _, cx| cx.notify()).detach();
        // Scrolling asks for rows, which can end a pause.
        cx.observe(&results, |_, _, cx| cx.notify()).detach();
        Self {
            data_sources,
            explorer,
            editor,
            results,
            output: Vec::new(),
            pane: Pane::Output,
            running: None,
            summary: None,
        }
    }

    #[cfg(test)]
    pub fn editor(&self) -> &Entity<EditorState> {
        &self.editor
    }

    #[cfg(test)]
    pub fn results(&self) -> &Entity<TableState<ResultSet>> {
        &self.results
    }

    #[cfg(test)]
    pub fn output_lines(&self) -> Vec<String> {
        self.output.iter().map(|l| l.text.to_string()).collect()
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// The session selected in the explorer, if it's connected.
    fn session(&self, cx: &App) -> Result<Arc<Session>, &'static str> {
        let id = self
            .explorer
            .read(cx)
            .selected_connection(cx)
            .ok_or("Select a data source in the explorer to run queries.")?;
        match self.data_sources.read(cx).state(id) {
            SourceState::Connected(session) => Ok(session.clone()),
            _ => Err("Connect the selected data source to run queries."),
        }
    }

    fn sql(&self, cx: &App) -> String {
        let editor = self.editor.read(cx);
        let selected = editor.selected_value();
        let sql = if selected.trim().is_empty() {
            editor.value()
        } else {
            selected
        };
        sql.to_string()
    }

    fn run(&mut self, _: &RunQuery, window: &mut Window, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        let session = match self.session(cx) {
            Ok(session) => session,
            Err(message) => {
                window.push_notification(Notification::warning(message), cx);
                return;
            }
        };
        let sql = self.sql(cx);
        if sql.trim().is_empty() {
            return;
        }

        self.results.update(cx, |table, cx| {
            *table.delegate_mut() = ResultSet::empty();
            table.refresh(cx);
        });
        self.output.clear();
        self.pane = Pane::Output;
        self.summary = None;
        let reader = cx.spawn_in(window, async move |this, cx| {
            let started = session::join(runtime::spawn(async move { session.execute(sql).await }));
            let mut query = match started.await {
                Ok(query) => query,
                Err(err) => {
                    this.update_in(cx, |this, window, cx| this.fail(err, window, cx))
                        .ok();
                    this.update(cx, |this, cx| this.finish(cx)).ok();
                    return;
                }
            };
            // Rows of the current result set; `None` for statements without one.
            let mut loaded: Option<usize> = None;
            let mut skipped = 0;
            let mut pacer: Option<Arc<Pacer>> = None;
            while let Some(event) = query.next().await {
                let alive = match event {
                    Ok(QueryEvent::Columns(meta)) => {
                        let fresh = Pacer::new();
                        pacer = Some(fresh.clone());
                        loaded = Some(0);
                        skipped = 0;
                        this.update(cx, |this, cx| this.start_result(meta, fresh, cx))
                    }
                    Ok(QueryEvent::Rows(rows)) => {
                        let total = loaded.get_or_insert(0);
                        // Wait with this page in hand rather than after adding
                        // it, so an end that follows the last page isn't held up.
                        let next = match &pacer {
                            Some(pacer) => pacer.room_for(*total).await,
                            None => Next::Load,
                        };
                        if next == Next::Skip {
                            skipped += rows.len();
                            continue;
                        }
                        *total += rows.len();
                        let total = *total;
                        this.update(cx, |this, cx| this.add_rows(rows, total, cx))
                    }
                    Ok(QueryEvent::Done {
                        rows_affected,
                        elapsed,
                    }) => {
                        let rows = loaded.take();
                        pacer = None;
                        this.update(cx, |this, cx| {
                            this.statement_done(rows, skipped, rows_affected, elapsed, cx)
                        })
                    }
                    Err(err) => this.update_in(cx, |this, window, cx| this.fail(err, window, cx)),
                };
                if alive.is_err() {
                    return;
                }
            }
            this.update(cx, |this, cx| this.finish(cx)).ok();
        });
        self.running = Some(Running {
            started: Instant::now(),
            _reader: reader,
        });
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        let Some(running) = self.running.take() else {
            return;
        };
        drop(running);
        let rows = self.results.read(cx).delegate().len();
        self.log(format!("Cancelled after {}", count(rows, "row")), false);
        self.stop_grid(cx);
        self.summary = Some(format!("{} · cancelled", count(rows, "row")).into());
        cx.notify();
    }

    fn start_result(
        &mut self,
        meta: Arc<[savoia_core::ColumnMeta]>,
        pacer: Arc<Pacer>,
        cx: &mut Context<Self>,
    ) {
        self.results.update(cx, |table, cx| {
            table.delegate_mut().start(meta, pacer);
            table.refresh(cx);
            cx.notify();
        });
        self.pane = Pane::Result;
        cx.notify();
    }

    fn add_rows(&mut self, rows: Vec<savoia_core::Row>, total: usize, cx: &mut Context<Self>) {
        self.results.update(cx, |table, cx| {
            table.delegate_mut().extend(rows);
            cx.notify();
        });
        self.summary = Some(format!("{}…", count(total, "row")).into());
        cx.notify();
    }

    fn statement_done(
        &mut self,
        rows: Option<usize>,
        skipped: usize,
        rows_affected: Option<u64>,
        elapsed: Duration,
        cx: &mut Context<Self>,
    ) {
        let what = match (rows, rows_affected) {
            (Some(rows), _) if skipped > 0 => {
                format!("{} ({} skipped)", count(rows, "row"), count(skipped, "row"))
            }
            (Some(rows), _) => count(rows, "row"),
            (None, Some(affected)) => format!("{} affected", count(affected as usize, "row")),
            (None, None) => "OK".to_string(),
        };
        self.log(format!("{what} · {}", duration(elapsed)), false);
        if rows.is_some() {
            self.stop_grid(cx);
        }
        cx.notify();
    }

    fn fail(&mut self, err: AppError, window: &mut Window, cx: &mut Context<Self>) {
        let message = err.to_string();
        self.log(message.clone(), true);
        self.pane = Pane::Output;
        window.push_notification(Notification::error(message), cx);
        cx.notify();
    }

    /// The execution ended (all statements done, or an error).
    fn finish(&mut self, cx: &mut Context<Self>) {
        if let Some(running) = self.running.take() {
            self.stop_grid(cx);
            let rows = self.results.read(cx).delegate();
            let rows = rows.has_result().then(|| count(rows.len(), "row"));
            let time = duration(running.started.elapsed());
            self.summary = Some(
                match rows {
                    Some(rows) => format!("{rows} · {time}"),
                    None => time,
                }
                .into(),
            );
        }
        cx.notify();
    }

    fn stop_grid(&self, cx: &mut Context<Self>) {
        self.results.update(cx, |table, cx| {
            table.delegate_mut().finish();
            cx.notify();
        });
    }

    fn log(&mut self, text: String, error: bool) {
        self.output.push(OutputLine {
            text: text.into(),
            error,
        });
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // The console's Ivrea band: the zone you run things from. Run is the
        // app's one Savoy-blue control.
        let sep = || {
            div()
                .w(px(1.))
                .h(px(16.))
                .mx_1()
                .bg(theme::c(theme::BAND_RULE))
        };
        let tool = |id: &'static str| Button::new(id).custom(theme::band_button(cx)).small();
        let running = self.is_running();
        let source = self
            .explorer
            .read(cx)
            .selected_connection(cx)
            .and_then(|id| self.data_sources.read(cx).get(id))
            .map(|c| c.display_name());

        h_flex()
            .h(px(34.))
            .px_2()
            .gap_0p5()
            .bg(theme::c(theme::IVREA_GREEN))
            .text_color(theme::c(theme::BAND_INK))
            .child(
                Button::new("run")
                    .custom(theme::run_button(cx))
                    .bg(theme::run_fill())
                    .small()
                    .icon(Icon::new(IconName::Play))
                    .label("Run")
                    .tooltip("Execute (⌘↩)")
                    .disabled(running)
                    .on_click(cx.listener(|this, _, window, cx| this.run(&RunQuery, window, cx))),
            )
            .child(
                tool("cancel")
                    .icon(Icon::new(Lucide::CircleStop))
                    .tooltip("Cancel running query")
                    .band_disabled(!running)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .child(sep())
            .child(
                tool("history")
                    .icon(Icon::new(Lucide::Timer))
                    .tooltip("Query history")
                    .on_click(coming_later("Query history")),
            )
            .child(
                tool("tx")
                    .label("Tx: Auto")
                    .tooltip("Transaction mode")
                    .on_click(coming_later("Transaction modes")),
            )
            .child(sep())
            .child(
                tool("explain")
                    .label("Explain")
                    .tooltip("Explain plan")
                    .on_click(coming_later("Explain plans")),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(theme::c(theme::BAND_INK_MUTED))
                    .child(Icon::new(Lucide::Database).xsmall())
                    .child(source.unwrap_or_else(|| "no data source".to_string())),
            )
    }

    fn render_results(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let results = self.results.read(cx).delegate();
        let (has_result, paused) = (results.has_result(), results.is_paused());
        let pacer = results.pacer().cloned();
        let pane_tab = |id: &'static str, label: &'static str, pane: Pane| {
            let selected = self.pane == pane;
            div()
                .id(id)
                .cursor_pointer()
                .border_b_2()
                .border_color(if selected {
                    theme::c(theme::IVREA_LINE)
                } else {
                    transparent_black()
                })
                .when(!selected, |this| this.text_color(theme.muted_foreground))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.pane = pane;
                    cx.notify();
                }))
        };

        let body = match self.pane {
            Pane::Result if has_result => div()
                .flex_1()
                .min_h_0()
                .child(DataTable::new(&self.results).bordered(false))
                .into_any_element(),
            _ if self.output.is_empty() => div()
                .flex_1()
                .p_3()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(if self.is_running() {
                    "Running…"
                } else {
                    "Run a query (⌘↩) to see results."
                })
                .into_any_element(),
            _ => v_flex()
                .id("output")
                .flex_1()
                .min_h_0()
                .p_3()
                .gap_1()
                .overflow_y_scroll()
                .font_family("monospace")
                .text_sm()
                .children(self.output.iter().map(|line| {
                    div()
                        .when(line.error, |this| this.text_color(theme.danger))
                        .child(line.text.clone())
                }))
                .into_any_element(),
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
                    .child(pane_tab("pane-output", "Output", Pane::Output))
                    .when(has_result, |this| {
                        this.child(pane_tab("pane-result", "Result 1", Pane::Result))
                    })
                    .child(div().flex_1())
                    .when_some(pacer.filter(|_| paused), |this, pacer| {
                        let skip = pacer.clone();
                        this.child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child("Paused · statements after this one wait"),
                        )
                        .child(
                            Button::new("load-all")
                                .ghost()
                                .xsmall()
                                .label("Load all")
                                .tooltip("Load every remaining row of this result")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    pacer.load_all();
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("skip-rest")
                                .ghost()
                                .xsmall()
                                .label("Skip rest")
                                .tooltip("Discard the remaining rows and continue")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    skip.skip_rest();
                                    cx.notify();
                                })),
                        )
                    })
                    .children(
                        self.summary
                            .clone()
                            .map(|s| div().text_color(theme.muted_foreground).child(s)),
                    ),
            )
            .child(body)
    }
}

impl Render for QueryConsole {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::run))
            .size_full()
            .child(self.render_toolbar(cx))
            .child(
                div().flex_1().min_h_0().child(
                    v_resizable("console-split")
                        .child(
                            resizable_panel()
                                .child(Editor::new(&self.editor).bordered(false).h_full()),
                        )
                        .child(
                            resizable_panel()
                                .size(px(300.))
                                .size_range(px(120.)..px(2000.))
                                .child(self.render_results(cx)),
                        ),
                ),
            )
    }
}

/// "1 row", "1,234 rows".
fn count(n: usize, noun: &str) -> String {
    let digits = n.to_string();
    let mut grouped = String::new();
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(d);
    }
    let plural = if n == 1 { "" } else { "s" };
    format!("{grouped} {noun}{plural}")
}

/// "12 ms", "3.40 s".
fn duration(d: Duration) -> String {
    if d < Duration::from_secs(1) {
        format!("{} ms", d.as_millis())
    } else {
        format!("{:.2} s", d.as_secs_f64())
    }
}

/// Click handler for console controls whose feature lands later in M2, so
/// they say so instead of doing nothing.
fn coming_later(feature: &'static str) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, window, cx| {
        window.push_notification(
            Notification::info(format!("{feature} isn't available yet.")),
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{count, duration};

    #[test]
    fn formats_counts_and_durations() {
        assert_eq!(count(1, "row"), "1 row");
        assert_eq!(count(0, "row"), "0 rows");
        assert_eq!(count(1234567, "row"), "1,234,567 rows");
        assert_eq!(duration(Duration::from_millis(12)), "12 ms");
        assert_eq!(duration(Duration::from_millis(3400)), "3.40 s");
    }
}
