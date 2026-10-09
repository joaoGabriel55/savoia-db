//! Query execution over the text protocol, so the server renders every value
//! and scripts return one result per statement. See
//! `docs/adr/202610091303-stream-query-results-as-server-rendered-text-pages.md`.

use std::fmt::Write as _;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use mysql_async::consts::{ColumnFlags, ColumnType};
use mysql_async::prelude::Queryable;
use mysql_async::{Column, Conn, Opts, TextProtocol, Value};
use savoia_core::{
    AppError, AppResult, Cancel, CancelHandle, Cell, Closed, ColumnMeta, QueryHandle, QuerySender,
    Row, ValueKind,
};
use tokio::sync::OwnedMutexGuard;

use crate::{connect_once, live, query_error};

/// The character set number MySQL uses for binary strings.
const BINARY_CHARSET: u16 = 63;

/// Runs `sql` on the locked connection. The guard moves into the task, so
/// the connection stays taken until the results are read or drained.
pub(crate) fn execute(
    mut guard: OwnedMutexGuard<Option<Conn>>,
    opts: Opts,
    sql: String,
) -> QueryHandle {
    let finished = Arc::new(AtomicBool::new(false));
    let cancel = CancelHandle::new(MysqlCancel {
        opts,
        connection_id: live(&mut guard).id(),
        finished: finished.clone(),
    });
    let (mut tx, handle) = QueryHandle::channel(cancel.clone());
    tokio::spawn(async move {
        let conn = live(&mut guard);
        match conn.query_iter(sql).await {
            Ok(mut result) => match forward(&mut result, &mut tx).await {
                Ok(()) => {}
                Err(Stop::Failed(err)) => tx.fail(err).await,
                // The UI dropped its handle: free the connection quickly.
                Err(Stop::Closed) => {
                    drop(cancel.cancel().await);
                    drop(result.drop_result().await);
                }
            },
            Err(err) => tx.fail(query_error(err)).await,
        }
        // Still under the lock, so no later query of ours can be killed.
        finished.store(true, Ordering::SeqCst);
    });
    handle
}

type QueryResult<'a> = mysql_async::QueryResult<'a, 'static, TextProtocol>;

enum Stop {
    Closed,
    Failed(AppError),
}

impl From<Closed> for Stop {
    fn from(_: Closed) -> Self {
        Stop::Closed
    }
}

/// Moves the server's results into the channel, one result set at a time.
async fn forward(result: &mut QueryResult<'_>, tx: &mut QuerySender) -> Result<(), Stop> {
    loop {
        let columns = result.columns().unwrap_or_default();
        // Read now: once this set's rows run out, `result` points at the next set.
        let affected = result.affected_rows();
        if !columns.is_empty() {
            tx.columns(columns.iter().map(meta).collect()).await?;
        }
        let mut count = 0;
        while let Some(row) = next_row(result, tx).await? {
            count += 1;
            tx.row(cells(row, &columns)).await?;
        }
        let rows_affected = if columns.is_empty() { affected } else { count };
        tx.done(Some(rows_affected)).await?;
        if result.is_empty() {
            // A later statement's error doesn't show in `is_empty()`; it is
            // held back and returned by the next read.
            return match result.next().await {
                Ok(_) => Ok(()),
                Err(err) => Err(Stop::Failed(query_error(err))),
            };
        }
    }
}

/// The next row of the current result set, sending the pending page if the
/// server is slow meanwhile. `next()` reads in place, so it always runs to
/// completion; abandoning it halfway would corrupt the connection.
async fn next_row(
    result: &mut QueryResult<'_>,
    tx: &mut QuerySender,
) -> Result<Option<mysql_async::Row>, Stop> {
    let mut next = pin!(result.next());
    let mut closed = false;
    let row = loop {
        match tx.flush_deadline() {
            Some(deadline) if !closed => tokio::select! {
                row = &mut next => break row,
                () = tokio::time::sleep_until(deadline.into()) => {
                    closed = tx.flush().await.is_err();
                }
            },
            _ => break next.await,
        }
    };
    if closed {
        return Err(Stop::Closed);
    }
    row.map_err(|err| Stop::Failed(query_error(err)))
}

fn cells(row: mysql_async::Row, columns: &[Column]) -> Row {
    row.unwrap_raw()
        .into_iter()
        .zip(columns)
        .map(|(value, column)| cell(value.unwrap_or(Value::NULL), column))
        .collect::<Vec<Cell>>()
        .into()
}

fn cell(value: Value, column: &Column) -> Cell {
    match value {
        Value::NULL => None,
        Value::Bytes(bytes) if is_binary(column) => Some(hex(&bytes).into()),
        Value::Bytes(bytes) => Some(String::from_utf8_lossy(&bytes).into()),
        // The text protocol only sends bytes; anything else is rendered as SQL.
        other => Some(other.as_sql(true).into()),
    }
}

