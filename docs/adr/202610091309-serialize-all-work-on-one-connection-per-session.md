# Serialize all work on one connection per session

## Status

Accepted

## Context

[Stream query results as server-rendered text pages](./202610091303-stream-query-results-as-server-rendered-text-pages.md) means that a result still being read keeps its connection busy. With `tokio-postgres`, a second query sent on the same `Client` waits behind the first one's stream. The response channel holds one message, so nothing further arrives until the open result is drained. That ADR left one question open: who owns which connection.

Today a `Session` (`savoia-app/src/session.rs`) holds one `Arc<dyn Connection>` per connected data source:

- Postgres is a single `Client`.
- MySQL is a `Pool` of up to 4 connections (`PoolConstraints::new(0, 4)`).

Options considered on 2026-10-09:

- **A dedicated connection per console tab, with the explorer on its own.** Consoles never block each other or the explorer. But each tab costs a server connection, and a tunnelled session would open one forwarded channel per tab. Session state also splits: a `SET search_path` or an open transaction in one tab is invisible to the others and to the explorer, which surprises users who expect "the connection" to be one thing.
- **One shared connection that buffers whole results before the next query runs.** It never blocks, but it gives up the memory bound the streaming ADR exists for.
- **One shared connection, with work serialized (chosen).** The user chose this.

## Decision

**Each connected data source has exactly one database connection. All work on it (console queries, catalog loads, explorer actions) goes through one gate, one operation at a time.**

- **The gate is an async mutex owned by the session.**
  - `execute` takes the gate, and its `QueryHandle` holds it until the last statement's `Done`, an error, or the end of the cancel-and-drain that follows a drop.
  - Catalog calls take it for the length of their query.
- **Callers that are waiting show it, and never block the UI.** The explorer shows its node as loading, with "waiting for the running query". A second console tab's Run queues up behind the first. Both waits can be abandoned.
- **Cancel never takes the gate.**
  - Postgres cancels with `CancelToken`, which opens its own short-lived socket.
  - MySQL opens a short-lived extra connection just to send `KILL QUERY <id>`, through the tunnel if there is one.

  These cancel-only connections are the one exception to "exactly one connection".
- **MySQL changes from a pool of up to 4 to a single `Conn`,** so that MySQL and Postgres behave the same.

## Consequences

- **The session is one coherent thing, as in `psql` or the `mysql` CLI.** `SET`, `USE`, temporary tables and open transactions are visible everywhere, and the explorer shows what the console sees.
- **That also cuts the other way.**
  - A `USE other_db` in the console changes the MySQL database the explorer's next unqualified catalog query sees. Catalog queries must therefore always qualify by schema or database.
  - In Postgres, a failed statement inside the user's transaction aborts it, and catalog loads then fail with "current transaction is aborted" until the user rolls back. The explorer has to report that clearly rather than look broken.
- **A long-running or unread result blocks the explorer and the other consoles.** The UI makes this visible and offers to cancel, rather than hiding it.
- **The cheapest server footprint:** one connection and one tunnel channel per data source.
- **The M3 table browser and the M4 built-in exporter also wait in line.** Long-running dumps will want their own connection. That is a separate decision for M4, and this ADR doesn't rule it out.
- **Reversing course later is cheap.** The gate lives in the session, not in the `Connection` trait, so moving to connections per tab doesn't change the trait.
