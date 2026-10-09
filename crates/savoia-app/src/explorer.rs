//! Database Explorer: saved data sources → databases → schemas → object groups.
//!
//! Tree ids are `<kind>:<connection id>[/<path>]`; the kind picks the icon and
//! the connection id routes toolbar and context-menu actions.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::tree::{TreeEntry, TreeItem, TreeState, tree};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_core::{ConnectionConfig, ConnectionId, DatabaseNode, SchemaObjects};
use savoia_tunnel::HostKeyPolicy;

use crate::connection_form::ConnectionForm;
use crate::data_sources::{DataSources, DataSourcesEvent, SourceState};
use crate::theme::{self, BandDisabled as _};

/// Per-source decoration for the tree rows.
#[derive(Clone)]
struct SourceMeta {
    color: Option<u32>,
    status: SourceStatus,
}

#[derive(Clone)]
enum SourceStatus {
    Idle,
    Connecting,
    Connected(SharedString),
    Failed,
}

pub struct Explorer {
    data_sources: Entity<DataSources>,
    tree: Entity<TreeState>,
    /// Last items handed to the tree. Items share expansion state with the
    /// tree, so this is how expansion survives a rebuild.
    items: Vec<TreeItem>,
    meta: Rc<HashMap<ConnectionId, SourceMeta>>,
    /// Sessions whose current database was already expanded automatically.
    auto_opened: HashSet<ConnectionId>,
    _subscriptions: Vec<Subscription>,
}

/// The connection a tree id belongs to.
fn connection_of(id: &str) -> Option<ConnectionId> {
    let rest = id.split_once(':')?.1;
    let uuid = rest.split('/').next()?;
    uuid.parse().ok().map(ConnectionId)
}

fn kind_of(id: &str) -> &str {
    id.split_once(':').map_or(id, |(kind, _)| kind)
}

fn collect_expanded(items: &[TreeItem], out: &mut HashSet<SharedString>) {
    for item in items {
        if item.is_expanded() {
            out.insert(item.id.clone());
        }
        collect_expanded(&item.children, out);
    }
}

fn groups(prefix: &str, objects: &SchemaObjects) -> Vec<TreeItem> {
    let group = |label: &str, kind: &str, names: &[String]| {
        TreeItem::new(format!("group:{prefix}/{label}"), label.to_owned()).children(
            names
                .iter()
                .map(|name| TreeItem::new(format!("{kind}:{prefix}/{name}"), name.clone())),
        )
    };
    let mut out = vec![
        group("tables", "table", &objects.tables),
        group("views", "view", &objects.views),
    ];
    if !objects.functions.is_empty() {
        out.push(group("functions", "function", &objects.functions));
    }
    if !objects.sequences.is_empty() {
        out.push(group("sequences", "sequence", &objects.sequences));
    }
    out
}

fn database_item(config: &ConnectionConfig, db: &DatabaseNode) -> TreeItem {
    let prefix = format!("{}/{}", config.id, db.name);
    let item = TreeItem::new(format!("database:{prefix}"), db.name.clone());
    if config.engine.has_schemas() {
        item.children(db.schemas.iter().map(|schema| {
            let prefix = format!("{prefix}/{}", schema.name);
            TreeItem::new(format!("schema:{prefix}"), schema.name.clone())
                .children(groups(&prefix, &schema.objects))
        }))
    } else {
        item.children(
            db.schemas
                .iter()
                .flat_map(|schema| groups(&prefix, &schema.objects)),
        )
    }
}

/// `public` if present, else the only schema.
fn default_schema(database: &TreeItem) -> Option<SharedString> {
    let schemas: Vec<_> = database
        .children
        .iter()
        .filter(|c| kind_of(&c.id) == "schema")
        .collect();
    schemas
        .iter()
        .find(|s| s.label.as_ref() == "public")
        .or(if schemas.len() == 1 {
            schemas.first()
        } else {
            None
        })
        .map(|s| s.id.clone())
}

