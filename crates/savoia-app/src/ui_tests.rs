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
use crate::data_sources::{DataSources, RefreshState, SourceState};
use crate::diagram::ErDiagram;
use crate::explorer::{Explorer, NodeRef};
use crate::session::Session;

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

/// The visible tree rows once nothing shown is loading any more.
fn settled_labels(cx: &mut TestAppContext, explorer: &Entity<Explorer>) -> Vec<String> {
    let loading = |labels: &[String]| labels.iter().any(|l| l.ends_with('…'));
    wait_until(cx, "the explorer to load", |cx| {
        !loading(&explorer.read_with(cx, |e, _| e.visible_labels()))
    });
    explorer.read_with(cx, |e, _| e.visible_labels())
}

async fn save_connect_and_list(cx: &mut TestAppContext, url: &str) -> Vec<String> {
    let (_, explorer) = save_and_connect(cx, url);
    settled_labels(cx, &explorer)
}

/// Connects to `url` and opens a console on it, with the source selected.
fn console_on(
    cx: &mut TestAppContext,
    url: &str,
) -> (Entity<DataSources>, Entity<QueryConsole>, AnyWindowHandle) {
    let (ds, explorer) = save_and_connect(cx, url);
    explorer.update(cx, |e, cx| e.select_first_source(cx));
    cx.update(crate::console::init);
    let (window, console) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| QueryConsole::new(ds.clone(), explorer, window, cx))
            })
        })
        .expect("window");
    (ds, console, window)
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

/// Row count and first row of result `ix` (negative: from the end).
fn grid_at(
    cx: &mut TestAppContext,
    console: &Entity<QueryConsole>,
    ix: isize,
) -> (usize, Vec<String>) {
    console.read_with(cx, |c, cx| {
        let results = c.results();
        let Some(table) = results.get(ix.rem_euclid(results.len().max(1) as isize) as usize) else {
            return (0, Vec::new());
        };
        let table = table.read(cx);
        let rows = table.delegate();
        let first = (0..rows.columns_count(cx))
            .filter(|_| rows.rows_count(cx) > 0)
            .map(|col| rows.cell_text(0, col, cx))
            .collect();
        (rows.len(), first)
    })
}

/// The last result.
fn grid(cx: &mut TestAppContext, console: &Entity<QueryConsole>) -> (usize, Vec<String>) {
    grid_at(cx, console, -1)
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
    let (_, console, window) = console_on(cx, url);
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

/// A result that ends exactly where the grid stops asking still finishes:
/// the reader only waits when another page actually arrives.
#[gpui_kit::test]
async fn postgres_console_finishes_a_result_that_fills_the_grid(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (_, console, window) = console_on(cx, &url);
    run(
        cx,
        window,
        &console,
        "SELECT g FROM generate_series(1, 1000) g",
    );
    idle(cx, &console);
    assert_eq!(grid(cx, &console).0, 1000);
}

fn paused(cx: &mut TestAppContext, console: &Entity<QueryConsole>) -> bool {
    console.read_with(cx, |c, cx| {
        c.results()
            .last()
            .is_some_and(|t| t.read(cx).delegate().is_paused())
    })
}

/// Skip rest discards the paused result's remaining rows so the script goes on.
#[gpui_kit::test]
async fn postgres_console_skips_the_rest_of_a_paused_result(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (_, console, window) = console_on(cx, &url);
    run(
        cx,
        window,
        &console,
        "SELECT g FROM generate_series(1, 100000) g; SELECT 'after' AS a",
    );
    wait_until(cx, "the pause", |cx| paused(cx, &console));
    click(cx, window, "skip-rest");
    idle(cx, &console);

    // Each result keeps its own tab: the kept rows, then the next statement.
    assert_eq!(console.read_with(cx, |c, _| c.results().len()), 2);
    assert_eq!(grid_at(cx, &console, 0).0, 1000);
    assert_eq!(grid_at(cx, &console, 1).1[1], "after");
    let (tabs, run) = console.read_with(cx, |c, _| c.summaries());
    assert!(
        tabs[0].starts_with("1,000 rows (99,000 rows skipped) · "),
        "{tabs:?}"
    );
    assert!(tabs[1].starts_with("1 row · "), "{tabs:?}");
    assert!(
        run.as_deref().unwrap_or("").starts_with("2 results · "),
        "{run:?}"
    );
    let lines = output(cx, &console);
    assert!(
        lines[0].starts_with("1,000 rows (99,000 rows skipped) · "),
        "{lines:?}"
    );
    assert!(lines[1].starts_with("1 row · "), "{lines:?}");
}

/// Load all streams the rest of a paused result into the grid.
#[gpui_kit::test]
async fn mysql_console_loads_all_of_a_paused_result(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_MYSQL_URL") else {
        return;
    };
    let (_, console, window) = console_on(cx, &url);
    run(
        cx,
        window,
        &console,
        // Recursion stops at depth 1,000 by default; cross join to get more rows.
        "WITH RECURSIVE k (n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM k WHERE n < 999) \
         SELECT a.n * 1000 + b.n AS n FROM k a CROSS JOIN k b LIMIT 20000",
    );
    wait_until(cx, "the pause", |cx| paused(cx, &console));
    click(cx, window, "load-all");
    idle(cx, &console);
    assert_eq!(grid(cx, &console).0, 20000);
}

