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
| 1.1 | [x] Password prompt on connect | Connecting with no stored password asks for it instead of failing with an auth error. | ½ day |
| 1.2 | [x] Lazy-load non-current Postgres databases | Expanding another database opens a connection to it and loads its schemas. | 1 day |

## Next: M3 (Data view), about 2½ weeks

A no-SQL view per table that browses, edits and queries data, joins included. See the ADRs on the [data view](./adr/202610091908-add-a-no-sql-data-view-with-visual-joins-to-v1.md), [joins](./adr/202610091908-build-joins-from-foreign-key-relationship-paths.md) and [edits](./adr/202610091908-write-data-edits-as-generated-sql-in-one-previewed-transaction.md).

| # | Task | Done when | Est. |
|---|------|-----------|------|
| 3.1 | [ ] Browse | "Open data" opens a data view tab: keyset (or `LIMIT/OFFSET`) pages of 200, header click sorts on the server, filter chips build the `WHERE`; "View SQL" / "Open in console". | 2 days |
| 3.2 | [ ] Edit, insert, delete | Edits on key-bearing tables are held as pending changes, highlighted; "Review SQL" shows the statements; Commit runs them in one transaction with a 1-row check each, Discard drops them. | 3–4 days |
| 3.3 | [ ] Relationship columns | "+ Column" adds lookups through many-to-one FKs (LEFT JOIN, chained paths), summary columns for one-to-many (count/sum/min/max/avg/list), drill-down to children, an FK value picker, and "Join another table…" for undeclared relations. | 3–4 days |
| 3.4 | [~] Read-only mode | Drivers already set `default_transaction_read_only` (PG) and read-only sessions (MySQL). Left: disable edit UI and show a read-only badge. | ¼ day |
| 3.5 | [ ] Summaries | Group by plus aggregates, read-only; "Save as query" opens the SQL in a console. | 2 days |

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
