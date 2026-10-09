mod assets;
mod connection_form;
mod console;
mod data_sources;
mod diagram;
mod erd_layout;
mod explorer;
mod memory;
mod results;
mod runtime;
mod session;
mod table_menu;
mod theme;
#[cfg(test)]
mod ui_tests;
mod workspace;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

fn main() {
    gpui_kit::application()
        .with_assets(assets::AppAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            theme::apply(cx);
            console::init(cx);

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