impl Explorer {
    pub fn new(data_sources: Entity<DataSources>, cx: &mut Context<Self>) -> Self {
        let tree = cx.new(|cx| TreeState::new(cx));
        let subscriptions = vec![
            cx.subscribe(&data_sources, |this, _, event, cx| {
                if let DataSourcesEvent::Changed = event {
                    this.rebuild(cx);
                }
            }),
            cx.observe(&tree, |_, _, cx| cx.notify()),
        ];
        let mut this = Self {
            data_sources,
            tree,
            items: Vec::new(),
            meta: Rc::default(),
            auto_opened: HashSet::new(),
            _subscriptions: subscriptions,
        };
        this.rebuild(cx);
        this
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let mut expanded = HashSet::new();
        collect_expanded(&self.items, &mut expanded);

        let ds = self.data_sources.read(cx);
        let mut meta = HashMap::new();
        let items: Vec<TreeItem> = ds
            .connections()
            .iter()
            .map(|config| {
                let source = TreeItem::new(format!("source:{}", config.id), config.display_name());
                let status = match ds.state(config.id) {
                    SourceState::Disconnected => SourceStatus::Idle,
                    SourceState::Connecting => SourceStatus::Connecting,
                    SourceState::Failed(_) => SourceStatus::Failed,
                    SourceState::Connected(session) => {
                        SourceStatus::Connected(session.catalog.server.version.clone().into())
                    }
                };
                meta.insert(
                    config.id,
                    SourceMeta {
                        color: config.color.map(|c| c.rgb()),
                        status,
                    },
                );
                match ds.state(config.id) {
                    SourceState::Connected(session) => {
                        // Once per session, open the path to the current database
                        // and its default schema, like DataGrip does.
                        let auto_open = self.auto_opened.insert(config.id);
                        source.children(session.catalog.databases.iter().map(|db| {
                            let item = database_item(config, db);
                            if db.is_current && auto_open {
                                expanded.insert(format!("source:{}", config.id).into());
                                expanded.insert(item.id.clone());
                                if let Some(schema) = default_schema(&item) {
                                    expanded.insert(schema);
                                }
                            }
                            item
                        }))
                    }
                    _ => {
                        self.auto_opened.remove(&config.id);
                        source
                    }
                }
            })
            .collect();

        fn apply(items: Vec<TreeItem>, expanded: &HashSet<SharedString>) -> Vec<TreeItem> {
            items
                .into_iter()
                .map(|mut item| {
                    let children = apply(std::mem::take(&mut item.children), expanded);
                    let open = expanded.contains(&item.id);
                    item.children(children).expanded(open)
                })
                .collect()
        }
        let items = apply(items, &expanded);

        self.meta = Rc::new(meta);
        self.items = items.clone();
        self.tree.update(cx, |tree, cx| tree.set_items(items, cx));
        cx.notify();
    }

    /// The connection of the selected row, if any.
    pub fn selected_connection(&self, cx: &App) -> Option<ConnectionId> {
        self.tree
            .read(cx)
            .selected_item()
            .and_then(|item| connection_of(&item.id))
    }