/// `0x00FF`, as the `mysql` client shows binary values.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(2 + bytes.len() * 2);
    out.push_str("0x");
    for b in bytes {
        let _ = write!(out, "{b:02X}");
    }
    out
}

fn is_binary(column: &Column) -> bool {
    use ColumnType::*;
    match column.column_type() {
        MYSQL_TYPE_BIT | MYSQL_TYPE_GEOMETRY => true,
        MYSQL_TYPE_STRING
        | MYSQL_TYPE_VAR_STRING
        | MYSQL_TYPE_VARCHAR
        | MYSQL_TYPE_BLOB
        | MYSQL_TYPE_TINY_BLOB
        | MYSQL_TYPE_MEDIUM_BLOB
        | MYSQL_TYPE_LONG_BLOB => column.character_set() == BINARY_CHARSET,
        _ => false,
    }
}

fn meta(column: &Column) -> ColumnMeta {
    let (name, kind) = type_of(column);
    let unsigned = column.flags().contains(ColumnFlags::UNSIGNED_FLAG) && kind.is_numeric();
    ColumnMeta {
        name: column.name_str().into_owned(),
        type_name: if unsigned {
            format!("{name} UNSIGNED")
        } else {
            name.to_owned()
        },
        kind,
    }
}

fn type_of(column: &Column) -> (&'static str, ValueKind) {
    use ColumnType::*;
    use ValueKind::*;
    let binary = is_binary(column);
    match column.column_type() {
        MYSQL_TYPE_TINY => ("TINYINT", Integer),
        MYSQL_TYPE_SHORT => ("SMALLINT", Integer),
        MYSQL_TYPE_INT24 => ("MEDIUMINT", Integer),
        MYSQL_TYPE_LONG => ("INT", Integer),
        MYSQL_TYPE_LONGLONG => ("BIGINT", Integer),
        MYSQL_TYPE_YEAR => ("YEAR", Integer),
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => ("DECIMAL", Decimal),
        MYSQL_TYPE_FLOAT => ("FLOAT", Float),
        MYSQL_TYPE_DOUBLE => ("DOUBLE", Float),
        MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE => ("DATE", Temporal),
        MYSQL_TYPE_TIME | MYSQL_TYPE_TIME2 => ("TIME", Temporal),
        MYSQL_TYPE_DATETIME | MYSQL_TYPE_DATETIME2 => ("DATETIME", Temporal),
        MYSQL_TYPE_TIMESTAMP | MYSQL_TYPE_TIMESTAMP2 => ("TIMESTAMP", Temporal),
        MYSQL_TYPE_JSON => ("JSON", Json),
        MYSQL_TYPE_BIT => ("BIT", Binary),
        MYSQL_TYPE_GEOMETRY => ("GEOMETRY", Binary),
        MYSQL_TYPE_ENUM => ("ENUM", Text),
        MYSQL_TYPE_SET => ("SET", Text),
        MYSQL_TYPE_STRING if binary => ("BINARY", Binary),
        MYSQL_TYPE_STRING => ("CHAR", Text),
        MYSQL_TYPE_VAR_STRING | MYSQL_TYPE_VARCHAR if binary => ("VARBINARY", Binary),
        MYSQL_TYPE_VAR_STRING | MYSQL_TYPE_VARCHAR => ("VARCHAR", Text),
        MYSQL_TYPE_TINY_BLOB | MYSQL_TYPE_MEDIUM_BLOB | MYSQL_TYPE_LONG_BLOB | MYSQL_TYPE_BLOB
            if binary =>
        {
            ("BLOB", Binary)
        }
        MYSQL_TYPE_TINY_BLOB | MYSQL_TYPE_MEDIUM_BLOB | MYSQL_TYPE_LONG_BLOB | MYSQL_TYPE_BLOB => {
            ("TEXT", Text)
        }
        MYSQL_TYPE_NULL => ("NULL", Other),
        _ => ("", Other),
    }
}

/// `KILL QUERY` from a short-lived second connection; the session's own
/// connection is busy with the query.
struct MysqlCancel {
    opts: Opts,
    connection_id: u32,
    /// Set once the query is over, so a late cancel can't hit a later query.
    finished: Arc<AtomicBool>,
}

#[async_trait]
impl Cancel for MysqlCancel {
    async fn cancel(&self) -> AppResult<()> {
        if self.finished.load(Ordering::SeqCst) {
            return Ok(());
        }
        let mut conn = connect_once(self.opts.clone()).await?;
        let result = conn
            .query_drop(format!("KILL QUERY {}", self.connection_id))
            .await
            .map_err(query_error);
        drop(conn.disconnect().await);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_matches_the_mysql_client() {
        assert_eq!(hex(&[0x00, 0xff, 0x1a]), "0x00FF1A");
        assert_eq!(hex(&[]), "0x");
    }
}
