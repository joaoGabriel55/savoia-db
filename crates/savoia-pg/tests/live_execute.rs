//! Live query-execution tests. Skipped unless `SAVOIA_PG_URL` is set, e.g.
//! `SAVOIA_PG_URL=postgres://savoia:savoia@127.0.0.1:54317/savoia` (docker compose).

use std::time::{Duration, Instant};

use savoia_core::{
    AppError, AppResult, Connection, Driver, PAGE_ROWS, QueryEvent, QueryHandle, SslMode,
    ValueKind, parse_url,
};
use savoia_pg::PgDriver;

async fn connect(ssl: SslMode) -> Option<Box<dyn Connection>> {
    let url = std::env::var("SAVOIA_PG_URL").ok()?;
    let (mut config, secrets) = parse_url(&url).expect("SAVOIA_PG_URL must be a valid URL");
    config.ssl = ssl;
    Some(
        PgDriver
            .connect(&config.endpoint(), &secrets)
            .await
            .expect("connect"),
    )
}

async fn all_events(mut handle: QueryHandle) -> Vec<AppResult<QueryEvent>> {
    let mut events = Vec::new();
    while let Some(event) = handle.next().await {
        events.push(event);
    }
    events
}

/// A compact trace: "cols:a,b", "rows:N", "done:N", "err".
fn trace(events: &[AppResult<QueryEvent>]) -> Vec<String> {
    events
        .iter()
        .map(|e| match e {
            Ok(QueryEvent::Columns(c)) => {
                let names: Vec<_> = c.iter().map(|c| c.name.as_str()).collect();
                format!("cols:{}", names.join(","))
            }
            Ok(QueryEvent::Rows(rows)) => format!("rows:{}", rows.len()),
            Ok(QueryEvent::Done { rows_affected, .. }) => {
                format!(
                    "done:{}",
                    rows_affected.map_or("-".into(), |n| n.to_string())
                )
            }
            Err(_) => "err".into(),
        })
        .collect()
}

#[tokio::test]
async fn single_statement_keeps_server_text_and_types() {
    let Some(conn) = connect(SslMode::Disable).await else {
        return;
    };
    let sql = r"SELECT 9007199254740993::int8 AS big, 1.10::numeric(10,2) AS dec,
                       NULL::text AS nul, ''::text AS empty, '\x00ff'::bytea AS bin,
                       '2026-01-02 03:04:05+00'::timestamptz AS ts, ARRAY[1,2] AS arr";
    let events = all_events(conn.execute(sql.into()).await.unwrap()).await;
    assert_eq!(
        trace(&events),
        ["cols:big,dec,nul,empty,bin,ts,arr", "rows:1", "done:1"]
    );

    let Ok(QueryEvent::Columns(columns)) = &events[0] else {
        unreachable!()
    };
    let kinds: Vec<_> = columns.iter().map(|c| c.kind).collect();
    use ValueKind::*;
    assert_eq!(
        kinds,
        [Integer, Decimal, Text, Text, Binary, Temporal, Other]
    );
    assert_eq!(columns[5].type_name, "timestamptz");

    let Ok(QueryEvent::Rows(rows)) = &events[1] else {
        unreachable!()
    };
    let cells: Vec<Option<&str>> = rows[0].iter().map(|c| c.as_deref()).collect();
    assert_eq!(cells[0], Some("9007199254740993"));
    assert_eq!(cells[1], Some("1.10"));
    assert_eq!(cells[2], None);
    assert_eq!(cells[3], Some(""));
    assert_eq!(cells[4], Some(r"\x00ff"));
    assert!(cells[5].unwrap().starts_with("2026-01-0"), "{:?}", cells[5]);
    assert_eq!(cells[6], Some("{1,2}"));
    conn.close().await;
}

#[tokio::test]
async fn script_reports_each_statement() {
    let Some(conn) = connect(SslMode::Disable).await else {
        return;
    };
    let sql = "CREATE TEMP TABLE it_script (x int);
               INSERT INTO it_script VALUES (1), (2);
               SELECT x FROM it_script ORDER BY x;";
    let events = all_events(conn.execute(sql.into()).await.unwrap()).await;
    assert_eq!(
        trace(&events),
        ["done:0", "done:2", "cols:x", "rows:2", "done:2"]
    );
    // Scripts can't be described, so their columns are untyped.
    let Ok(QueryEvent::Columns(columns)) = &events[2] else {
        unreachable!()
    };
    assert_eq!(columns[0].kind, ValueKind::Other);
    conn.close().await;
}