    pub fn new_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        open_form(self.data_sources.clone(), None, window, cx);
    }

    fn edit(&mut self, id: ConnectionId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(config) = self.data_sources.read(cx).get(id).cloned() else {
            return;
        };
        let secrets = self.data_sources.read(cx).load_secrets(id);
        let data_sources = self.data_sources.clone();
        cx.spawn_in(window, async move |_, cx| {
            let secrets = secrets.await.unwrap_or_default();
            cx.update(|window, cx| open_form(data_sources, Some((config, secrets)), window, cx))
                .ok();
        })
        .detach();
    }

    fn delete(&mut self, id: ConnectionId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.data_sources.read(cx).get(id).map(|c| c.display_name()) else {
            return;
        };
        let data_sources = self.data_sources.clone();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let data_sources = data_sources.clone();
            alert
                .icon(Icon::new(IconName::TriangleAlert).text_color(cx.theme().warning))
                .title(format!("Delete “{name}”?"))
                .description("The saved connection and its stored passwords will be removed.")
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    data_sources.update(cx, |ds, cx| ds.delete(id, cx));
                    true
                })
        });
    }

    fn connect(&mut self, id: ConnectionId, cx: &mut Context<Self>) {
        self.data_sources
            .update(cx, |ds, cx| ds.connect(id, HostKeyPolicy::KnownOnly, cx));
    }

    fn disconnect(&mut self, id: ConnectionId, cx: &mut Context<Self>) {
        self.data_sources.update(cx, |ds, cx| ds.disconnect(id, cx));
    }

    fn refresh(&mut self, id: ConnectionId, cx: &mut Context<Self>) {
        self.data_sources.update(cx, |ds, cx| {
            ds.disconnect(id, cx);
            ds.connect(id, HostKeyPolicy::KnownOnly, cx);
        });
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected_connection(cx);
        let connected = selected.is_some_and(|id| {
            matches!(
                self.data_sources.read(cx).state(id),
                SourceState::Connected(_)
            )
        });
        let band = theme::band_button(cx);
        let button = move |id: &'static str, icon: Icon, tooltip: &'static str| {
            Button::new(id)
                .custom(band)
                .small()
                .icon(icon)
                .tooltip(tooltip)
        };

        // The explorer's Ivrea band, level with the console's.
        h_flex()
            .h(px(34.))
            .px_2()
            .gap_0p5()
            .bg(theme::c(theme::IVREA_GREEN))
            .text_color(theme::c(theme::BAND_INK))
            .child(
                button("ex-add", Icon::new(IconName::Plus), "New data source")
                    .on_click(cx.listener(|this, _, window, cx| this.new_connection(window, cx))),
            )
            .child(
                button(
                    "ex-refresh",
                    Icon::new(IconName::RefreshCw),
                    "Reconnect and refresh",
                )
                .band_disabled(selected.is_none())
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(id) = selected {
                        this.refresh(id, cx);
                    }
                })),
            )
            .child(
                button("ex-disconnect", Icon::new(Lucide::Unplug), "Disconnect")
                    .band_disabled(!connected)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(id) = selected {
                            this.disconnect(id, cx);
                        }
                    })),
            )
            .child(
                button(
                    "ex-edit",
                    Icon::new(IconName::Settings),
                    "Data source properties",
                )
                .band_disabled(selected.is_none())
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(id) = selected {
                        this.edit(id, window, cx);
                    }
                })),
            )
            .child(
                div()
                    .w(px(1.))
                    .h(px(16.))
                    .mx_1()
                    .bg(theme::c(theme::BAND_RULE)),
            )
            .child(
                button(
                    "ex-console",
                    Icon::new(IconName::SquareTerminal),
                    "New query console (M2)",
                )
                .band_disabled(true),
            )
            .child(
                button("ex-ddl", Icon::new(Lucide::FileCode), "Show DDL (M2)").band_disabled(true),
            )
            .child(button("ex-dump", Icon::new(Lucide::Download), "Dump… (M4)").band_disabled(true))
            .child(
                button("ex-import", Icon::new(Lucide::Upload), "Import… (M4)").band_disabled(true),
            )
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_2()
            .p_4()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(Icon::new(Lucide::Database).large())
            .child("No data sources yet")
            .when_some(
                self.data_sources.read(cx).storage_error.clone(),
                |this, err| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(theme.danger)
                            .child(format!("Storage error: {err}")),
                    )
                },
            )
            .child(
                Button::new("empty-add")
                    .small()
                    .primary()
                    .label("New data source")
                    .on_click(cx.listener(|this, _, window, cx| this.new_connection(window, cx))),
            )
    }
}

fn open_form(
    data_sources: Entity<DataSources>,
    existing: Option<(ConnectionConfig, savoia_core::Secrets)>,
    window: &mut Window,
    cx: &mut App,
) {
    let title = if existing.is_some() {
        "Data source properties"
    } else {
        "New data source"
    };
    let form = cx.new(|cx| ConnectionForm::new(data_sources, existing, window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog.title(title).w(px(640.)).child(form.clone())
    });
}

fn node_icon(entry: &TreeEntry) -> Icon {
    match kind_of(&entry.item().id) {
        "source" => Icon::new(Lucide::Server),
        "database" => Icon::new(Lucide::Database),
        "schema" => Icon::new(Lucide::Layers),
        "group" if entry.is_expanded() => Icon::new(IconName::FolderOpen),
        "group" => Icon::new(IconName::Folder),
        "table" => Icon::new(Lucide::Table),
        "view" => Icon::new(IconName::Eye),
        "function" => Icon::new(Lucide::Braces),
        "sequence" => Icon::new(Lucide::ListOrdered),
        _ => Icon::new(IconName::File),
    }
}

