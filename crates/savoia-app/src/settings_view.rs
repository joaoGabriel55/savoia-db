//! The Settings tab: appearance, dump tools, updates and crash reports,
//! the keyboard shortcuts, and an About page with the Ko-fi link.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::Button;
use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::setting::{
    SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::*;

use crate::commands::{self, CheckForUpdates, DOCS_URL, KOFI_URL};
use crate::data_sources::DataSources;
use crate::settings::{self, prefs};
use crate::theme::Appearance;
use crate::transfer::TOOLS_DIR_SETTING;
use crate::updates;

pub struct SettingsView {
    data_sources: Entity<DataSources>,
}

impl SettingsView {
    pub fn new(data_sources: Entity<DataSources>) -> Self {
        Self { data_sources }
    }

    fn general(&self) -> SettingPage {
        let options = Appearance::ALL
            .into_iter()
            .map(|a| (a.key().into(), a.label().into()))
            .collect();
        let updates_note = if updates::enabled() {
            "Asks GitHub Releases for a newer version when Savoia starts. Installing always waits for you."
        } else {
            "This build was made without an update key, so it can't update itself."
        };
        SettingPage::new("General")
            .icon(Icon::new(Lucide::Settings))
            .default_open(true)
            .group(
                SettingGroup::new().title("Appearance").item(
                    SettingItem::new(
                        "Theme",
                        SettingField::dropdown(
                            options,
                            |cx| prefs(cx).appearance.key().into(),
                            |key, cx| settings::set_appearance(Appearance::parse(Some(&key)), cx),
                        ),
                    )
                    .description("Match system follows the OS, also while Savoia is open."),
                ),
            )
            .group(
                SettingGroup::new()
                    .title("Updates")
                    .item(
                        SettingItem::new(
                            "Check for updates at startup",
                            SettingField::switch(
                                |cx| prefs(cx).check_updates,
                                settings::set_check_updates,
                            ),
                        )
                        .description(updates_note)
                        .disabled(!updates::enabled()),
                    )
                    .item(SettingItem::render(|_, _, _| {
                        h_flex().justify_end().child(
                            Button::new("check-updates")
                                .small()
                                .icon(Icon::new(Lucide::RefreshCw))
                                .label("Check now")
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(CheckForUpdates), cx)
                                }),
                        )
                    })),
            )
            .group(
                SettingGroup::new().title("Privacy").item(
                    SettingItem::new(
                        "Offer to report crashes",
                        SettingField::switch(
                            |cx| prefs(cx).crash_reports,
                            settings::set_crash_reports,
                        ),
                    )
                    .description(
                        "After a crash, Savoia offers to open a GitHub issue with the report filled in. \
                         You read it and submit it yourself; nothing is sent automatically.",
                    ),
                ),
            )
    }

    fn tools(&self) -> SettingPage {
        let (read, write) = (self.data_sources.clone(), self.data_sources.clone());
        SettingPage::new("Dump tools")
            .icon(Icon::new(Lucide::Download))
            .group(
                SettingGroup::new().item(
                    SettingItem::new(
                        "Client tools folder",
                        SettingField::input(
                            move |cx| {
                                read.read(cx)
                                    .setting(TOOLS_DIR_SETTING)
                                    .unwrap_or_default()
                                    .into()
                            },
                            move |dir, cx| {
                                let dir = dir.trim().to_owned();
                                write.update(cx, |ds, _| {
                                    ds.set_setting(
                                        TOOLS_DIR_SETTING,
                                        (!dir.is_empty()).then_some(dir.as_str()),
                                    )
                                });
                            },
                        ),
                    )
                    .description(
                        "Where pg_dump, pg_restore, psql, mysqldump and mysql live. \
                         Empty searches PATH and the usual install folders.",
                    ),
                ),
            )
    }

    fn keyboard(&self) -> SettingPage {
        SettingPage::new("Keyboard")
            .icon(Icon::new(Lucide::Keyboard))
            .group(SettingGroup::new().item(SettingItem::render(|_, _, cx| {
                let muted = cx.theme().muted_foreground;
                let row = |label: SharedString, keys: &str| {
                    h_flex()
                        .w_full()
                        .py_1()
                        .justify_between()
                        .text_sm()
                        .child(label)
                        .children(Keystroke::parse(keys).ok().map(Kbd::new))
                };
                v_flex()
                    .w_full()
                    .child(row("Command palette".into(), commands::palette_keys()))
                    .children(
                        commands::commands()
                            .into_iter()
                            .filter_map(|c| c.keys.map(|k| row(c.label.into(), k))),
                    )
                    .children(
                        commands::VIEW_SHORTCUTS
                            .iter()
                            .map(|(label, keys)| row((*label).into(), keys)),
                    )
                    .child(
                        div()
                            .pt_2()
                            .text_xs()
                            .text_color(muted)
                            .child("Every other command is in the palette."),
                    )
            })))
    }

    fn about(&self) -> SettingPage {
        SettingPage::new("About")
            .icon(Icon::new(Lucide::Info))
            .group(SettingGroup::new().item(SettingItem::render(|_, _, cx| {
                let muted = cx.theme().muted_foreground;
                v_flex()
                    .w_full()
                    .gap_2()
                    .text_sm()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("Savoia Studio {}", env!("CARGO_PKG_VERSION"))),
                    )
                    .child(div().text_color(muted).child(
                        "A native database client for PostgreSQL and MySQL. MIT or Apache-2.0.",
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .pt_1()
                            .child(
                                Button::new("about-docs")
                                    .small()
                                    .icon(Icon::new(Lucide::BookOpen))
                                    .label("Documentation")
                                    .on_click(|_, _, cx| cx.open_url(DOCS_URL)),
                            )
                            .child(
                                Button::new("about-kofi")
                                    .small()
                                    .icon(Icon::new(Lucide::Coffee))
                                    .label("Support me on Ko-fi")
                                    .on_click(|_, _, cx| cx.open_url(KOFI_URL)),
                            ),
                    )
            })))
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Settings::new("settings")
            .with_group_variant(GroupBoxVariant::Outline)
            .sidebar_width(px(200.))
            .pages([self.general(), self.tools(), self.keyboard(), self.about()])
    }
}
