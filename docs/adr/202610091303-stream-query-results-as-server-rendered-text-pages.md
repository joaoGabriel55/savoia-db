# Stream query results as server-rendered text pages

## Status

Accepted

## Context

M2 adds query execution, the first feature where user SQL runs against real servers. The result shape chosen here is used by everything after it: the result grid, copy/export, query history, the M3 table editor and the M4 built-in exporter. It is expensive to change once those exist.

The constraints come from [PLAN.md](../PLAN.md) and [Use native async drivers behind a `Driver` trait](./202610091008-use-native-async-drivers-behind-a-driver-trait.md):

- **Any column type must display, without losing precision.** This includes `numeric`, `bigint`, timestamps with time zones, intervals, arrays, enums, and extension types like PostGIS that we will never model.
- **Results must stream.** The performance budget asks for 1M-row results that scroll at 60 fps with idle RAM under 150 MB. Loading a whole result before showing it fails both.
- **Cancellation must always work**, including while rows are still arriving.
- **The console runs scripts as well as single statements.** One execution can produce several result sets and command completions.
- **The UI must never block.** Drivers run on the Tokio runtime in `savoia-app/src/runtime.rs`. Views await from GPUI's executor, which is a different one.
- **`savoia-core` has no driver or UI dependencies.**

What the pinned drivers actually provide (checked against the sources on 2026-10-09):

- **`tokio-postgres` 0.7.18.**
  - `simple_query_raw` streams text-format rows. Responses go through a `channel(1)`, so if we stop polling, the server is held back by TCP backpressure.
  - The simple protocol accepts several statements in one call.
  - `SimpleColumn` exposes **only the column name, not the type OID**.
  - The extended protocol (`query_raw`) gives types but always asks for **binary** results, which we would have to decode per type.
- **`mysql_async` 0.37.1.**
  - `query_iter` streams text-protocol rows without buffering and walks multiple result sets.
  - `CLIENT_MULTI_STATEMENTS` is enabled by default.
  - Columns carry a `ColumnType` and flags, so the type is always known.

## Decision

**The server renders every value as text. Rows reach the UI as pages over a bounded channel. Type information is attached per column and is best-effort.**

The model in `savoia-core`:

- **A cell is `Option<Box<str>>`.** `None` is SQL `NULL`; `Some("")` is an empty string. The text is exactly what the server sent. We never parse it into numbers or dates, so precision and the session's time-zone rendering are kept.
- **Binary values become hex text**: Postgres `bytea` as the server's `\x…`, and MySQL binary-charset columns as `0x…`, which the driver formats.
- **Each result set starts with column metadata**:
  - `name`
  - `type_name`, the engine's own name, e.g. `timestamptz` or `DECIMAL(10,2)`
  - `kind`, a coarse `ValueKind`: Bool, Integer, Decimal, Float, Text, Temporal, Json, Binary, or Other.

  The type is stored once per column rather than per cell, as PLAN.md had it. A column has one type, so a per-cell tag would only cost memory.
- **`kind` is cosmetic.** It drives alignment and formatting hints, never correctness, and `Other` is always allowed.
  - MySQL always knows the type.
  - Postgres learns it by preparing the statement first, which parses it without running it. This happens only when the input is a single statement.
  - For scripts, or when the prepare fails (for example on `$1` placeholders), Postgres columns fall back to `Other` and `type_name` is left empty.
- **`Connection::execute(sql)` returns a `QueryHandle`** with two parts:
  - **An event receiver.** It yields, in order: `Columns` (a result set begins), `Rows(page)`, and `Done { rows_affected, elapsed }` once per statement, or a single `Err` that ends the execution. Results that arrived before the error are kept.
  - **A cancel handle.** It can be cloned and used from any task. Postgres cancels with `CancelToken`; MySQL sends `KILL QUERY <id>` from a second connection.
- **The driver side runs on the I/O runtime** and fills a bounded channel. A page is sent when it reaches 500 rows, or after 50 ms if rows are waiting, so slow queries still show early rows.
  - When the channel is full, the driver stops polling and the server waits.
  - The UI pulls pages as the grid nears its end (`DataTable::load_more`). Memory therefore grows with how far the user scrolls, not with the size of the result.
  - The channel works without a Tokio runtime, so GPUI's executor can await it.
- **Dropping a handle that hasn't finished cancels the query, then drains the rest in the background.** Otherwise an abandoned 10M-row result would tie up the connection while it emptied.
- **Savoia never rewrites user SQL**, for example by adding a `LIMIT`. Fetching on scroll does that job.

Alternatives considered:

- **Typed values** (`Value::Int(i64)`, `Numeric(BigDecimal)`, `Timestamp(chrono…)`, …). Sorting and editing would be richer. But we would have to decode every type in two engines, get time zones right, and still fall back to text for arrays, enums, domains and extension types. Rejected: the server already renders exactly what users expect to see.
- **Postgres extended protocol with our own binary-to-text formatter.** It gives real type OIDs for scripts too. But it repeats the server's output functions for `numeric`, `interval`, `timestamptz` and every extension type, and any mismatch shows users wrong data. Rejected for the same reason.
- **Postgres server-side cursors** (`DECLARE … CURSOR` / portals with a row limit), which PLAN.md named. Cursors only work inside a transaction and only for queries. They would change the user's transaction state and don't cover scripts. Wire backpressure gives the same memory bound without these problems.
- **Returning the whole result as a `Vec`.** This is simplest, but it fails the 1M-row and RAM budgets and shows nothing until the query finishes.
- **Rows delivered one at a time.** This is one `cx.notify()` per row, which is far too much UI churn. Pages fix that.

## Consequences

- **The grid, copy and export can treat every engine the same**: text in, text out. CSV/TSV/JSON export is a straight copy, and nothing can drift in precision.
- **Postgres type tags are missing for scripts**, so numbers in script results align left. If that annoys users, a later ADR can revisit the binary formatter.
- **An unfinished result keeps its connection busy.** One shared connection per session cannot serve a console and the explorer at the same time. M2 needs a decision on connection ownership, probably a dedicated connection per console tab, with the explorer and catalog on their own.
- **Cancel needs per-connection state**: the Postgres `CancelToken` and the MySQL connection id. MySQL also needs a second connection, through the same tunnel when one is used.
- **Very large cells** (multi-MB `text` or `bytea`) are held in full. A display cap per cell, with "open value in viewer", is a follow-up.
- **M3 editing cannot rely on `kind`.** It must take column types from the catalog, which it needs anyway to find keys.
- **The backpressure claim must be proven, not assumed.** A live test should check that an unread `SELECT` over `generate_series(1, 10000000)` keeps memory flat, and that cancelling it mid-stream returns quickly on both engines.
- **PLAN.md's notes on "PG portals" and a "type tag per value" are now out of date.** Update them if this ADR is accepted.
