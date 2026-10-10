mod assets;
mod commands;
mod completion;
mod connection_form;
mod console;
mod crash;
mod data_grid;
mod data_pickers;
mod data_sources;
mod data_view;
mod diagram;
mod erd_layout;
mod explorer;
mod history;
mod memory;
mod relations;
mod results;
mod runtime;
mod session;
mod settings;
mod settings_view;
mod structure;
mod table_menu;
mod theme;
mod transfer;
#[cfg(test)]
mod ui_tests;
mod updates;
mod workspace;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

fn main() {
    crash::install();
    gpui_kit::application()
        .with_assets(assets::AppAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            // The saved appearance replaces this once the store is open.
            theme::apply(theme::Appearance::System, cx);
            console::init(cx);
            commands::init(cx);

            let bounds = Bounds::centered(None, size(px(1360.), px(860.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(900.), px(560.))),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| workspace::Workspace::new(window, cx))
            })
            .expect("failed to open the main window");
            cx.activate(true);
        });
}
