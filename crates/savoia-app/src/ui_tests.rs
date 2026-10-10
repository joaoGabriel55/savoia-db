//! Headless UI tests: real views in a test window, driven by clicks.
//! The live test needs `SAVOIA_PG_URL` (docker compose); it polls in real
//! time because database I/O runs on the Tokio runtime, not the test clock.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::table::TableDelegate as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Entity, TestAppContext, Window, WindowOptions,
};
use savoia_store::{ConnectionStore, MemorySecrets};

use crate::connection_form::ConnectionForm;
use crate::console::QueryConsole;
use crate::data_sources::{DataSources, DataSourcesEvent, RefreshState, SourceState};
use crate::diagram::ErDiagram;
use crate::explorer::{Explorer, NodeRef};
use crate::history::HistoryPanel;
use crate::session::Session;
use crate::structure::StructureView;
use crate::workspace::{NewConsole, Workspace};

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
                cx.new(|cx| QueryConsole::new(ds.clone(), explorer, None, window, cx))
            })
        })
        .expect("window");
    (ds, console, window)
}

/// Types `sql` into the console and clicks Run script.
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
    click(cx, window, "run-script");
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
    let zoom = |cx: &mut TestAppContext| diagram.read_with(cx, |d, _| d.zoom());
    let fitted = zoom(cx);
    assert!(fitted <= 1., "fit never magnifies: {fitted}");

    click(cx, window, "erd-zoom-reset");
    assert!((zoom(cx) - 1.).abs() < 1e-4);
    click(cx, window, "erd-zoom-in");
    assert!((zoom(cx) - 1.25).abs() < 1e-4, "{}", zoom(cx));
    click(cx, window, "erd-zoom-out");
    click(cx, window, "erd-zoom-out");
    assert!((zoom(cx) - 0.8).abs() < 1e-4, "{}", zoom(cx));
    click(cx, window, "erd-fit");
    assert!((zoom(cx) - fitted).abs() < 1e-4);

    click(cx, window, "erd-collapse-all");
    assert_eq!(diagram.read_with(cx, |d, _| d.collapsed_count()), 3);
    click(cx, window, "erd-collapse-all");
    assert_eq!(diagram.read_with(cx, |d, _| d.collapsed_count()), 0);

    // Right after a frame, a burst of moves waits for the next 60 fps slot
    // instead of redrawing at once.
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert!(diagram.update(cx, |d, cx| d.drag_moves(50, cx)));
    std::thread::sleep(Duration::from_millis(30));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

/// Memory benchmark over the Serie A sample (`samples/serie_a.postgres.sql`
/// loaded). Prints resident memory after each step:
/// `SAVOIA_PG_URL=… cargo test -p savoia-app --release memory_benchmark -- --ignored --nocapture`
/// The test platform draws nothing, so GPU-side memory of the real app isn't included.
#[gpui_kit::test]
#[ignore = "benchmark; run by hand"]
async fn memory_benchmark(cx: &mut TestAppContext) {
    use crate::memory::{format_bytes, resident_bytes};
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let mut rows: Vec<(&str, u64, Duration)> = Vec::new();
    let mut step = |name, started: std::time::Instant| {
        rows.push((name, resident_bytes().unwrap_or(0), started.elapsed()));
    };

    let t = std::time::Instant::now();
    step("start", t);
    let (ds, console, window) = console_on(cx, &url);
    step("connected, explorer open", t);

    let t = std::time::Instant::now();
    run(cx, window, &console, "SELECT * FROM serie_a.match_events");
    wait_until(cx, "the pause", |cx| paused(cx, &console));
    click(cx, window, "load-all");
    idle(cx, &console);
    assert_eq!(
        grid(cx, &console).0,
        150_000,
        "load samples/serie_a.postgres.sql first"
    );
    step("150,000 rows x 6 columns in the grid", t);

    let t = std::time::Instant::now();
    run(cx, window, &console, "SELECT 1");
    idle(cx, &console);
    step("result replaced by SELECT 1", t);

    let t = std::time::Instant::now();
    let node = NodeRef {
        connection: ds.read_with(cx, |ds, _| ds.connections()[0].id),
        database: "savoia".into(),
        schema: "serie_a".into(),
        table: None,
    };
    let (erd_window, diagram) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(|cx| ErDiagram::new(ds.clone(), node, cx))
            })
        })
        .expect("window");
    wait_until(cx, "the diagram", |cx| {
        diagram.read_with(cx, |d, _| d.loaded().is_some())
    });
    for _ in 0..3 {
        cx.update_window(erd_window, |_, window, cx| window.render_frame(cx))
            .unwrap();
    }
    step("serie_a ER diagram (14 tables) drawn", t);

    println!("\n| Step | Resident memory | Δ | Took |\n| --- | --- | --- | --- |");
    let mut last = rows[0].1;
    for (name, bytes, took) in &rows {
        let delta = *bytes as i64 - last as i64;
        println!(
            "| {name} | {} | {}{} | {} ms |",
            format_bytes(*bytes),
            if delta < 0 { "-" } else { "+" },
            format_bytes(delta.unsigned_abs()),
            took.as_millis()
        );
        last = *bytes;
    }
}

