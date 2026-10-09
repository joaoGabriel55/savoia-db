# Savoia Studio: build plan

A lightweight but rich, cross-platform desktop database client. MySQL and PostgreSQL come first. 100% Rust, with a native GPU-rendered UI (GPUI Kit). DataGrip-style IDE layout with Beekeeper-style connection flow and Savoia's own branding.

Decisions behind this plan live in [`docs/adr/`](./adr/README.md). Open work, task by task, is in [BACKLOG.md](./BACKLOG.md).

## 1. Product scope

**v1 must-haves**

- Connection manager: saved and recent connections, color tags, filter, test/connect, import from URL (`postgres://…`, `mysql://…`).
- Connection options: host/port or socket, user/password, default DB, SSL/TLS (modes + CA/client cert), SSH tunnel (password/key/agent), read-only mode.
- Schema explorer: databases → schemas → tables/views/routines/sequences, plus columns, indexes, FKs and DDL view.
- SQL editor: tabs, highlighting, schema-aware autocomplete, run the statement under the cursor or the selection, cancel, query history.
- Result grid: virtualized and streamed, sort/filter, copy as TSV/CSV/JSON/INSERT, NULL vs empty distinction.
- Table data view: paginated browse and inline edit (pending changes → generated SQL preview → commit). Edits are disabled in read-only mode. It doubles as a no-SQL query view: related columns through foreign keys, filters, summaries ([ADR](./adr/202610091908-add-a-no-sql-data-view-with-visual-joins-to-v1.md)).
- **Dump**: whole DB or selected schemas/tables; schema-only, data-only or both; SQL, custom (pg) or CSV per table. Progress, cancel, log.
- **Import**: `.sql` file (stop or continue on error), CSV into a table with column mapping, pg custom archives via `pg_restore`.
- Light and dark theme, keyboard-first, command palette.

**Later**: ER diagram, table designer (ALTER UI), schema diff, saved queries/folders, SQLite/MariaDB/SQL Server, AI assistant, query plan visualizer.

## 2. Architecture

```
savoia-studio/
├─ Cargo.toml                 # workspace; gpui-kit pinned exactly
├─ crates/
│  ├─ savoia-app/             # binary: GPUI views (workspace, explorer, console, results, dialogs), theme, assets
│  ├─ savoia-core/            # domain types, Driver/Connection traits, errors, capabilities
│  ├─ savoia-pg/              # tokio-postgres impl + PG catalog queries
│  ├─ savoia-mysql/           # mysql_async impl + MySQL catalog queries
│  ├─ savoia-tunnel/          # SSH tunnel (russh) → local port forward
│  ├─ savoia-transfer/        # DumpEngine/ImportEngine: external CLI runner + built-in exporter
│  └─ savoia-store/           # app SQLite (connections, history, settings) + secrets file (0600)
└─ docs/                      # PLAN.md, adr/
```

Key design rules:

- **UI never blocks.** Views own GPUI entities. Database work runs on a Tokio runtime owned by a `SessionRegistry`, and results flow back to entities via channels and `cx.spawn` / `cx.notify()`.
- **Core crates know nothing about GPUI.** `savoia-app` is the only crate depending on `gpui-kit`, so drivers, dumps and storage are testable headless.
- **Streaming results.** Execute returns a query handle, and rows arrive in pages over a bounded channel that append to the `DataTable` delegate (`load_more` / `has_more`). See [ADR](./adr/202610091303-stream-query-results-as-server-rendered-text-pages.md).
- **Values are server-rendered text, with NULL kept distinct; type info is per column.** Never lose precision (numeric, bigint, timestamps tz).
- **Cancellation everywhere.** Queries use PG `CancelToken` or MySQL `KILL QUERY`. Dumps and imports use a child-process kill or a `CancellationToken`.
- **Errors**: `thiserror` in the crates, surfaced in the UI as notifications plus the console Output tab.
- **Theme**: all colors come from `savoia-app/src/theme.rs` tokens. Icons must be registered in `assets.rs`.

## 3. Milestones

Each milestone ends with a runnable build on macOS, Windows and Linux.

### M0: Foundations (week 1) ✅
- Cargo workspace, `savoia-core`, `savoia-app` on `gpui-kit` 0.7.1.
- DataGrip-style shell: title bar, Database Explorer (toolbar + tree), console tab with toolbar, SQL editor (tree-sitter SQL), resizable result grid, status bar. Demo data only.
- Savoia theme (graphite + amber) and extra Lucide icons.
- CI (GitHub Actions): fmt, clippy `-D warnings`, tests, and a build matrix for macOS, Windows and Linux.
- Docker compose with Postgres 13/17 and MySQL 8.0/8.4.

