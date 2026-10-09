//! Checks that an unread result doesn't pile up in memory. In its own test
//! binary so no other test changes the process's memory meanwhile. Skipped
//! unless `SAVOIA_PG_URL` is set.

#![cfg(unix)]

use std::time::Duration;

use savoia_core::{Driver, SslMode, parse_url};
use savoia_pg::PgDriver;

/// Resident set size of this process, in KiB.
fn rss_kib() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8(out.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

#[tokio::test]
async fn unread_result_keeps_memory_flat() {
    let Ok(url) = std::env::var("SAVOIA_PG_URL") else {
        return;
    };
    let (mut config, secrets) = parse_url(&url).unwrap();
    config.ssl = SslMode::Disable;
    let conn = PgDriver
        .connect(&config.endpoint(), &secrets)
        .await
        .unwrap();

    let mut handle = conn
        .execute("SELECT generate_series(1, 10000000)".into())
        .await
        .unwrap();
    handle.next().await.unwrap().unwrap(); // columns
    handle.next().await.unwrap().unwrap(); // first page
    let before = rss_kib();
    // Unbounded buffering would take in millions of rows meanwhile.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let grown = rss_kib().saturating_sub(before);
    assert!(grown < 32 * 1024, "memory grew by {grown} KiB while unread");

    drop(handle);
    conn.close().await;
}