/// "Open data" adds its SELECT after the user's SQL and runs only that.
#[gpui_kit::test]
async fn postgres_open_data_keeps_the_users_sql(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (_, console, window) = console_on(cx, &url);
    let object = crate::table_menu::ObjectRef::parse(
        &format!(
            "table:{}/postgres/pg_catalog/pg_namespace",
            savoia_core::ConnectionId::new()
        ),
        savoia_core::Engine::Postgres,
    )
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        let editor = console.read(cx).editor().clone();
        editor.update(cx, |e, cx| e.set_value("SELECT 1 AS mine;", window, cx));
        console.update(cx, |c, cx| {
            c.insert_sql(&object.select_sql(), true, window, cx)
        });
    })
    .unwrap();
    idle(cx, &console);

    let text = console.read_with(cx, |c, cx| c.editor().read(cx).value().to_string());
    assert_eq!(
        text,
        "SELECT 1 AS mine;\n\nSELECT * FROM \"pg_catalog\".\"pg_namespace\" LIMIT 200;"
    );
    assert_eq!(console.read_with(cx, |c, _| c.results().len()), 1);
    assert!(grid(cx, &console).0 > 1);
}

/// Connecting without a stored password asks for one; a wrong one asks
/// again with the server's error; the right one connects and is kept.
#[gpui_kit::test]
async fn postgres_connect_asks_for_a_missing_password(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let without = url.replacen(":savoia@", "@", 1);
    assert_ne!(without, url, "SAVOIA_PG_URL has the password savoia");
    cx.executor().allow_parking();
    let (ds, form, window) = mount(cx);
    let asked = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        let asked = asked.clone();
        cx.subscribe(&ds, move |_, event, _| {
            if let DataSourcesEvent::NeedsPassword { error, .. } = event {
                asked.borrow_mut().push(error.clone());
            }
        })
        .detach();
    });
    import(cx, window, &form, &without);
    click(cx, window, "save-connect");
    let id = ds
        .read_with(cx, |ds, _| ds.connections().first().map(|c| c.id))
        .expect("saved");

    wait_until(cx, "the password prompt", |_| asked.borrow().len() == 1);
    assert_eq!(asked.borrow()[0], None, "nothing was tried yet");

    ds.update(cx, |ds, cx| {
        ds.connect_with_password(id, "wrong".into(), cx)
    });
    wait_until(cx, "the second prompt", |_| asked.borrow().len() == 2);
    let error = asked.borrow()[1].clone().expect("the server's error");
    assert!(error.contains("password"), "{error}");

    ds.update(cx, |ds, cx| {
        ds.connect_with_password(id, "savoia".into(), cx)
    });
    wait_until(cx, "the session", |cx| {
        ds.read_with(cx, |ds, _| {
            matches!(ds.state(id), SourceState::Connected(_))
        })
    });
    let secrets = ds.read_with(cx, |ds, _| ds.load_secrets(id)).await.unwrap();
    assert_eq!(secrets.password.as_deref(), Some("savoia"));
}

