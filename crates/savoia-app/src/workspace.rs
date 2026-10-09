//! Main window: title bar, explorer | console and diagram tabs, status bar. Also turns
//! data-source events into notifications and the host-key trust and password prompts.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, TitleBar, WindowExt as _,
    h_flex, v_flex,
};
use gpui_kit::*;
use savoia_core::ConnectionId;
use savoia_tunnel::HostKeyPolicy;

use crate::console::QueryConsole;
use crate::data_sources::{DataSources, DataSourcesEvent, SourceState};
use crate::diagram::ErDiagram;
use crate::explorer::{Explorer, ExplorerEvent, NodeRef};
use crate::memory::MemoryMeter;
use crate::theme;

pub struct Workspace {
    data_sources: Entity<DataSources>,
    explorer: Entity<Explorer>,
    console: Entity<QueryConsole>,
    /// Tabs after the console's.
    diagrams: Vec<Entity<ErDiagram>>,
    /// 0 is the console, then `diagrams`.
    active_tab: usize,
    memory: Entity<MemoryMeter>,
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
            cx.subscribe_in(&explorer, window, Self::on_explorer_event),
        ];
        let console =
            cx.new(|cx| QueryConsole::new(data_sources.clone(), explorer.clone(), window, cx));
        Self {
            console,
            diagrams: Vec::new(),
            active_tab: 0,
            memory: cx.new(MemoryMeter::new),
            data_sources,
            explorer,
            _subscriptions: subscriptions,
        }
    }

    /// Opens the diagram of `node`'s schema, or brings its tab forward.
    fn show_diagram(&mut self, node: NodeRef, cx: &mut Context<Self>) {
        let open = self.diagrams.iter().position(|d| d.read(cx).shows(&node));
        let index = match open {
            Some(i) => {
                self.diagrams[i].update(cx, |d, cx| d.focus(node.table, cx));
                i
            }
            None => {
                let data_sources = self.data_sources.clone();
                self.diagrams
                    .push(cx.new(|cx| ErDiagram::new(data_sources, node, cx)));
                self.diagrams.len() - 1
            }
        };
        self.active_tab = index + 1;
        cx.notify();
    }

    fn close_diagram(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.diagrams.len() {
            return;
        }
        self.diagrams.remove(index);
        if self.active_tab > index {
            self.active_tab -= 1;
        }
        cx.notify();
    }

    fn on_explorer_event(
        &mut self,
        _: &Entity<Explorer>,
        event: &ExplorerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ExplorerEvent::ShowDiagram(node) => self.show_diagram(node.clone(), cx),
            ExplorerEvent::Sql { sql, run, refresh } => {
                // The SQL lands in the console, so bring it forward.
                self.active_tab = 0;
                self.console
                    .update(cx, |console, cx| console.insert_sql(sql, *run, window, cx));
                // Queued behind the statement: refreshes wait for the running query.
                if let Some(id) = refresh {
                    self.data_sources.update(cx, |ds, cx| ds.refresh(*id, cx));
                }
                cx.notify();
            }
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
            DataSourcesEvent::NeedsPassword { id, error } => {
                self.ask_password(*id, error.clone(), data_sources.clone(), window, cx);
            }
        }
    }

    /// Asks for the database password of `id`, then connects with it.
    fn ask_password(
        &mut self,
        id: ConnectionId,
        error: Option<String>,
        data_sources: Entity<DataSources>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(config) = data_sources.read(cx).get(id).cloned() else {
            return;
        };
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Password")
        });
        password.update(cx, |input, cx| input.focus(window, cx));
        let target = format!("{}@{}:{}", config.user, config.host, config.port);
        let note = if config.save_password {
            "It is saved once the connection works."
        } else {
            "It is kept until Savoia Studio quits."
        };
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let theme = cx.theme();
            let (data_sources, input) = (data_sources.clone(), password.clone());
            alert
                .icon(Icon::new(Lucide::KeyRound).text_color(theme.muted_foreground))
                .title(format!("Password for {}", config.display_name()))
                .description(
                    v_flex()
                        .gap_3()
                        .children(
                            error
                                .clone()
                                .map(|e| div().text_color(theme.danger).child(e)),
                        )
                        .child(format!("Connecting as {target}. {note}"))
                        .child(Input::new(&password).mask_toggle()),
                )
                .show_cancel(true)
                .ok_text("Connect")
                .on_ok(move |_, _, cx| {
                    let value = input.read(cx).value().to_string();
                    data_sources.update(cx, |ds, cx| ds.connect_with_password(id, value, cx));
                    true
                })
        });
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
                    session.catalog().server.version,
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
                .child(div().font_semibold().child("Savoia Studio"))
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

        let diagram_tabs: Vec<Tab> = self
            .diagrams
            .iter()
            .enumerate()
            .map(|(i, diagram)| {
                Tab::new()
                    .label(diagram.read(cx).title())
                    .prefix(Icon::new(Lucide::Workflow).small().ml_2().text_color(muted))
                    .suffix(
                        Button::new(("close-diagram", i))
                            .ghost()
                            .xsmall()
                            .mr_1()
                            .icon(Icon::new(IconName::Close))
                            .tooltip("Close")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.close_diagram(i, cx);
                            })),
                    )
            })
            .collect();
        let workspace = cx.entity().downgrade();
        let content = match self
            .active_tab
            .checked_sub(1)
            .and_then(|i| self.diagrams.get(i))
        {
            Some(diagram) => diagram.clone().into_any_element(),
            None => self.console.clone().into_any_element(),
        };

        let main = v_flex()
            .size_full()
            .child(
                TabBar::new("consoles")
                    .underline()
                    .child(console_tab)
                    .children(diagram_tabs)
                    .selected_index(self.active_tab.min(self.diagrams.len()))
                    .on_click(move |ix, _, cx| {
                        let ix = *ix;
                        workspace
                            .update(cx, |this, cx| {
                                this.active_tab = ix;
                                cx.notify();
                            })
                            .ok();
                    }),
            )
            .child(div().flex_1().min_h_0().child(content));

        let status = StatusBar::new()
            .left(
                h_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(muted)
                    .child(Icon::new(status_icon).xsmall())
                    .child(status_text),
            )
            .right(
                h_flex()
                    .gap_3()
                    .child(self.memory.clone())
                    .child(div().text_xs().text_color(muted).child("SQL · UTF-8")),
            );

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
