# Write built-in dumps as plain SQL from one snapshot

## Status

Proposed

## Context

[Use native dump tools when available, with a built-in Rust fallback](./202610091007-use-native-dump-tools-with-built-in-rust-fallback.md) calls for a built-in exporter that writes "plain SQL (DDL from catalog queries plus data)". Backlog task 4.3 builds it in `savoia-transfer`. Three choices shape every dump it writes, and the importer (4.4) and the round-trip test (4.7) depend on them:

- **Where the DDL comes from.** `savoia_core::ddl::create_table` rebuilds `CREATE TABLE` from `describe_table`, but it was written as a reading aid for the Structure view. It loses check constraints, identity and generated columns, `ON DELETE` actions, sequences and enum types, and has no view bodies. The servers can print their own DDL: Postgres through `pg_get_constraintdef`, `pg_get_indexdef`, `pg_get_viewdef` and `format_type`; MySQL through `SHOW CREATE TABLE` / `SHOW CREATE VIEW`.
- **How rows are written.** Postgres `COPY ... FROM stdin` blocks load fastest, but only `psql` can replay them; any other client and our own statement splitter would choke on the data block. One `INSERT` per row works everywhere but is slow to restore and large.
- **Consistency.** The exporter issues many queries (catalog reads, then one `SELECT` per table). Rows written during that window could leave a dump whose tables disagree with each other or break foreign keys.

## Decision

The built-in exporter:

1. **Reads everything inside one read-only snapshot transaction:** `BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY` on Postgres, `START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY` on MySQL. It therefore needs a connection of its own.
2. **Takes DDL from the server's own deparsers**, not from `ddl::create_table`. On Postgres it reads them with `search_path = ''` so every name comes out schema-qualified, as `pg_dump` does. It writes tables without constraints, then rows, then sequence values, constraints, indexes and views. On MySQL it uses `SHOW CREATE` verbatim, with `FOREIGN_KEY_CHECKS = 0`. It strips view `DEFINER`s and keeps each view's session collation. Table names are left unqualified so the dump restores into any database.
3. **Writes rows as multi-row `INSERT`s** (100 rows or 1 MB per statement), never `COPY`. Generated columns are left out, and identity columns use `OVERRIDING SYSTEM VALUE`.
4. **Writes to `<file>.part` and renames the file on success**, so a failed or cancelled export never leaves a file that looks complete.

Not covered, and listed in the dump's header: routines, triggers, grants, partitioned and foreign tables, domains and composite types. Users who need those use the external tools.

Alternatives considered:

- **Reuse `ddl::create_table`.** One DDL generator for the Structure view and dumps, but restored schemas would silently lose constraints and defaults.
- **`COPY` blocks on Postgres.** About 2–3× faster restores, but the dump would only replay through `psql`, and 4.4's importer would need a `COPY` data parser.
- **No snapshot.** Simpler, and doesn't hold a transaction open on the server, but the dump could be inconsistent.

## Consequences

- Dumps replay through `psql`, `mysql`, any GUI client, or our own SQL importer.
- Live round-trip tests (`crates/savoia-transfer/tests/live_builtin.rs`) restore a dump over the original and compare columns, constraints, indexes, views, sequence values and row checksums.
- Restores are slower than `pg_dump`'s `COPY` output. Large databases should use the external tools, which the UI prefers anyway.
- A long export holds a snapshot open on the server, delaying vacuum (Postgres) or purge (MySQL) for its duration.
- Each new kind of object to support (e.g. triggers) means another deparser query per engine.