/// Expanding another Postgres database opens a connection to it and loads
/// its schemas, while a query holds the session's own connection. Refresh
/// keeps them loaded.
#[gpui_kit::test]
async fn postgres_explorer_loads_another_database_on_expand(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (ds, console, window) = console_on(cx, &url);
    let id = ds.read_with(cx, |ds, _| ds.connections()[0].id);
    let session = session_of(cx, &ds).expect("connected");
    exec(
        session.clone(),
        "DROP DATABASE IF EXISTS it_other WITH (FORCE)",
    );
    exec(session.clone(), "CREATE DATABASE it_other");
    ds.update(cx, |ds, cx| ds.refresh(id, cx));
    wait_until(cx, "the refresh", |cx| refresh_state(cx, &ds).is_none());
    let explorer = cx.new(|cx| Explorer::new(ds.clone(), cx));
    settled_labels(cx, &explorer);

    // The console's paused result holds the session's connection.
    run(
        cx,
        window,
        &console,
        "SELECT g FROM generate_series(1, 100000) g",
    );
    wait_until(cx, "the pause", |cx| paused(cx, &console));
    explorer.update(cx, |e, cx| e.expand(&["it_other"], cx));
    let labels = settled_labels(cx, &explorer);
    let db = labels
        .iter()
        .position(|l| l == "it_other")
        .expect("it_other");
    assert_eq!(
        labels.get(db + 1).map(String::as_str),
        Some("public"),
        "{labels:?}"
    );
    assert!(session.is_busy(), "loaded beside the running query");
    click(cx, window, "cancel");

    explorer.update(cx, |e, cx| e.expand(&["it_other", "public"], cx));
    ds.update(cx, |ds, cx| ds.refresh(id, cx));
    wait_until(cx, "the refresh", |cx| refresh_state(cx, &ds).is_none());
    let labels = settled_labels(cx, &explorer);
    let db = labels
        .iter()
        .position(|l| l == "it_other")
        .expect("it_other");
    assert_eq!(
        labels[db + 1..db + 4],
        ["public", "tables", "views"],
        "{labels:?}"
    );

    // FORCE closes the explorer's connection to it.
    exec(session, "DROP DATABASE it_other WITH (FORCE)");
}

/// Each console tab keeps its own editor, runs on the source it was opened
/// on, and closing it cancels its query.
#[gpui_kit::test]
async fn postgres_console_tabs_are_independent(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (ds, _) = save_and_connect(cx, &url);
    let id = ds.read_with(cx, |ds, _| ds.connections()[0].id);
    let session = session_of(cx, &ds).expect("connected");
    cx.update(|cx| {
        crate::console::init(cx);
        crate::workspace::init(cx);
    });
    let (window, workspace) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| Workspace::with_data_sources(ds.clone(), window, cx))
            })
        })
        .expect("window");
    let consoles = |cx: &mut TestAppContext| {
        workspace.read_with(cx, |w, _| w.consoles().cloned().collect::<Vec<_>>())
    };
    workspace.update(cx, |w, cx| {
        w.explorer()
            .clone()
            .update(cx, |e, cx| e.select_first_source(cx))
    });
    cx.update_window(window, |_, window, cx| {
        workspace.update(cx, |w, cx| w.new_console(&NewConsole, window, cx));
    })
    .unwrap();
    let [first, second] = consoles(cx).try_into().expect("two consoles");
    assert_eq!(second.read_with(cx, |c, cx| c.source(cx)), Some(id));

    // Only the active tab renders, so run each console directly.
    let run_in = |cx: &mut TestAppContext, console: &Entity<QueryConsole>, sql: &str| {
        let sql = sql.to_owned();
        cx.update_window(window, |_, window, cx| {
            console.update(cx, |c, cx| {
                c.editor()
                    .clone()
                    .update(cx, |editor, cx| editor.set_value(sql, window, cx));
                c.run(&crate::console::RunQuery, window, cx);
            })
        })
        .unwrap();
    };
    run_in(cx, &first, "SELECT 1");
    wait_until(cx, "the first run", |cx| {
        !first.read_with(cx, |c, _| c.is_running())
    });
    run_in(cx, &second, "SELECT g FROM generate_series(1, 100000) g");
    wait_until(cx, "the pause", |cx| paused(cx, &second));
    assert!(session.is_busy());

    workspace.update(cx, |w, cx| w.close(1, cx));
    drop(second);
    wait_until(cx, "the cancel", |_| !session.is_busy());
    let [first] = consoles(cx).try_into().expect("one console");
    let text = first.read_with(cx, |c, cx| c.editor().read(cx).value().to_string());
    assert_eq!(text, "SELECT 1");
}

