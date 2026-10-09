//! Query console: toolbar, SQL editor and the result grid below it.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::resizable::{resizable_panel, v_resizable};
use gpui_kit::component::table::{DataTable, TableState};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::results::ResultSet;
use crate::theme::BandDisabled as _;
use crate::{demo, theme};

pub struct QueryConsole {
    editor: Entity<EditorState>,
    results: Entity<TableState<ResultSet>>,
}

impl QueryConsole {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("sql")
                .default_value(demo::SAMPLE_QUERY)
        });
        let results = cx.new(|cx| TableState::new(ResultSet::empty(), window, cx));
        Self { editor, results }
    }

    fn render_toolbar(&self, cx: &App) -> impl IntoElement {
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
                    .on_click(arrives_in_m2("Query execution")),
            )
            .child(
                tool("cancel")
                    .icon(Icon::new(Lucide::CircleStop))
                    .tooltip("Cancel running query")
                    .band_disabled(true),
            )
            .child(sep())
            .child(
                tool("history")
                    .icon(Icon::new(Lucide::Timer))
                    .tooltip("Query history")
                    .on_click(arrives_in_m2("Query history")),
            )
            .child(
                tool("tx")
                    .label("Tx: Auto")
                    .tooltip("Transaction mode")
                    .on_click(arrives_in_m2("Transaction modes")),
            )
            .child(sep())
            .child(
                tool("explain")
                    .label("Explain")
                    .tooltip("Explain plan")
                    .on_click(arrives_in_m2("Explain plans")),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(theme::c(theme::BAND_INK_MUTED))
                    .child(Icon::new(Lucide::Database).xsmall())
                    .child("savoia.public"),
            )
    }

    fn render_results(&self, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let results = self.results.read(cx).delegate();
        let (has_result, rows) = (results.has_result(), results.len());

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
                    .child(div().text_color(theme.muted_foreground).child("Output"))
                    .when(has_result, |this| {
                        this.child(
                            div()
                                .border_b_2()
                                .border_color(theme::c(theme::IVREA_LINE))
                                .child("Result 1"),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(format!("{rows} rows")),
                        )
                    }),
            )
            .child(if has_result {
                div()
                    .flex_1()
                    .min_h_0()
                    .child(DataTable::new(&self.results).bordered(false))
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .p_3()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Run a query (⌘↩) to see results.")
                    .into_any_element()
            })
    }
}

impl Render for QueryConsole {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full().child(self.render_toolbar(cx)).child(
            div().flex_1().min_h_0().child(
                v_resizable("console-split")
                    .child(
                        resizable_panel().child(Editor::new(&self.editor).bordered(false).h_full()),
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

/// Click handler for console controls whose feature lands in M2, so they say
/// so instead of doing nothing.
fn arrives_in_m2(feature: &'static str) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, window, cx| {
        window.push_notification(
            Notification::info(format!("{feature} arrives in the next milestone (M2).")),
            cx,
        );
    }
}
