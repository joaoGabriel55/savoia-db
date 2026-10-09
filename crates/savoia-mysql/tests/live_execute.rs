//! Live query-execution tests. Skipped unless `SAVOIA_MYSQL_URL` is set, e.g.
//! `SAVOIA_MYSQL_URL=mysql://savoia:savoia@127.0.0.1:33084/savoia` (docker compose).

use std::time::{Duration, Instant};

use savoia_core::{
    AppError, AppResult, Connection, Driver, PAGE_ROWS, QueryEvent, QueryHandle, SslMode,
    ValueKind, parse_url,
};
use savoia_mysql::MysqlDriver;

async fn connect(ssl: SslMode) -> Option<Box<dyn Connection>> {
    let url = std::env::var("SAVOIA_MYSQL_URL").ok()?;
    let (mut config, secrets) = parse_url(&url).expect("SAVOIA_MYSQL_URL must be a valid URL");
    config.ssl = ssl;
    Some(
        MysqlDriver
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

/// A query yielding `limit` rows (up to 10^9) that starts streaming at once.
/// (A cross join of many small tables would make MySQL's hash join buffer
/// almost all of them before the first row.)
fn series(limit: u64) -> String {
    format!(
        "WITH RECURSIVE k(n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM k WHERE n < 999) \
         SELECT a.n * 1000000 + b.n * 1000 + c.n AS n \
         FROM k a CROSS JOIN k b CROSS JOIN k c LIMIT {limit}"
    )
}

#[tokio::test]
async fn single_statement_keeps_server_text_and_types() {
    let Some(conn) = connect(SslMode::Disable).await else {
        return;
    };
    let sql = "SELECT CAST(9007199254740993 AS SIGNED) AS big, CAST(1.10 AS DECIMAL(10,2)) AS dcm,
                      NULL AS nul, '' AS blank, X'00FF' AS bin,
                      TIMESTAMP '2026-01-02 03:04:05' AS ts, JSON_OBJECT('a', 1) AS js";
    let events = all_events(conn.execute(sql.into()).await.unwrap()).await;
    assert_eq!(
        trace(&events),
        ["cols:big,dcm,nul,blank,bin,ts,js", "rows:1", "done:1"]
    );

    let Ok(QueryEvent::Columns(columns)) = &events[0] else {
        unreachable!()
    };
    let kinds: Vec<_> = columns.iter().map(|c| c.kind).collect();
    use ValueKind::*;
    assert_eq!(
        kinds,
        [Integer, Decimal, Other, Text, Binary, Temporal, Json]
    );
    assert_eq!(columns[0].type_name, "BIGINT");

    let Ok(QueryEvent::Rows(rows)) = &events[1] else {
        unreachable!()
    };
    let cells: Vec<Option<&str>> = rows[0].iter().map(|c| c.as_deref()).collect();
    assert_eq!(
        cells,
        [
            Some("9007199254740993"),
            Some("1.10"),
            None,
            Some(""),
            Some("0x00FF"),
            Some("2026-01-02 03:04:05"),
            Some(r#"{"a": 1}"#),
        ]
    );
    conn.close().await;
}

#[tokio::test]
async fn script_reports_each_statement() {
    let Some(conn) = connect(SslMode::Disable).await else {
        return;
    };
    let sql = "CREATE TEMPORARY TABLE it_script (x INT);
               INSERT INTO it_script VALUES (1), (2);
               SELECT x FROM it_script ORDER BY x;";
    let events = all_events(conn.execute(sql.into()).await.unwrap()).await;
    assert_eq!(
        trace(&events),
        ["done:0", "done:2", "cols:x", "rows:2", "done:2"]
    );
    conn.close().await;
}

#[tokio::test]
async fn error_ends_the_script_after_earlier_results() {
    let Some(conn) = connect(SslMode::Disable).await else {
        return;
    };
    let sql = "SELECT 1 AS a; SELECT * FROM it_no_such_table; SELECT 3";
    let events = all_events(conn.execute(sql.into()).await.unwrap()).await;
    assert_eq!(trace(&events), ["cols:a", "rows:1", "done:1", "err"]);
    let Some(Err(AppError::Query { message })) = events.last() else {
        unreachable!()
    };
    assert!(message.contains("doesn't exist"), "{message}");

    // The session is usable afterwards.
    let events = all_events(conn.execute("SELECT 2 AS b".into()).await.unwrap()).await;
    assert_eq!(trace(&events), ["cols:b", "rows:1", "done:1"]);
    conn.close().await;
}

#[tokio::test]
async fn first_statement_error_is_an_event() {
    let Some(conn) = connect(SslMode::Disable).await else {
        return;
    };
    let events = all_events(conn.execute("SELEC 1".into()).await.unwrap()).await;
    assert_eq!(trace(&events), ["err"]);
    conn.close().await;
}

#[tokio::test]
async fn large_results_arrive_in_pages() {
    let Some(conn) = connect(SslMode::Disable).await else {
        return;
    };
    let sql = series(2 * PAGE_ROWS as u64 + 1);
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
    // The server flushes its socket only when its 16 KB buffer fills, so send
    // ~40 KB (well under a page) before pausing for two seconds.
    let sql = format!(
        "SELECT REPEAT('x', 100) AS v FROM ({}) s UNION ALL SELECT SLEEP(2)",
        series(400)
    );
    let started = Instant::now();
    let mut handle = conn.execute(sql).await.unwrap();
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
        let mut handle = conn.execute("SELECT SLEEP(30)".into()).await.unwrap();
        let cancel = handle.cancel_handle();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            cancel.cancel().await.expect("cancel request");
        });
        let started = Instant::now();
        // A killed SLEEP() returns 1 rather than an error.
        while handle.next().await.is_some() {}
        assert!(started.elapsed() < Duration::from_secs(5), "{ssl:?}");

        let events = all_events(conn.execute("SELECT 'free' AS f".into()).await.unwrap()).await;
        assert_eq!(trace(&events), ["cols:f", "rows:1", "done:1"], "{ssl:?}");
        conn.close().await;
    }
}

#[tokio::test]
async fn late_cancel_is_a_no_op() {
    let Some(conn) = connect(SslMode::Disable).await else {
        return;
    };
    let handle = conn.execute("SELECT 1".into()).await.unwrap();
    let cancel = handle.cancel_handle();
    all_events(handle).await;
    cancel.cancel().await.unwrap();
    let events = all_events(conn.execute("SELECT SLEEP(1) AS s".into()).await.unwrap()).await;
    assert_eq!(trace(&events), ["cols:s", "rows:1", "done:1"]);
    conn.close().await;
}

#[tokio::test]
async fn dropping_the_handle_cancels_and_frees_the_session() {
    let Some(conn) = connect(SslMode::Disable).await else {
        return;
    };
    // Far too many rows to drain without cancelling.
    let mut handle = conn.execute(series(1_000_000_000)).await.unwrap();
    handle.next().await.unwrap().unwrap(); // columns
    handle.next().await.unwrap().unwrap(); // first page
    drop(handle);

    let started = Instant::now();
    let events = all_events(conn.execute("SELECT 'free' AS f".into()).await.unwrap()).await;
    assert_eq!(trace(&events), ["cols:f", "rows:1", "done:1"]);
    assert!(started.elapsed() < Duration::from_secs(5));
    conn.close().await;
}
