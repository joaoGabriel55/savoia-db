# Use native async drivers behind a `Driver` trait

## Status

Accepted

## Context

A DB client runs arbitrary user SQL against arbitrary schemas. It must show any column type as text, stream large results, cancel running queries, run multiple statements, and read catalog metadata. MySQL and PostgreSQL come first, and more engines (SQLite, MariaDB nuances, maybe SQL Server) are likely later.

Options evaluated:

- **`sqlx`.** One API for PG/MySQL/SQLite. It's built for compile-time-checked app queries. Dynamic decoding of unknown types is awkward, and query cancellation and simple-protocol multi-statement support are limited.
- **`tokio-postgres` + `mysql_async`.** Engine-native async drivers. `tokio-postgres` has `simple_query` (text values for every type), `COPY` streaming and `CancelToken`. `mysql_async` has text-protocol queries, multi-result sets and `KILL QUERY` via a second connection.
- **ODBC/JDBC bridges.** Rejected because they add heavy external dependencies.

## Decision

Define a `Driver` / `Connection` trait in a `savoia-core` crate with capabilities: connect/test, list schema objects, execute (streamed pages + result-set metadata), cancel, begin/commit/rollback, and quote identifiers. Implement it with **`tokio-postgres`** (TLS via `rustls`) and **`mysql_async`**. Expose engine differences as capability flags so the UI adapts instead of erroring.

## Consequences

- Full control over streaming, cancellation and type rendering, which are the parts users notice.
- More code than `sqlx`: two type-mapping layers and two catalog query sets.
- Adding an engine means implementing the trait. The UI stays engine-agnostic.