/// Run takes the statement at the caret; a selection wins over it.
#[gpui_kit::test]
async fn postgres_run_takes_the_statement_at_the_caret(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (_, console, window) = console_on(cx, &url);
    let sql = "SELECT 1 AS one;\nSELECT 2 AS two;\nSELECT 3 AS three;";
    let run_at = |cx: &mut TestAppContext, range: std::ops::Range<usize>| {
        cx.update_window(window, |_, window, cx| {
            console.update(cx, |c, cx| {
                c.editor().clone().update(cx, |editor, cx| {
                    editor.set_value(sql, window, cx);
                    editor.set_selected_range(range, cx);
                });
                c.run(&crate::console::RunQuery, window, cx);
            })
        })
        .unwrap();
        wait_until(cx, "the run", |cx| {
            !console.read_with(cx, |c, _| c.is_running())
        });
        let results = console.read_with(cx, |c, _| c.results().len());
        (results, grid(cx, &console).1.last().cloned())
    };
    let caret = sql.find("2 AS").unwrap();
    assert_eq!(run_at(cx, caret..caret), (1, Some("2".into())));
    let three = sql.find("SELECT 3").unwrap();
    assert_eq!(run_at(cx, three..sql.len()), (1, Some("3".into())));
}

/// The quick filter hides rows of the result; Copy takes the rows shown,
/// with NULL kept apart from the empty string.
#[gpui_kit::test]
async fn postgres_result_filter_and_copy(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (_, console, window) = console_on(cx, &url);
    run(
        cx,
        window,
        &console,
        "SELECT * FROM (VALUES (1, 'Roma'), (2, 'Torino'), (3, NULL), (4, '')) t(n, name)",
    );
    idle(cx, &console);
    cx.update_window(window, |_, window, cx| {
        let filter = console.read(cx).filter().clone();
        filter.update(cx, |input, cx| input.set_value("tor", window, cx));
        // Typing emits a change; `set_value` doesn't.
        console.update(cx, |c, cx| c.apply_filter(cx));
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        grid(cx, &console).1.last().map(String::as_str),
        Some("Torino")
    );

    cx.update_window(window, |_, window, cx| {
        let filter = console.read(cx).filter().clone();
        filter.update(cx, |input, cx| input.set_value("", window, cx));
        console.update(cx, |c, cx| c.apply_filter(cx));
    })
    .unwrap();
    cx.run_until_parked();
    console.update(cx, |c, cx| c.copy_result(0, cx));
    let copied = cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
    assert_eq!(
        copied.as_deref(),
        Some("n\tname\n1\tRoma\n2\tTorino\n3\t\\N\n4\t\n")
    );
}

/// Every run lands in the history, failed ones with their error; search
/// narrows it, and picking an entry hands its SQL back.
#[gpui_kit::test]
async fn postgres_runs_are_kept_in_the_history(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (ds, console, window) = console_on(cx, &url);
    run(cx, window, &console, "SELECT 41 + 1 AS answer");
    idle(cx, &console);
    run(cx, window, &console, "SELECT no_such_column");
    idle(cx, &console);

    let picked = Rc::new(RefCell::new(None));
    let panel = cx
        .update_window(window, |_, window, cx| {
            let picked = picked.clone();
            cx.new(|cx| {
                HistoryPanel::new(
                    ds.clone(),
                    Rc::new(move |sql: String, _: &mut Window, _: &mut App| {
                        *picked.borrow_mut() = Some(sql)
                    }),
                    window,
                    cx,
                )
            })
        })
        .unwrap();
    let entries = panel.read_with(cx, |p, _| p.entries().to_vec());
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].sql, "SELECT no_such_column");
    assert!(
        entries[0]
            .error
            .as_deref()
            .is_some_and(|e| e.contains("no_such_column"))
    );
    assert_eq!((entries[1].rows, entries[1].error.as_deref()), (1, None));

    cx.update_window(window, |_, window, cx| {
        let search = panel.read(cx).search().clone();
        search.update(cx, |input, cx| input.set_value("41", window, cx));
        panel.update(cx, |p, cx| p.reload(cx));
        panel.update(cx, |p, cx| p.pick(0, window, cx));
    })
    .unwrap();
    assert_eq!(panel.read_with(cx, |p, _| p.entries().len()), 1);
    assert_eq!(picked.borrow().as_deref(), Some("SELECT 41 + 1 AS answer"));
}

