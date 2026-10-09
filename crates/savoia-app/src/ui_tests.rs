//! Headless UI tests: real views in a test window, driven by clicks.
//! The live test needs `SAVOIA_PG_URL` (docker compose); it polls in real
//! time because database I/O runs on the Tokio runtime, not the test clock.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, WindowOptions};
use savoia_store::{ConnectionStore, MemorySecrets};

use crate::connection_form::ConnectionForm;
use crate::data_sources::{DataSources, SourceState};
use crate::explorer::Explorer;

fn mount(
    cx: &mut TestAppContext,
) -> (Entity<DataSources>, Entity<ConnectionForm>, AnyWindowHandle) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::apply(cx);
    });
    let ds = cx.new(|_| {
        DataSources::with_stores(
            ConnectionStore::open_in_memory(),
            Arc::new(MemorySecrets::default()),
        )
    });
    let (window, form) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ConnectionForm::new(ds.clone(), None, window, cx))
            })
        })
        .expect("window");
    (ds, form, window)
}

fn import(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    form: &Entity<ConnectionForm>,
    url: &str,
) {
    let url = url.to_owned();
    cx.update_window(window, |_, window, cx| {
        form.read(cx)
            .url_input()
            .update(cx, |input, cx| input.set_value(url, window, cx));
        window.render_frame(cx);
        window.click("import", cx);
    })
    .unwrap();
}

fn click(cx: &mut TestAppContext, window: AnyWindowHandle, id: &'static str) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
}

#[gpui_kit::test]
async fn saving_without_a_user_shows_an_error_and_saves_nothing(cx: &mut TestAppContext) {
    let (ds, form, window) = mount(cx);
    import(cx, window, &form, "postgres://localhost/db");
    click(cx, window, "save");

    let status = form.read_with(cx, |f, _| f.status_text());
    assert_eq!(status.as_deref(), Some("User is required"));
    assert!(ds.read_with(cx, |ds, _| ds.connections().is_empty()));
}

#[gpui_kit::test]
async fn bad_url_is_reported(cx: &mut TestAppContext) {
    let (_, form, window) = mount(cx);
    import(cx, window, &form, "redis://localhost");
    let status = form
        .read_with(cx, |f, _| f.status_text())
        .unwrap_or_default();
    assert!(status.contains("Unsupported scheme"), "{status}");
}

/// Fills the form from `url`, clicks Save & Connect, waits for the session,
/// and returns the explorer's visible labels.
async fn save_connect_and_list(cx: &mut TestAppContext, url: &str) -> Vec<String> {
    cx.executor().allow_parking();
    let (ds, form, window) = mount(cx);
    let explorer = cx.new(|cx| Explorer::new(ds.clone(), cx));

    import(cx, window, &form, url);
    click(cx, window, "save-connect");

    let id = ds
        .read_with(cx, |ds, _| ds.connections().first().map(|c| c.id))
        .expect("saved");
    for _ in 0..300 {
        cx.run_until_parked();
        let state = ds.read_with(cx, |ds, _| match ds.state(id) {
            SourceState::Connected(_) => Some(Ok(())),
            SourceState::Failed(err) => Some(Err(err.clone())),
            _ => None,
        });
        match state {
            Some(Ok(())) => return explorer.read_with(cx, |e, _| e.visible_labels()),
            Some(Err(err)) => panic!("connect failed: {err}"),
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    panic!("timed out connecting");
}

#[gpui_kit::test]
async fn postgres_save_and_connect_opens_the_default_schema(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let labels = save_connect_and_list(cx, &format!("{url}?sslmode=require")).await;
    // source → databases (current expanded) → schemas (public expanded) → groups
    assert_eq!(
        labels.first().map(String::as_str),
        Some("127.0.0.1:54317"),
        "{labels:?}"
    );
    let public = labels
        .iter()
        .position(|l| l == "public")
        .expect("public schema");
    assert_eq!(
        labels.get(public + 1).map(String::as_str),
        Some("tables"),
        "{labels:?}"
    );
}

#[gpui_kit::test]
async fn mysql_save_and_connect_shows_groups_under_the_database(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_MYSQL_URL") else {
        return;
    };
    let labels = save_connect_and_list(cx, &url).await;
    // No schema level in MySQL: source → databases (current expanded) → groups
    let db = labels
        .iter()
        .position(|l| l == "savoia")
        .expect("current database");
    assert_eq!(
        labels.get(db + 1).map(String::as_str),
        Some("tables"),
        "{labels:?}"
    );
}