#[tokio::test]
async fn error_ends_the_script_after_earlier_results() {
    let Some(conn) = connect(SslMode::Disable).await else {
        return;
    };
    let events = all_events(
        conn.execute("SELECT 1 AS a; SELECT 1/0; SELECT 3".into())
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(trace(&events), ["cols:a", "rows:1", "done:1", "err"]);
    let Some(Err(AppError::Query { message })) = events.last() else {
        unreachable!()
    };
    assert!(message.contains("division by zero"), "{message}");

    // The session is usable afterwards.
    let events = all_events(conn.execute("SELECT 2".into()).await.unwrap()).await;
    assert_eq!(trace(&events), ["cols:?column?", "rows:1", "done:1"]);
    conn.close().await;
}

#[tokio::test]
async fn large_results_arrive_in_pages() {
    let Some(conn) = connect(SslMode::Disable).await else {
        return;
    };
    let n = 2 * PAGE_ROWS + 1;
    let sql = format!("SELECT * FROM generate_series(1, {n})");
    let events = all_events(conn.execute(sql).await.unwrap()).await;
    let pages: Vec<String> = trace(&events)
        .into_iter()
        .filter(|t| t.starts_with("rows:"))
        .collect();
    assert_eq!(
        pages,
        [
            format!("rows:{PAGE_ROWS}"),
            format!("rows:{PAGE_ROWS}"),
            "rows:1".into()
        ]
    );
    conn.close().await;
}

#[tokio::test]
async fn slow_rows_are_not_held_back_for_a_full_page() {
    let Some(conn) = connect(SslMode::Disable).await else {
        return;
    };
    // The backend flushes its socket only when its 8 KB buffer fills, so send
    // ~20 KB (well under a page) before pausing for two seconds.
    let sql = "SELECT repeat('x', 100) FROM generate_series(1, 200)
               UNION ALL SELECT 'late' FROM pg_sleep(2)";
    let started = Instant::now();
    let mut handle = conn.execute(sql.into()).await.unwrap();
    handle.next().await.unwrap().unwrap(); // columns
    let first = handle.next().await.unwrap().unwrap();
    let QueryEvent::Rows(rows) = first else {
        panic!("expected rows, got {first:?}");
    };
    assert!(rows.len() < PAGE_ROWS, "{}", rows.len());
    assert!(started.elapsed() < Duration::from_millis(1500));
    drop(handle);
    conn.close().await;
}

#[tokio::test]
async fn cancel_stops_a_running_query() {
    for ssl in [SslMode::Disable, SslMode::Require] {
        let Some(conn) = connect(ssl).await else {
            return;
        };
        let mut handle = conn.execute("SELECT pg_sleep(30)".into()).await.unwrap();
        let cancel = handle.cancel_handle();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            cancel.cancel().await.expect("cancel request");
        });
        let started = Instant::now();
        let mut last = None;
        while let Some(event) = handle.next().await {
            last = Some(event);
        }
        assert!(started.elapsed() < Duration::from_secs(5), "{ssl:?}");
        let Some(Err(AppError::Query { message })) = last else {
            panic!("{ssl:?}: expected a cancel error, got {last:?}");
        };
        assert!(message.contains("canceling statement"), "{message}");
        conn.close().await;
    }
}

#[tokio::test]
async fn dropping_the_handle_cancels_and_frees_the_session() {
    let Some(conn) = connect(SslMode::Disable).await else {
        return;
    };
    // Far too many rows to drain without cancelling. In the select list, rows
    // stream as generated; in FROM, the server would build them all first.
    let mut handle = conn
        .execute("SELECT generate_series(1, 1000000000)".into())
        .await
        .unwrap();
    handle.next().await.unwrap().unwrap(); // columns
    handle.next().await.unwrap().unwrap(); // first page
    drop(handle);

    let started = Instant::now();
    let events = all_events(conn.execute("SELECT 'free'".into()).await.unwrap()).await;
    assert_eq!(trace(&events), ["cols:?column?", "rows:1", "done:1"]);
    assert!(started.elapsed() < Duration::from_secs(5));
    conn.close().await;
}