/// A big result loads only what the grid asked for; Cancel stops it and
/// frees the connection for the next run.
#[gpui_kit::test]
async fn postgres_console_pauses_big_results_and_cancels(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (_, console, window) = console_on(cx, &url);
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

fn session_of(cx: &mut TestAppContext, ds: &Entity<DataSources>) -> Option<Arc<Session>> {
    ds.read_with(cx, |ds, _| match ds.state(ds.connections()[0].id) {
        SourceState::Connected(session) => Some(session.clone()),
        _ => None,
    })
}

fn refresh_state(cx: &mut TestAppContext, ds: &Entity<DataSources>) -> Option<RefreshState> {
    ds.read_with(cx, |ds, _| ds.refresh_state(ds.connections()[0].id))
}

/// Refresh waits for the running query and then reloads the catalog on the
/// same connection; disconnecting abandons the wait.
#[gpui_kit::test]
async fn postgres_refresh_waits_for_the_running_query(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (ds, console, window) = console_on(cx, &url);
    let id = ds.read_with(cx, |ds, _| ds.connections()[0].id);
    let before = session_of(cx, &ds).expect("connected");
    let big = "SELECT g FROM generate_series(1, 100000) g";

    run(cx, window, &console, big);
    wait_until(cx, "the pause", |cx| paused(cx, &console));
    ds.update(cx, |ds, cx| ds.refresh(id, cx));
    std::thread::sleep(Duration::from_millis(300));
    cx.run_until_parked();
    assert_eq!(refresh_state(cx, &ds), Some(RefreshState::Waiting));

    click(cx, window, "cancel");
    wait_until(cx, "the refresh", |cx| refresh_state(cx, &ds).is_none());
    let after = session_of(cx, &ds).expect("still connected");
    assert!(Arc::ptr_eq(&before, &after), "refresh reconnected");

    run(cx, window, &console, big);
    wait_until(cx, "the pause", |cx| paused(cx, &console));
    ds.update(cx, |ds, cx| {
        ds.refresh(id, cx);
        ds.disconnect(id, cx);
    });
    assert_eq!(refresh_state(cx, &ds), None);
    assert!(session_of(cx, &ds).is_none());
}

/// Runs `sql` to the end on the source's own session.
fn exec(session: Arc<Session>, sql: &str) {
    let sql = sql.to_owned();
    let (tx, rx) = std::sync::mpsc::channel();
    drop(crate::runtime::spawn(async move {
        let result = async {
            let mut query = session.execute(sql).await?;
            while let Some(event) = query.next().await {
                event?;
            }
            Ok::<_, savoia_core::AppError>(())
        }
        .await;
        drop(tx.send(result));
    }));
    rx.recv_timeout(Duration::from_secs(15))
        .expect("seed timed out")
        .expect("seed failed");
}

const UI_SCHEMA: &str = "DROP SCHEMA IF EXISTS it_ui CASCADE;
    CREATE SCHEMA it_ui;
    CREATE TABLE it_ui.customers (id serial PRIMARY KEY, name text NOT NULL);
    CREATE TABLE it_ui.orders (
      id serial PRIMARY KEY,
      customer_id int NOT NULL REFERENCES it_ui.customers (id),
      placed_at timestamptz);
    CREATE TABLE it_ui.notes (body text);";

/// Expanding a schema loads its object names; expanding a table loads its
/// columns and keys. The diagram of the schema joins the tables by their
/// foreign keys.
#[gpui_kit::test]
async fn postgres_explorer_loads_on_expand_and_draws_the_diagram(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (ds, explorer) = save_and_connect(cx, &url);
    let id = ds.read_with(cx, |ds, _| ds.connections()[0].id);
    exec(session_of(cx, &ds).expect("connected"), UI_SCHEMA);
    ds.update(cx, |ds, cx| ds.refresh(id, cx));
    wait_until(cx, "the refresh", |cx| refresh_state(cx, &ds).is_none());

    let labels = settled_labels(cx, &explorer);
    let schema = labels.iter().position(|l| l == "it_ui").expect("it_ui");
    assert_ne!(
        labels.get(schema + 1).map(String::as_str),
        Some("tables"),
        "not loaded before expanding"
    );

    explorer.update(cx, |e, cx| e.expand(&["it_ui"], cx));
    let labels = settled_labels(cx, &explorer);
    assert_eq!(
        labels.get(schema + 1).map(String::as_str),
        Some("tables"),
        "{labels:?}"
    );

    explorer.update(cx, |e, cx| e.expand(&["it_ui", "tables"], cx));
    explorer.update(cx, |e, cx| e.expand(&["it_ui", "tables", "orders"], cx));
    let labels = settled_labels(cx, &explorer);
    let orders = labels.iter().position(|l| l == "orders").expect("orders");
    assert_eq!(
        labels[orders + 1..orders + 6],
        ["id", "customer_id", "placed_at", "foreign keys", "indexes"],
        "{labels:?}"
    );

    let node = NodeRef {
        connection: id,
        database: "savoia".into(),
        schema: "it_ui".into(),
        table: Some("orders".into()),
    };
    let (window, diagram) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(|cx| ErDiagram::new(ds.clone(), node, cx))
            })
        })
        .expect("window");
    wait_until(cx, "the diagram", |cx| {
        diagram.read_with(cx, |d, _| d.loaded().is_some())
    });
    let (tables, lines) = diagram.read_with(cx, |d, _| d.loaded().unwrap());
    assert_eq!(tables, ["customers", "notes", "orders"]);
    assert_eq!(lines, 1);
    assert_eq!(
        diagram
            .read_with(cx, |d, _| d.focused().map(str::to_owned))
            .as_deref(),
        Some("orders")
    );
    // Paints boxes and lines without panicking, and survives a reset.
    click(cx, window, "erd-reset");
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
}
