# Savoia Studio: delivery backlog

The work still missing from [PLAN.md](./PLAN.md), split into tasks small enough to ship one per PR.
Each task lists where it lives, what "done" means, and a rough estimate for one developer.

Status as of 2026-10-10: M0–M4 are done. M5 is done except publishing v0.1.0, which needs the maintainer.

Legend: **[ ]** open · **[~]** partly done · **[x]** done

## Done: M2 (Explore and query)

Already shipped: lazy schema tree with counts, ER diagrams, streamed execute, cancel, multiple result sets, timing and rows-affected, table context menu.

| # | Task | Where | Done when | Est. |
|---|------|-------|-----------|------|
| 2.1 | [x] Multiple console tabs | `workspace.rs`, `console.rs` | "New console" opens another tab bound to a session; each tab keeps its own editor text and results; closing a tab cancels its running query. | ½ day |
| 2.2 | [x] Run the statement under the cursor | `console.rs` (+ a splitter in `savoia-core`) | With no selection, Run executes only the statement around the caret. Today it runs the selection or the whole editor. The splitter is reused by M4 import. | 1 day |
| 2.3 | [x] Copy and export results | `results.rs` | Copy selected cells/rows as TSV, CSV, JSON or INSERT; "Export…" writes the whole result to CSV. NULL and empty string stay distinct. | 1 day |
| 2.4 | [x] Sort and filter in the result grid | `results.rs` | Click a header to sort the loaded rows; a quick filter hides non-matching rows. | ½ day |
| 2.5 | [x] Query history | `savoia-store` (new migration), `console.rs` | Every run is saved with connection, time, duration and row count; a searchable panel re-opens a query in the console. | 1–1½ days |
| 2.6 | [x] Table "Structure" view | `table_menu.rs` (item disabled, tagged M2), new view | Shows columns, indexes, FKs and DDL from `describe_table`. | 1 day |
| 2.7 | [x] Schema-aware completion | `console.rs`, completion provider | Suggests schemas, tables and columns from the loaded catalog, and JOIN conditions from FKs. | 2–3 days |
| 2.8 | [x] Close M2 | `PLAN.md` | M2 marked ✅; UI tests cover tabs, history and completion. | ¼ day |

## Debt carried from M1

| # | Task | Done when | Est. |
|---|------|-----------|------|
| 1.1 | [x] Password prompt on connect | Connecting with no stored password asks for it instead of failing with an auth error. | ½ day |
| 1.2 | [x] Lazy-load non-current Postgres databases | Expanding another database opens a connection to it and loads its schemas. | 1 day |

## Done: M3 (Data view)

A no-SQL view per table that browses, edits and queries data, joins included. See the ADRs on the [data view](./adr/202610091908-add-a-no-sql-data-view-with-visual-joins-to-v1.md), [joins](./adr/202610091908-build-joins-from-foreign-key-relationship-paths.md) and [edits](./adr/202610091908-write-data-edits-as-generated-sql-in-one-previewed-transaction.md).

| # | Task | Done when | Est. |
|---|------|-----------|------|
| 3.1 | [x] Browse | "Open data" opens a data view tab: keyset (or `LIMIT/OFFSET`) pages of 200, header click sorts on the server, filter chips build the `WHERE`; "View SQL" / "Open in console". | 2 days |
| 3.2 | [x] Edit, insert, delete | Edits on key-bearing tables are held as pending changes, highlighted; "Review SQL" shows the statements; Commit runs them in one transaction with a 1-row check each, Discard drops them. | 3–4 days |
| 3.3 | [x] Relationship columns | "+ Column" adds lookups through many-to-one FKs (LEFT JOIN, chained paths), summary columns for one-to-many (count/sum/min/max/avg/list), drill-down to children, an FK value picker, and "Join another table…" for undeclared relations. | 3–4 days |
| 3.4 | [x] Read-only mode | Drivers already set `default_transaction_read_only` (PG) and read-only sessions (MySQL). Left: disable edit UI and show a read-only badge. | ¼ day |
| 3.5 | [x] Summaries | Group by plus aggregates, read-only; "Save as query" opens the SQL in a console. | 2 days |

## Done: M4 (Dump and import)

Lives in the `savoia-transfer` crate. Tools are detected, not bundled: see the [ADR](./adr/202610092308-detect-installed-dump-tools-instead-of-bundling-them.md). The wizards open from the explorer's Dump…/Import… buttons and the table menu's Export…/Import….

| # | Task |
|---|------|
| 4.1 | [x] Tool detection: find `pg_dump`/`pg_restore`/`psql`/`mysqldump`/`mysql` on PATH or a custom path; check version against the server. |
| 4.2 | [x] External runner: pass secrets through 0600 temp files (`PGPASSFILE`, `--defaults-extra-file`), parse progress, cancel, show the log. |
| 4.3 | [x] Built-in exporter: DDL from the catalog plus data to `.sql`, `.sql.gz`, CSV. |
| 4.4 | [x] Import: SQL file runner (reuses the 2.2 splitter; handles `$$`, `DELIMITER`, comments), CSV import with column mapping. |
| 4.5 | [x] Wizards: objects → options → destination → progress screen. |
| 4.6 | [x] Dump and import through SSH tunnels. |
| 4.7 | [x] Round-trip test: dump → restore, compare schema and row checksums. |

## Now: M5 (Polish and release)

Release mechanics are in [RELEASING.md](./RELEASING.md). Decisions: [packaging and updates](./adr/202610100804-package-with-cargo-packager-and-update-from-github-releases.md), [license and distribution](./adr/202610100805-license-under-mit-or-apache-2-and-distribute-through-github-releases.md), [crash reports](./adr/202610100806-report-crashes-as-user-reviewed-github-issues.md) (Proposed).

| # | Task |
|---|------|
| 5.1 | [x] Command palette (⌘⇧P) over every app command, with keybindings for tabs, settings and quit, and a macOS menu bar (`commands.rs`). |
| 5.2 | [x] Settings tab (⌘,): appearance, dump tools folder, updates, crash reports, keyboard list, about (`settings.rs`, `settings_view.rs`). |
| 5.3 | [x] Light theme: Olivetti-paper palette beside graphite in `theme.rs`; Match system follows the OS live. |
| 5.4 | [x] Packaging and auto-update: cargo-packager config, a release workflow that also builds every installer on packaging PRs, signed `latest.json`, and an in-app updater tested end to end on macOS (update installs; a tampered package is refused). The dmg is 12.9 MB. Code signing is deferred: v0.1 ships unsigned on macOS and Windows. |
| 5.5 | [~] Opt-in crash reports, docs site, license files and CHANGELOG are done. Left, for the maintainer: set the two updater secrets, turn on Pages, merge, then tag `v0.1.0` and publish the draft (see [RELEASING.md](./RELEASING.md)). |
| 5.6 | [x] Ko-fi button at the top of README.md, in the status bar and in Settings › About. |

## Release checks

Budgets from PLAN.md, measured before v0.1:

- [x] Cold start under 1 s: about 205 ms; the first launch of a new binary takes 1.1 s while macOS scans it (see [benchmarks/startup.md](./benchmarks/startup.md))
- [x] Idle RAM under 150 MB (see [benchmarks/memory.md](./benchmarks/memory.md))
- [x] Scrolling 1M-row results at 60 fps: 8.3 ms frames (120 Hz) steady, fling and jumps; one noisy run missed on jumps (see [benchmarks/scrolling.md](./benchmarks/scrolling.md))
- [x] Installer under 15 MB (12.9 MB dmg, Apple silicon, 2026-10-10)
