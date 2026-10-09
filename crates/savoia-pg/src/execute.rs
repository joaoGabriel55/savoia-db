//! Query execution: the simple query protocol, so the server renders every
//! value as text and scripts with several statements work. See
//! `docs/adr/202610091303-stream-query-results-as-server-rendered-text-pages.md`.

use std::pin::pin;

use async_trait::async_trait;
use futures_util::StreamExt;
use savoia_core::{
    AppResult, Cancel, CancelHandle, Cell, Closed, ColumnMeta, QueryHandle, QuerySender, Row,
    SslMode, ValueKind,
};
use tokio_postgres::types::Type;
use tokio_postgres::{CancelToken, Client, NoTls, SimpleQueryMessage, SimpleQueryStream};

use crate::{query_error, tls};

pub(crate) async fn execute(client: &Client, ssl: SslMode, sql: String) -> AppResult<QueryHandle> {
    let typed = describe(client, &sql).await;
    let stream = client.simple_query_raw(&sql).await.map_err(query_error)?;
    let cancel = CancelHandle::new(PgCancel {
        token: client.cancel_token(),
        ssl,
    });
    let (tx, handle) = QueryHandle::channel(cancel.clone());
    tokio::spawn(pump(stream, tx, typed, cancel));
    Ok(handle)
}

/// Column types for a single statement, learned by parsing it without running
/// it. `None` for scripts (Postgres refuses to prepare several commands), for
/// `$n` placeholders, and for anything the server won't parse; those columns
/// go untyped and the real error, if any, comes from running the SQL.
async fn describe(client: &Client, sql: &str) -> Option<Vec<ColumnMeta>> {
    let statement = client.prepare(sql).await.ok()?;
    Some(
        statement
            .columns()
            .iter()
            .map(|c| ColumnMeta {
                name: c.name().to_owned(),
                type_name: c.type_().name().to_owned(),
                kind: kind_of(c.type_()),
            })
            .collect(),
    )
}

fn kind_of(ty: &Type) -> ValueKind {
    match *ty {
        Type::BOOL => ValueKind::Bool,
        Type::INT2 | Type::INT4 | Type::INT8 | Type::OID => ValueKind::Integer,
        Type::NUMERIC => ValueKind::Decimal,
        Type::FLOAT4 | Type::FLOAT8 => ValueKind::Float,
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::CHAR | Type::UUID => {
            ValueKind::Text
        }
        Type::DATE
        | Type::TIME
        | Type::TIMETZ
        | Type::TIMESTAMP
        | Type::TIMESTAMPTZ
        | Type::INTERVAL => ValueKind::Temporal,
        Type::JSON | Type::JSONB => ValueKind::Json,
        Type::BYTEA => ValueKind::Binary,
        _ => ValueKind::Other,
    }
}

/// Moves the server's stream into the channel. If the UI drops its handle,
/// cancels the query and drains the rest so the connection is free again.
async fn pump(
    stream: SimpleQueryStream,
    mut tx: QuerySender,
    typed: Option<Vec<ColumnMeta>>,
    cancel: CancelHandle,
) {
    let mut stream = pin!(stream);
    let mut typed = typed;
    loop {
        let message = match tx.flush_deadline() {
            Some(deadline) => tokio::select! {
                message = stream.next() => message,
                () = tokio::time::sleep_until(deadline.into()) => {
                    if tx.flush().await.is_err() {
                        break;
                    }
                    continue;
                }
            },
            None => stream.next().await,
        };
        let sent = match message {
            None => return,
            Some(Err(err)) => return tx.fail(query_error(err)).await,
            Some(Ok(SimpleQueryMessage::RowDescription(columns))) => {
                let names = columns.iter().map(|c| c.name());
                // `typed` describes the only statement there is, so use it once.
                let meta = match typed.take() {
                    Some(meta) if meta.len() == columns.len() => meta,
                    _ => names.map(ColumnMeta::untyped).collect(),
                };
                tx.columns(meta).await
            }
            Some(Ok(SimpleQueryMessage::Row(row))) => {
                let cells: Row = (0..row.len())
                    .map(|i| row.get(i).map(Into::into))
                    .collect::<Vec<Cell>>()
                    .into();
                tx.row(cells).await
            }
            Some(Ok(SimpleQueryMessage::CommandComplete(rows))) => tx.done(Some(rows)).await,
            Some(Ok(_)) => Ok(()),
        };
        if sent == Err(Closed) {
            break;
        }
    }
    drop(cancel.cancel().await);
    while let Some(Ok(_)) = stream.next().await {}
}

struct PgCancel {
    token: CancelToken,
    ssl: SslMode,
}

#[async_trait]
impl Cancel for PgCancel {
    async fn cancel(&self) -> AppResult<()> {
        let result = match self.ssl {
            SslMode::Disable => self.token.cancel_query(NoTls).await,
            mode => {
                let tls = tls::connector(mode == SslMode::VerifyFull);
                self.token.cancel_query(tls).await
            }
        };
        result.map_err(query_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_types() {
        assert_eq!(kind_of(&Type::INT8), ValueKind::Integer);
        assert_eq!(kind_of(&Type::NUMERIC), ValueKind::Decimal);
        assert_eq!(kind_of(&Type::TIMESTAMPTZ), ValueKind::Temporal);
        assert_eq!(kind_of(&Type::JSONB), ValueKind::Json);
        assert_eq!(kind_of(&Type::BYTEA), ValueKind::Binary);
        assert_eq!(kind_of(&Type::INT4_ARRAY), ValueKind::Other);
    }
}