### M1: Connections (weeks 2–3) ✅
- `savoia-core`: `ConnectionConfig` (SSL modes, SSH, read-only, color), URL import, `Driver`/`Connection` traits, catalog model, typed errors.
- `savoia-pg` (tokio-postgres + rustls) and `savoia-mysql` (mysql_async + rustls): connect/test, TLS disable/prefer/require/verify-full, read-only sessions, catalog (databases, schemas, tables, views, functions, sequences).
- `savoia-store`: SQLite (migrations via `user_version`) for connections and recents; secrets in a user-only `0600` file, or in memory when not saved.
- `savoia-tunnel` (russh): password, private-key and agent auth; known_hosts verification with explicit trust; concurrent forwarded sessions.
- UI: connection dialog (URL import, Test, Save, Save & Connect, color), explorer of real catalogs (double-click to connect, context menu, auto-open of the default schema), host-trust prompt, error notifications, live status bar.
- Tests: unit tests, live suites against Docker (Postgres with TLS, MySQL, SSH bastion), and headless UI tests driving the real form and explorer. CI runs the live suites.
- Deferred, then done in M2: password prompt on connect; lazy loading of non-current Postgres databases.

### M2: Explore and query (weeks 4–6)
- Driver trait: list objects, describe table, execute (streamed), cancel.
- Schema tree (lazy-loaded, with counts), multiple console tabs, schema-aware completion via the editor's completion provider (tables, columns, JOIN suggestions using FKs, like DataGrip).
- Virtualized result grid, multiple result sets, timing/rows-affected, copy/export of results.
- Query history (searchable).

### M3: Data view (weeks 7–9)
- No-SQL data view per table, with server-side pages, sort and filter chips.
- Inline edits, inserts and deletes for rows with a PK/unique key: pending-change buffer → SQL preview → transaction commit/rollback ([ADR](./adr/202610091908-write-data-edits-as-generated-sql-in-one-previewed-transaction.md)).
- Joins without join syntax: lookup columns through many-to-one FKs, summary columns and drill-down for one-to-many, an FK value picker ([ADR](./adr/202610091908-build-joins-from-foreign-key-relationship-paths.md)).
- Summaries: group by plus aggregates, read-only.
- Read-only mode enforced in Rust as well as the UI (PG `default_transaction_read_only`, MySQL `SET SESSION TRANSACTION READ ONLY`).

### M4: Dump and import (weeks 9–11)
- Tools detection (PATH + custom paths) and version check against the server.
- External runner: pg_dump/pg_restore/psql, mysqldump/mysql. Secrets via `PGPASSFILE`/`--defaults-extra-file` temp files (0600, deleted after). Progress parsing, cancel, log viewer.
- Built-in exporter: DDL from catalogs + data (`COPY TO STDOUT` / streaming SELECT) → `.sql`, `.sql.gz`, CSV.
- Import: SQL file runner with statement splitter (handles `$$`, `DELIMITER`, comments), CSV import with mapping and batch inserts / `COPY FROM`.
- Wizards: select objects → options → destination → run screen with progress.
- Dump/import over SSH tunnels.

### M5: Polish and release (weeks 12–13)
- Command palette, keybindings, settings, light theme.
- Packaging with `cargo-packager` (or similar; needs an ADR): dmg, msi/nsis, AppImage/deb/rpm. Code signing (macOS notarization, Windows signing), auto-update.
- Crash/error reporting (opt-in), docs site, v0.1 release.

## 4. Testing strategy

- **Unit**: type mapping, URL parsing, SQL splitter, identifier quoting, DDL generation (snapshot tests with `insta`).
- **Integration**: `testcontainers` against every supported server version. The dump → restore round-trip is checked by comparing schema + row checksums.
- **UI**: `gpui-kit` `test-support` headless window tests for views (explorer expand/collapse, console run/cancel, grid paging).
- **Performance budget**: cold start < 1 s, idle RAM < 150 MB, scrolling 1M-row results at 60 fps, installer < 15 MB.

## 5. Risks

| Risk | Mitigation |
| --- | --- |
| GPUI is pre-1.0; breaking API changes | Pin `gpui-kit` exactly, keep UI code in `savoia-app` only, upgrade deliberately with a checklist. |
| Missing widgets vs. web ecosystem | Build on GPUI primitives. Upstream fixes to gpui-kit where sensible. |
| `pg_dump` version mismatch with the server | Version check, a clear error, and a setting to choose a specific binary. Fall back to the built-in engine. |
| Huge result sets exhausting memory | Wire backpressure (PG simple-query stream, MySQL unbuffered), bounded channels, and a "fetch more" model. |
| Secrets leaking via argv/logs | Temp option files, redaction in logs, and secrets that never return to the UI. |
| Scope creep toward a full Beekeeper/DBeaver clone | Hold the v1 list above. New engines and features need an ADR or roadmap entry. |

## 6. Open decisions (need ADRs)

- Bundle pg_dump/mysqldump binaries per OS, or only detect them?
- Packaging/updater tool now that Tauri is gone.
- License, and update/distribution channel.