impl Render for Explorer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let empty = self.data_sources.read(cx).connections().is_empty();
        let meta = self.meta.clone();
        let this = cx.entity().downgrade();
        let menu_target = cx.entity().downgrade();

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(
                // Same height as the console's tab strip, so both green tool
                // bands start on one line across the panel seam.
                h_flex()
                    .h(px(36.))
                    .px_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .text_sm()
                    .font_semibold()
                    .child("Database Explorer"),
            )
            .child(self.render_toolbar(cx))
            .when(empty, |this| this.child(self.render_empty(cx)))
            .when(!empty, |el| {
                el.child(
                    div().flex_1().min_h_0().py_1().child(
                        tree(&self.tree, move |ix, entry, _selected, _, _| {
                            let id = entry.item().id.clone();
                            let depth = entry.depth() as f32;
                            let muted = theme.muted_foreground;
                            let chevron = if entry.is_folder() {
                                let icon = if entry.is_expanded() {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                };
                                Icon::new(icon)
                                    .xsmall()
                                    .text_color(muted)
                                    .into_any_element()
                            } else {
                                div().w(px(12.)).into_any_element()
                            };
                            let source = (kind_of(&id) == "source")
                                .then(|| connection_of(&id))
                                .flatten();
                            let source_meta = source.and_then(|c| meta.get(&c).cloned());
                            let count =
                                (kind_of(&id) == "group").then(|| entry.item().children.len());
                            let icon = node_icon(entry).small().text_color(
                                source_meta
                                    .as_ref()
                                    .and_then(|m| m.color)
                                    .map_or(muted, |c| rgb(c).into()),
                            );
                            let this = this.clone();

                            ListItem::new(ix)
                                .py_0p5()
                                .pl(px(6. + depth * 14.))
                                .on_click(move |event, _, cx| {
                                    // Double-click a data source to connect.
                                    if event.click_count() == 2
                                        && let Some(id) = source
                                    {
                                        this.update(cx, |this, cx| this.connect(id, cx)).ok();
                                    }
                                })
                                .child(
                                    h_flex()
                                        .gap_1p5()
                                        .text_sm()
                                        .child(chevron)
                                        .child(icon)
                                        .child(entry.item().label.clone())
                                        .when_some(count, |el, n| {
                                            el.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(muted)
                                                    .child(n.to_string()),
                                            )
                                        })
                                        .when_some(source_meta.map(|m| m.status), |el, status| {
                                            match status {
                                                SourceStatus::Idle => el,
                                                SourceStatus::Connecting => el.child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(muted)
                                                        .child("connecting…"),
                                                ),
                                                SourceStatus::Connected(version) => el
                                                    .child(
                                                        div()
                                                            .size(px(6.))
                                                            .rounded_full()
                                                            .bg(theme.success),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(muted)
                                                            .child(version),
                                                    ),
                                                SourceStatus::Failed => el.child(
                                                    div()
                                                        .size(px(6.))
                                                        .rounded_full()
                                                        .bg(theme.danger),
                                                ),
                                            }
                                        }),
                                )
                        })
                        .context_menu(move |_, entry, menu, _, _| {
                            let Some(id) = connection_of(&entry.item().id) else {
                                return menu;
                            };
                            let target = menu_target.clone();
                            let item = |label: &'static str,
                                        f: fn(
                                &mut Explorer,
                                ConnectionId,
                                &mut Window,
                                &mut Context<Explorer>,
                            )| {
                                let target = target.clone();
                                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                                    target.update(cx, |this, cx| f(this, id, window, cx)).ok();
                                })
                            };
                            menu.item(item("Connect", |this, id, _, cx| this.connect(id, cx)))
                                .item(item("Disconnect", |this, id, _, cx| {
                                    this.disconnect(id, cx)
                                }))
                                .item(item("Refresh", |this, id, _, cx| this.refresh(id, cx)))
                                .separator()
                                .item(item("Properties…", |this, id, window, cx| {
                                    this.edit(id, window, cx)
                                }))
                                .item(item("Delete…", |this, id, window, cx| {
                                    this.delete(id, window, cx)
                                }))
                        }),
                    ),
                )
            })
    }
}

#[cfg(test)]
impl Explorer {
    /// Labels of the rows a user would see (expanded paths only), depth-first.
    pub fn visible_labels(&self) -> Vec<String> {
        fn walk(items: &[TreeItem], out: &mut Vec<String>) {
            for item in items {
                out.push(item.label.to_string());
                if item.is_expanded() {
                    walk(&item.children, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.items, &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    // Explicit imports: `gpui_kit::*` (via `super::*`) would shadow `#[test]`.
    use savoia_core::ConnectionId;

    use super::{connection_of, kind_of};

    #[test]
    fn ids_route_to_their_connection() {
        let id = ConnectionId::new();
        assert_eq!(connection_of(&format!("source:{id}")), Some(id));
        assert_eq!(
            connection_of(&format!("table:{id}/shop/public/orders")),
            Some(id)
        );
        assert_eq!(connection_of("group:not-a-uuid/x"), None);
        assert_eq!(kind_of(&format!("schema:{id}/db/public")), "schema");
    }
}
