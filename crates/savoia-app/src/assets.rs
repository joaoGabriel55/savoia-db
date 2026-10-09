//! Asset source: the default component icons plus the extra Lucide icons the
//! app uses. Add an icon here before referencing it, or it renders blank.

use std::borrow::Cow;

use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::{AssetSource, Result, SharedString};

icon_assets!(
    ExtraIcons,
    [
        ArrowUpDown,
        Braces,
        CircleStop,
        Columns3,
        Database,
        Download,
        FileCode,
        Funnel,
        Key,
        Layers,
        ListOrdered,
        Plug,
        Server,
        Table,
        Timer,
        Unplug,
        Upload,
    ]
);

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
