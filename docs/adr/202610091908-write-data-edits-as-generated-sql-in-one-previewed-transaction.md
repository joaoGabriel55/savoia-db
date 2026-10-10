# Write data edits as generated SQL in one previewed transaction

## Status

Accepted

## Context

The data view ([Add a no-SQL data view with visual joins to v1](./202610091908-add-a-no-sql-data-view-with-visual-joins-to-v1.md)) lets users edit cells, insert rows and delete rows. PLAN.md already asks for "pending changes → generated SQL preview → commit", and PRODUCT.md for nothing destructive to happen silently.

Two existing decisions shape how the changes are sent:

- [Stream query results as server-rendered text pages](./202610091303-stream-query-results-as-server-rendered-text-pages.md): values reach the UI as server-rendered text. Postgres's simple protocol carries no type OIDs. The column types come from the catalog (`describe_table`) as the server prints them.
- [Serialize all work on one connection per session](./202610091309-serialize-all-work-on-one-connection-per-session.md): `Connection::execute` takes SQL text on the session's one connection, behind its gate.

Options considered on 2026-10-09:

- **Bind values as typed parameters.** This needs a new `Connection` method with parameter types for each engine, mapping catalog type names to driver types. Postgres would also need the extended protocol, with binary encoding per type, which is exactly what the streaming ADR avoided.
- **Generate SQL text with quoted string literals (chosen).** Every value goes back the way it came: as text. Both engines coerce a string literal to the column's type on `INSERT`/`UPDATE` (Postgres types an untyped literal from its target column; MySQL converts implicitly). The SQL that runs is exactly what the preview shows.

## Decision

**Pending changes become `UPDATE`, `INSERT` and `DELETE` statements with values as quoted string literals. They are shown to the user, then run as one script inside a transaction.**

- **Only tables with a primary key, or a unique key on non-null columns, are editable.** Each statement identifies its row by the key values loaded with the page, in the `WHERE` clause.
- **Values:**
  - NULL is written as `NULL`, never as the empty string.
  - Text is quoted with the engine's escaping (`'` doubled; MySQL backslashes too, unless `NO_BACKSLASH_ESCAPES` is on).
  - For Postgres, the literal is cast to the catalog type when it isn't plain text (`'…'::jsonb`), so overloaded contexts resolve the same way the column would.
  - Binary columns are read-only in v1.
- **Commit** holds the session's gate for the whole exchange. It sends `BEGIN` (`START TRANSACTION` on MySQL) and the statements, and checks that each affected exactly 1 row. Then it sends `COMMIT`, or `ROLLBACK` if any statement failed or matched another number of rows. The pending changes are kept and the failing change is marked. MySQL connections set `CLIENT_FOUND_ROWS`, so an update that rewrites a row with the same values still counts as 1.
- **Read-only connections** never build or send edits. The drivers already refuse writes as well (`default_transaction_read_only` and `SET SESSION TRANSACTION READ ONLY`).
- The generation lives in `savoia-core` and is unit-tested for each engine. Identifier quoting moves there from `savoia-app/src/table_menu.rs`.

## Consequences

- No new driver API. Edits use the same `execute` path, gate and cancel as the console.
- What the preview shows is byte-for-byte what runs, so it can be copied into a console or a migration.
- **Row identity depends on the key values loaded with the page.** A concurrent change to a key makes the update match 0 rows, which the affected-count check turns into a rollback with a clear message instead of a silent no-op. Other concurrent edits to the same row are overwritten (last write wins). Optimistic checks on all original values are possible later.
- A long commit holds the session's gate like any query, and the console waits behind it.
