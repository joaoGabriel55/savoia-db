//! User preferences: loaded once from the store's settings table into a
//! global, written back on every change. Views read them with [`prefs`].

use gpui_kit::{App, Entity, Global, WeakEntity};

use crate::data_sources::DataSources;
use crate::theme::{self, Appearance};

const APPEARANCE: &str = "ui.appearance";
const CHECK_UPDATES: &str = "updates.check_at_startup";
const CRASH_REPORTS: &str = "crash_reports.offer";

pub struct Prefs {
    /// Owns the store the preferences persist to.
    data_sources: WeakEntity<DataSources>,
    pub appearance: Appearance,
    /// Ask GitHub Releases for a newer version at startup. On by default;
    /// release builds only, since a dev build has no update key.
    pub check_updates: bool,
    /// After a crash, offer to open a prefilled GitHub issue. Off until the
    /// user opts in; reports are never sent without them.
    pub crash_reports: bool,
}

impl Global for Prefs {}

/// Loads the preferences and applies the appearance.
pub fn init(data_sources: &Entity<DataSources>, cx: &mut App) {
    let ds = data_sources.read(cx);
    let prefs = Prefs {
        data_sources: data_sources.downgrade(),
        appearance: Appearance::parse(ds.setting(APPEARANCE).as_deref()),
        check_updates: ds.setting(CHECK_UPDATES).as_deref() != Some("false"),
        crash_reports: ds.setting(CRASH_REPORTS).as_deref() == Some("true"),
    };
    let appearance = prefs.appearance;
    cx.set_global(prefs);
    theme::apply(appearance, cx);
}

pub fn prefs(cx: &App) -> &Prefs {
    cx.global::<Prefs>()
}

pub fn set_appearance(appearance: Appearance, cx: &mut App) {
    cx.global_mut::<Prefs>().appearance = appearance;
    save(cx, APPEARANCE, appearance.key());
    theme::apply(appearance, cx);
}

pub fn set_check_updates(on: bool, cx: &mut App) {
    cx.global_mut::<Prefs>().check_updates = on;
    save(cx, CHECK_UPDATES, if on { "true" } else { "false" });
}

pub fn set_crash_reports(on: bool, cx: &mut App) {
    cx.global_mut::<Prefs>().crash_reports = on;
    save(cx, CRASH_REPORTS, if on { "true" } else { "false" });
}

fn save(cx: &mut App, key: &str, value: &str) {
    if let Some(ds) = cx.global::<Prefs>().data_sources.upgrade() {
        ds.update(cx, |ds, _| ds.set_setting(key, Some(value)));
    }
}
