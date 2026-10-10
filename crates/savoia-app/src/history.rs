//! Query history: the console's past runs, searchable, newest first.
//! Picking one hands its SQL back to the console.

use std::rc::Rc;
use std::time::SystemTime;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_store::HistoryEntry;

use crate::data_sources::DataSources;

/// Entries shown at once; search narrows them down.
const SHOWN: usize = 200;

type OnPick = Rc<dyn Fn(String, &mut Window, &mut App)>;

pub struct HistoryPanel {
    data_sources: Entity<DataSources>,
    search: Entity<InputState>,
    entries: Vec<HistoryEntry>,
    on_pick: OnPick,
}

impl HistoryPanel {
    pub fn new(
        data_sources: Entity<DataSources>,
        on_pick: OnPick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search SQL"));
        cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.reload(cx);
            }
        })
        .detach();
        search.update(cx, |input, cx| input.focus(window, cx));
        let mut this = Self {
            data_sources,
            search,
            entries: Vec::new(),
            on_pick,
        };
        this.reload(cx);
        this
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let search = self.search.read(cx).value().to_string();
        self.entries = self.data_sources.read(cx).history(search.trim(), SHOWN);
        cx.notify();
    }

    #[cfg(test)]
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    #[cfg(test)]
    pub fn search(&self) -> &Entity<InputState> {
        &self.search
    }

    pub fn pick(&self, ix: usize, window: &mut Window, cx: &mut App) {
        if let Some(entry) = self.entries.get(ix) {
            (self.on_pick)(entry.sql.clone(), window, cx);
            window.close_dialog(cx);
        }
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let ds = self.data_sources.read(cx);
        let now = SystemTime::now();
        let rows = self.entries.iter().enumerate().map(|(ix, entry)| {
            let source = ds
                .get(entry.connection)
                .map(|c| c.display_name())
                .unwrap_or_else(|| "deleted data source".into());
            let first_line = entry
                .sql
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("");
            let more = entry.sql.trim().lines().count() > 1;
            let meta = format!(
                "{source} · {} · {} ms · {} rows",
                ago(now, entry.ran_at),
                entry.duration.as_millis(),
                entry.rows
            );
            v_flex()
                .id(("history", ix))
                .w_full()
                .px_2()
                .py_1p5()
                .gap_0p5()
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|el| el.bg(theme.accent))
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .font_family("monospace")
                        .text_sm()
                        .child(if more {
                            format!("{first_line} …")
                        } else {
                            first_line.to_owned()
                        }),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(meta)
                        .when_some(entry.error.clone(), |el, error| {
                            el.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(theme.danger)
                                    .child(error),
                            )
                        }),
                )
                .on_click(cx.listener(move |this, _, window, cx| this.pick(ix, window, cx)))
        });
        v_flex()
            .gap_2()
            .child(Input::new(&self.search).small())
            .child(
                v_flex()
                    .id("history-list")
                    .h(px(420.))
                    .overflow_y_scroll()
                    .children(rows)
                    .when(self.entries.is_empty(), |el| {
                        el.child(
                            div()
                                .p_3()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child("No queries yet. Every run of the console is kept here."),
                        )
                    }),
            )
    }
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
fn ago(now: SystemTime, then: SystemTime) -> String {
    let secs = now.duration_since(then).unwrap_or_default().as_secs();
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86400 => format!("{} h ago", secs / 3600),
        _ => match secs / 86400 {
            1 => "1 day ago".into(),
            days => format!("{days} days ago"),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::ago;

    #[test]
    fn relative_times() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let at = |secs| now - Duration::from_secs(secs);
        assert_eq!(ago(now, at(5)), "just now");
        assert_eq!(ago(now, at(300)), "5 min ago");
        assert_eq!(ago(now, at(7200)), "2 h ago");
        assert_eq!(ago(now, at(86400)), "1 day ago");
        assert_eq!(ago(now, at(3 * 86400)), "3 days ago");
        assert_eq!(ago(now, now + Duration::from_secs(9)), "just now");
    }
}
