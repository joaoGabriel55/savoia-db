//! Main window: title bar, explorer | console tabs, status bar. Also turns
//! data-source events into notifications and the host-key trust prompt.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, TitleBar, WindowExt as _,
    h_flex, v_flex,
};
use gpui_kit::*;
use savoia_tunnel::HostKeyPolicy;

use crate::console::QueryConsole;
use crate::data_sources::{DataSources, DataSourcesEvent, SourceState};
use crate::explorer::Explorer;
use crate::theme;

pub struct Workspace {
    data_sources: Entity<DataSources>,
    explorer: Entity<Explorer>,
    console: Entity<QueryConsole>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let data_sources = crate::data_sources::init(cx);
        let explorer = cx.new(|cx| Explorer::new(data_sources.clone(), cx));
        let subscriptions = vec![
            cx.observe(&explorer, |_, _, cx| cx.notify()),
            cx.observe(&data_sources, |_, _, cx| cx.notify()),
            cx.subscribe_in(&data_sources, window, Self::on_data_source_event),
        ];
        Self {
            console: cx.new(|cx| QueryConsole::new(window, cx)),
            data_sources,
            explorer,
            _subscriptions: subscriptions,
        }
    }

    fn on_data_source_event(
        &mut self,
        data_sources: &Entity<DataSources>,
        event: &DataSourcesEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            DataSourcesEvent::Changed => cx.notify(),
            DataSourcesEvent::Error(message) => {
                window.push_notification(Notification::error(message.clone()).autohide(false), cx);
            }
            DataSourcesEvent::NeedsHostTrust {
                id,
                host,
                fingerprint,
            } => {
                let (id, data_sources) = (*id, data_sources.clone());
                let description = format!(
                    "The authenticity of {host} can't be established.\n{fingerprint}\n\n\
                     Trust it only if this matches the server's key. It will be added to ~/.ssh/known_hosts."
                );
                window.open_alert_dialog(cx, move |alert, _, cx| {
                    let data_sources = data_sources.clone();
                    alert
                        .icon(Icon::new(IconName::TriangleAlert).text_color(cx.theme().warning))
                        .title("Unknown SSH host")
                        .description(description.clone())
                        .show_cancel(true)
                        .ok_text("Trust and connect")
                        .on_ok(move |_, _, cx| {
                            data_sources.update(cx, |ds, cx| {
                                ds.connect(id, HostKeyPolicy::TrustUnknown, cx)
                            });
                            true
                        })
                });
            }
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;

        let ds = self.data_sources.read(cx);
        let selected = self
            .explorer
            .read(cx)
            .selected_connection(cx)
            .and_then(|id| ds.get(id).cloned());
        let source_name = selected.as_ref().map(|c| c.display_name());
        let source_color = selected
            .as_ref()
            .and_then(|c| c.color)
            .map_or(muted, |c| rgb(c.rgb()).into());
        let (status_icon, status_text) = match selected.as_ref().map(|c| (c, ds.state(c.id))) {
            None => (Lucide::Plug, "No data source selected".to_string()),
            Some((c, SourceState::Disconnected)) => (
                Lucide::Unplug,
                format!("{} · disconnected", c.display_name()),
            ),
            Some((c, SourceState::Connecting)) => {
                (Lucide::Plug, format!("{} · connecting…", c.display_name()))
            }
            Some((c, SourceState::Failed(err))) => {
                (Lucide::Unplug, format!("{} · {err}", c.display_name()))
            }
            Some((c, SourceState::Connected(session))) => (
                Lucide::Plug,
                format!(
                    "{} · {}{}",
                    c.display_name(),
                    session.catalog.server.version,
                    if c.read_only { " · read-only" } else { "" }
                ),
            ),
        };

        let title_bar = TitleBar::new().child(
            h_flex()
                .gap_2()
                .text_sm()
                .child(
                    // The app icon's mark, without its blue dot: Run stays the
                    // window's only Savoy blue.
                    h_flex()
                        .size(px(16.))
                        .rounded(px(4.))
                        .justify_center()
                        .bg(theme::c(theme::IVREA_GREEN))
                        .child(
                            Icon::new(Lucide::Database)
                                .size(px(11.))
                                .text_color(theme::c(theme::BAND_INK)),
                        ),
                )
                .child(div().font_semibold().child("Savoia DB"))
                .children(
                    source_name
                        .clone()
                        .map(|name| div().text_color(muted).child(name)),
                ),
        );

        // The connection color carries onto the console tab, like Beekeeper's
        // colored connections, so prod vs local is visible at a glance.
        let console_tab = Tab::new()
            .label(match &source_name {
                Some(name) => format!("console [{name}]"),
                None => "console".to_string(),
            })
            .prefix(
                Icon::new(Lucide::Database)
                    .small()
                    .ml_2()
                    .text_color(source_color),
            );

        let main = v_flex()
            .size_full()
            .child(
                TabBar::new("consoles")
                    .underline()
                    .child(console_tab)
                    .selected_index(0),
            )
            .child(div().flex_1().min_h_0().child(self.console.clone()));

        let status = StatusBar::new()
            .left(
                h_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(muted)
                    .child(Icon::new(status_icon).xsmall())
                    .child(status_text),
            )
            .right(div().text_xs().text_color(muted).child("SQL · UTF-8"));

        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(title_bar)
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("workspace")
                        .child(
                            resizable_panel()
                                .size(px(320.))
                                .size_range(px(220.)..px(560.))
                                .child(self.explorer.clone()),
                        )
                        .child(resizable_panel().child(main)),
                ),
            )
            .child(status)
    }
}
