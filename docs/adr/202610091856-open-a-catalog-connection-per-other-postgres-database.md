# Open a catalog connection per other Postgres database

## Status

Accepted

## Context

A Postgres connection is bound to one database. Its `pg_catalog` describes only that database, and there is no `USE`. Through M2 the explorer listed the server's other databases with nothing under them, and loading their objects failed with "can only be browsed from a connection to it". The backlog carried this from M1 as task 1.2: "Expanding another database opens a connection to it and loads its schemas."

That needs a second connection, which [Serialize all work on one connection per session](./202610091309-serialize-all-work-on-one-connection-per-session.md) rules out: "Each connected data source has exactly one database connection." Its only exception is the short-lived cancel connection. MySQL doesn't have the problem, because `information_schema` covers every database from one connection ([Load schema objects and table details on demand](./202610091437-load-schema-objects-and-table-details-on-demand.md)).

Options considered on 2026-10-09:

- **Switch the session to the other database.** Expanding it reconnects the one connection there. This keeps one connection, but it throws away the console's state (`SET`, temporary tables, an open transaction) whenever the user browses, and the tree of the first database can no longer load.
- **Leave other databases unbrowsable.** Users open a second data source per database. This works today, but it is the gap the backlog item exists to close.
- **One more connection per other database, for the catalog only (chosen).** The user chose this on 2026-10-09.

## Decision

**When the user expands another Postgres database in the explorer, the session opens a connection to it, used only to load that database's catalog (schemas, object names, table details, diagrams). It stays open until the session ends.**

- It connects the way the session's own connection did: same user, secrets, TLS and read-only settings, through the same SSH tunnel if there is one.
- Each such connection has its own gate. Catalog loads of another database don't wait behind the console's query, and the console doesn't wait behind them.
- Console queries and table actions still run only on the session's own connection, in the database the data source opens.
- Refresh reloads the schemas of every database opened this way, and closes the connections of databases that no longer exist.
- MySQL is unchanged: one connection, every database.

This is a second exception to "exactly one connection" in the serialization ADR. That ADR stays in force for everything else.

## Consequences

- Every database on a Postgres server can be browsed and diagrammed from one data source.
- A session holds 1 + *n* server connections, where *n* is the number of other databases the user expanded. On servers with tight `max_connections` or per-user limits, expanding many databases can be refused. The error shows on that database's node.
- `DROP DATABASE` of an expanded database fails while its catalog connection is open, unless it uses `WITH (FORCE)` or the user disconnects first.
- Table actions on another database's tables ("Open data", generated SQL) run on the session's connection, in the wrong database, and fail. Running them there needs a console bound to that database, which belongs with multiple console tabs (backlog 2.1).
- `DatabaseNode.schemas` is now `Option`: `None` means "not loaded", which the explorer shows as a loading row on expand.
