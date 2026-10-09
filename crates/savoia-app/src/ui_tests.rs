//! Headless UI tests: real views in a test window, driven by clicks.
//! The live test needs `SAVOIA_PG_URL` (docker compose); it polls in real
//! time because database I/O runs on the Tokio runtime, not the test clock.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::table::TableDelegate as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, WindowOptions};
use savoia_store::{ConnectionStore, MemorySecrets};

use crate::connection_form::ConnectionForm;
use crate::console::QueryConsole;
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

/// Polls in real time (I/O runs on the Tokio runtime) until `done` holds.
fn wait_until(
    cx: &mut TestAppContext,
    what: &str,
    mut done: impl FnMut(&mut TestAppContext) -> bool,
) {
    for _ in 0..300 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

/// Fills the form from `url`, clicks Save & Connect and waits for the
/// session. Returns the data sources and an explorer over them.
fn save_and_connect(cx: &mut TestAppContext, url: &str) -> (Entity<DataSources>, Entity<Explorer>) {
    cx.executor().allow_parking();
    let (ds, form, window) = mount(cx);
    let explorer = cx.new(|cx| Explorer::new(ds.clone(), cx));

    import(cx, window, &form, url);
    click(cx, window, "save-connect");

    let id = ds
        .read_with(cx, |ds, _| ds.connections().first().map(|c| c.id))
        .expect("saved");
    wait_until(cx, "the session", |cx| {
        ds.read_with(cx, |ds, _| match ds.state(id) {
            SourceState::Connected(_) => true,
            SourceState::Failed(err) => panic!("connect failed: {err}"),
            _ => false,
        })
    });
    (ds, explorer)
}

async fn save_connect_and_list(cx: &mut TestAppContext, url: &str) -> Vec<String> {
    let (_, explorer) = save_and_connect(cx, url);
    explorer.read_with(cx, |e, _| e.visible_labels())
}

/// Connects to `url` and opens a console on it, with the source selected.
fn console_on(cx: &mut TestAppContext, url: &str) -> (Entity<QueryConsole>, AnyWindowHandle) {
    let (ds, explorer) = save_and_connect(cx, url);
    explorer.update(cx, |e, cx| e.select_first_source(cx));
    cx.update(crate::console::init);
    let (window, console) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| QueryConsole::new(ds, explorer, window, cx))
            })
        })
        .expect("window");
    (console, window)
}

/// Types `sql` into the console and clicks Run.
fn run(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    console: &Entity<QueryConsole>,
    sql: &str,
) {
    let sql = sql.to_owned();
    cx.update_window(window, |_, window, cx| {
        console
            .read(cx)
            .editor()
            .clone()
            .update(cx, |editor, cx| editor.set_value(sql, window, cx));
    })
    .unwrap();
    click(cx, window, "run");
}

fn grid(cx: &mut TestAppContext, console: &Entity<QueryConsole>) -> (usize, Vec<String>) {
    console.read_with(cx, |c, cx| {
        let table = c.results().read(cx);
        let rows = table.delegate();
        let first = (0..rows.columns_count(cx))
            .filter(|_| rows.rows_count(cx) > 0)
            .map(|col| rows.cell_text(0, col, cx))
            .collect();
        (rows.len(), first)
    })
}

fn idle(cx: &mut TestAppContext, console: &Entity<QueryConsole>) {
    wait_until(cx, "the query to end", |cx| {
        console.read_with(cx, |c, _| !c.is_running())
    });
}

fn output(cx: &mut TestAppContext, console: &Entity<QueryConsole>) -> Vec<String> {
    console.read_with(cx, |c, _| c.output_lines())
}

fn runs_a_script_and_reports_each_statement(url: &str, cx: &mut TestAppContext) {
    let (console, window) = console_on(cx, url);
    run(
        cx,
        window,
        &console,
        "CREATE TEMPORARY TABLE ui_t (n INT);\n\
         INSERT INTO ui_t VALUES (1), (2);\n\
         SELECT 42 AS n, NULL AS x;",
    );
    idle(cx, &console);
    assert_eq!(
        grid(cx, &console),
        (1, vec!["1".into(), "42".into(), "NULL".into()])
    );
    let lines = output(cx, &console);
    assert!(lines[1].starts_with("2 rows affected · "), "{lines:?}");
    assert!(lines[2].starts_with("1 row · "), "{lines:?}");

    run(cx, window, &console, "SELECT * FROM no_such_table");
    idle(cx, &console);
    let lines = output(cx, &console);
    assert!(lines.last().unwrap().contains("no_such_table"), "{lines:?}");
}

#[gpui_kit::test]
async fn postgres_console_runs_a_script(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    runs_a_script_and_reports_each_statement(&url, cx);
}

#[gpui_kit::test]
async fn mysql_console_runs_a_script(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_MYSQL_URL") else {
        return;
    };
    runs_a_script_and_reports_each_statement(&url, cx);
}

/// A big result loads only what the grid asked for; Cancel stops it and
/// frees the connection for the next run.
#[gpui_kit::test]
async fn postgres_console_pauses_big_results_and_cancels(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (console, window) = console_on(cx, &url);
    run(
        cx,
        window,
        &console,
        "SELECT g FROM generate_series(1, 100000) g",
    );
    wait_until(cx, "the first rows", |cx| grid(cx, &console).0 >= 1000);
    std::thread::sleep(Duration::from_millis(300));
    cx.run_until_parked();
    let (loaded, _) = grid(cx, &console);
    assert!(loaded < 100_000, "loaded everything: {loaded}");
    assert!(console.read_with(cx, |c, _| c.is_running()));

    click(cx, window, "cancel");
    assert!(!console.read_with(cx, |c, _| c.is_running()));
    assert!(output(cx, &console)[0].starts_with("Cancelled after "));

    run(cx, window, &console, "SELECT 'next'");
    idle(cx, &console);
    assert_eq!(grid(cx, &console).1[1], "next");
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
