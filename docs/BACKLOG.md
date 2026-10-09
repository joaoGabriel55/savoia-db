# Savoia Studio: delivery backlog

The work still missing from [PLAN.md](./PLAN.md), split into tasks small enough to ship one per PR.
Each task lists where it lives, what "done" means, and a rough estimate for one developer.

Status as of 2026-10-09: M0 and M1 are done. M2 is partly done. M3–M5 have not started, except read-only sessions (already enforced in the drivers).

Legend: **[ ]** open · **[~]** partly done · **[x]** done

## Now: finish M2 (Explore and query)

Already shipped: lazy schema tree with counts, ER diagrams, streamed execute, cancel, multiple result sets, timing and rows-affected, table context menu.

| # | Task | Where | Done when | Est. |
|---|------|-------|-----------|------|
| 2.1 | [ ] Multiple console tabs | `workspace.rs`, `console.rs` | "New console" opens another tab bound to a session; each tab keeps its own editor text and results; closing a tab cancels its running query. | ½ day |
| 2.2 | [ ] Run the statement under the cursor | `console.rs` (+ a splitter in `savoia-core`) | With no selection, Run executes only the statement around the caret. Today it runs the selection or the whole editor. The splitter is reused by M4 import. | 1 day |
| 2.3 | [ ] Copy and export results | `results.rs` | Copy selected cells/rows as TSV, CSV, JSON or INSERT; "Export…" writes the whole result to CSV. NULL and empty string stay distinct. | 1 day |
| 2.4 | [ ] Sort and filter in the result grid | `results.rs` | Click a header to sort the loaded rows; a quick filter hides non-matching rows. | ½ day |
| 2.5 | [ ] Query history | `savoia-store` (new migration), `console.rs` | Every run is saved with connection, time, duration and row count; a searchable panel re-opens a query in the console. | 1–1½ days |
| 2.6 | [ ] Table "Structure" view | `table_menu.rs` (item disabled, tagged M2), new view | Shows columns, indexes, FKs and DDL from `describe_table`. | 1 day |
| 2.7 | [ ] Schema-aware completion | `console.rs`, completion provider | Suggests schemas, tables and columns from the loaded catalog, and JOIN conditions from FKs. | 2–3 days |
| 2.8 | [ ] Close M2 | `PLAN.md` | M2 marked ✅; UI tests cover tabs, history and completion. | ¼ day |

## Debt carried from M1

| # | Task | Done when | Est. |
|---|------|-----------|------|
| 1.1 | [ ] Password prompt on connect | Connecting with no stored password asks for it instead of failing with an auth error. | ½ day |
| 1.2 | [ ] Lazy-load non-current Postgres databases | Expanding another database opens a connection to it and loads its schemas. | 1 day |

## Next: M3 (Data editing), about 2 weeks

| # | Task | Done when |
|---|------|-----------|
| 3.1 | [ ] Table browser | "Open data" pages server-side with `LIMIT/OFFSET` (or keyset), sort and filter run as SQL. |
| 3.2 | [ ] Inline edit buffer | Cell edits on tables with a PK/unique key are held as pending changes, shown highlighted. |
| 3.3 | [ ] SQL preview and commit | Pending changes become UPDATE/INSERT/DELETE shown before running; commit runs them in one transaction, rollback discards. |
| 3.4 | [~] Read-only mode | Drivers already set `default_transaction_read_only` (PG) and read-only sessions (MySQL). Left: disable edit UI and show a read-only badge. |

## Then: M4 (Dump and import), about 3 weeks

Needs the `savoia-transfer` crate, which does not exist yet. The Export… and Import… menu items are already in place, disabled and tagged M4.

| # | Task |
|---|------|
| 4.1 | [ ] Tool detection: find `pg_dump`/`pg_restore`/`psql`/`mysqldump`/`mysql` on PATH or a custom path; check version against the server. |
| 4.2 | [ ] External runner: pass secrets through 0600 temp files (`PGPASSFILE`, `--defaults-extra-file`), parse progress, cancel, show the log. |
| 4.3 | [ ] Built-in exporter: DDL from the catalog plus data to `.sql`, `.sql.gz`, CSV. |
| 4.4 | [ ] Import: SQL file runner (reuses the 2.2 splitter; handles `$$`, `DELIMITER`, comments), CSV import with column mapping. |
| 4.5 | [ ] Wizards: objects → options → destination → progress screen. |
| 4.6 | [ ] Dump and import through SSH tunnels. |
| 4.7 | [ ] Round-trip test: dump → restore, compare schema and row checksums. |

## Last: M5 (Polish and release), about 2 weeks

| # | Task |
|---|------|
| 5.1 | [ ] Command palette and keybindings |
| 5.2 | [ ] Settings screen |
| 5.3 | [ ] Light theme (tokens in `theme.rs`) |
| 5.4 | [ ] Packaging: dmg, msi/nsis, AppImage/deb/rpm; macOS notarization and Windows signing; auto-update |
| 5.5 | [ ] Opt-in crash reporting, docs site, v0.1 release |

## Decisions that block delivery

These need an ADR before the task that depends on them starts.

| Decision | Blocks |
|----------|--------|
| Bundle `pg_dump`/`mysqldump` per OS, or only detect them | 4.1 |
| Packaging and updater tool (Tauri is gone) | 5.4 |
| License and distribution channel | 5.5 |

## Release checks

Budgets from PLAN.md, measured before v0.1:

- [ ] Cold start under 1 s
- [x] Idle RAM under 150 MB (see [benchmarks/memory.md](./benchmarks/memory.md))
- [ ] Scrolling 1M-row results at 60 fps
- [ ] Installer under 15 MB