/// The Structure tab lists a table's columns, keys and indexes, and the
/// DDL rebuilt from them.
#[gpui_kit::test]
async fn postgres_structure_shows_columns_keys_and_ddl(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (ds, _) = save_and_connect(cx, &url);
    let id = ds.read_with(cx, |ds, _| ds.connections()[0].id);
    exec(
        session_of(cx, &ds).expect("connected"),
        "DROP SCHEMA IF EXISTS it_structure CASCADE;
         CREATE SCHEMA it_structure;
         CREATE TABLE it_structure.teams (id int PRIMARY KEY, name text NOT NULL);
         CREATE TABLE it_structure.players (
           id int PRIMARY KEY,
           team_id int REFERENCES it_structure.teams (id),
           shirt int DEFAULT 10);
         CREATE INDEX players_shirt ON it_structure.players (shirt);",
    );
    let node = NodeRef {
        connection: id,
        database: "savoia".into(),
        schema: "it_structure".into(),
        table: Some("players".into()),
    };
    let (window, view) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(|cx| StructureView::new(ds.clone(), node, cx))
            })
        })
        .expect("window");
    wait_until(cx, "the structure", |cx| {
        view.read_with(cx, |v, _| v.loaded().is_some())
    });
    let info = view.read_with(cx, |v, _| v.loaded().unwrap());
    let columns: Vec<_> = info.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(columns, ["id", "team_id", "shirt"]);
    let ddl = view.read_with(cx, |v, cx| v.ddl(cx).unwrap());
    assert!(ddl.contains("\"shirt\" integer DEFAULT 10"), "{ddl}");
    assert!(
        ddl.contains("REFERENCES \"it_structure\".\"teams\" (\"id\")"),
        "{ddl}"
    );
    assert!(ddl.contains("CREATE INDEX \"players_shirt\""), "{ddl}");
    assert_eq!(info.foreign_keys.len(), 1);
    // Draws without panicking.
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

/// Completion loads what it needs from the catalog, then suggests columns
/// by alias and whole JOIN clauses from foreign keys.
#[gpui_kit::test]
async fn postgres_completion_suggests_columns_and_joins(cx: &mut TestAppContext) {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (ds, console, window) = console_on(cx, &url);
    exec(
        session_of(cx, &ds).expect("connected"),
        "DROP SCHEMA IF EXISTS it_complete CASCADE;
         CREATE SCHEMA it_complete;
         CREATE TABLE it_complete.customers (id int PRIMARY KEY, name text);
         CREATE TABLE it_complete.orders (
           id int PRIMARY KEY,
           customer_id int REFERENCES it_complete.customers (id),
           total numeric);",
    );
    let id = ds.read_with(cx, |ds, _| ds.connections()[0].id);
    ds.update(cx, |ds, cx| ds.refresh(id, cx));
    wait_until(cx, "the refresh", |cx| refresh_state(cx, &ds).is_none());

    let complete = |cx: &mut TestAppContext, sql: &str| -> Vec<String> {
        use gpui_kit::component::input::CompletionProvider as _;
        let provider = crate::completion::SqlCompletion {
            data_sources: ds.clone(),
            console: console.downgrade(),
        };
        let rope = gpui_kit::component::Rope::from(sql);
        let context = lsp_types::CompletionContext {
            trigger_kind: lsp_types::CompletionTriggerKind::INVOKED,
            trigger_character: None,
        };
        let task = cx
            .update_window(window, |_, window, cx| {
                provider.completions(&rope, sql.len(), context, window, cx)
            })
            .unwrap();
        let done = Rc::new(RefCell::new(None));
        let slot = done.clone();
        cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
            .detach();
        wait_until(cx, "the completions", |_| done.borrow().is_some());
        let items = match done.take().unwrap().unwrap() {
            lsp_types::CompletionResponse::Array(items) => items,
            lsp_types::CompletionResponse::List(list) => list.items,
        };
        items.into_iter().map(|i| i.label).collect()
    };

    let columns = complete(cx, "SELECT * FROM it_complete.orders o WHERE o.");
    assert_eq!(columns, ["id", "customer_id", "total"]);
    let joins = complete(cx, "SELECT * FROM it_complete.orders o JOIN ");
    assert_eq!(
        joins.first().map(String::as_str),
        Some("it_complete.customers c ON c.id = o.customer_id"),
        "{joins:?}"
    );
}
