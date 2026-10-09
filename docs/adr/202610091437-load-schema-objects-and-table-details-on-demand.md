# Load schema objects and table details on demand

## Status

Proposed

## Context

M2 asks for a schema tree that is "lazy-loaded, with counts" and for the driver trait to "list objects, describe table" ([PLAN.md](../PLAN.md)).

Through M1, `Connection::catalog()` loads everything in one go: the database list, every schema of the current database, and every table, view, function and sequence name in them. The explorer builds its whole tree from that one `Catalog`. This has three problems:

- **It doesn't scale.** A Postgres database with a few hundred schemas (one per tenant is common) or tens of thousands of tables makes connect and Refresh slow, and the whole load holds the session's only connection ([Serialize all work on one connection per session](./202610091309-serialize-all-work-on-one-connection-per-session.md)), so the console waits behind it.
- **It stops at names.** Columns, keys and indexes aren't loaded at all, and they are what the tree, completion (M2) and the ER diagram need.
- **Other MySQL databases show nothing**, even though they are on the same server and readable from the same connection through `information_schema`.

Options considered on 2026-10-09:

- **Keep one eager call and add columns and keys to it.** Simple, but it multiplies the problem above: one Refresh would read every column of every table.
- **Load per level, on expand (chosen).** The cost of a call depends on what the user opened, not on the size of the server.
- **Load on demand but in the background, ahead of the user** (prefetching every schema after connect). This is what completion will eventually want, but it would hold the shared connection for long stretches. It can be added on top of the per-level calls later.

## Decision

**The `Connection` trait loads the catalog in three levels, each on demand:**

1. **`catalog()`**: the databases and, for each database the connection can read, its schemas with object *counts* only (tables, views, functions, sequences). No object names.
   - Postgres: the schemas of the current database. Other databases still need their own connection and stay unloaded (deferred, as in M1).
   - MySQL: every database, each with its one schema, because `information_schema` covers the whole server.
2. **`list_objects(database, schema)`**: the object names of one schema. The explorer calls it when a schema is expanded.
3. **`describe_table(database, schema, table)`** and **`describe_schema(database, schema)`**: columns (name, type as the server prints it, nullability, default), primary key, foreign keys (with referenced schema, table and columns) and indexes, for one table or for every table and view of a schema. The explorer calls the first when a table is expanded; the ER diagram calls the second.

Both drivers read the system catalogs (`pg_catalog` and `information_schema`) with every query qualified by schema, as the serialization ADR requires.

**The session caches what was loaded** and owns the cache, not the driver:

- Each call takes the session's gate, so a running query delays it and the explorer shows "waiting for the running query".
- Refresh reloads `catalog()`, then reloads the object lists of the schemas that were loaded before, so open branches stay open. Table details are dropped and reloaded when their node is next shown expanded.
- The explorer asks for a level when its node is expanded and has no data yet, including after auto-open and after Refresh. Each node has one load in flight at most.

## Consequences

- Connect and Refresh cost the same on a database with 10 tables or 10,000.
- MySQL users can browse every database on the server, not only the default one.
- Expanding a node is now a round trip. On a slow link or behind a long query it shows a loading row instead of the children.
- The data for completion (M2) and the ER diagram comes from the same cache, so they don't add their own catalog queries.
- `SchemaNode` changes shape: counts are always present, and object names are `None` until loaded. Code that read names from `catalog()` (the explorer) moves to the cache.
- Counts and object lists can disagree after DDL until the next Refresh. That is accepted; Refresh fixes both.
