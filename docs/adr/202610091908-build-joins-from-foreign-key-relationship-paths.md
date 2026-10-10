# Build joins from foreign-key relationship paths

## Status

Accepted

## Context

The no-SQL data view ([Add a no-SQL data view with visual joins to v1](./202610091908-add-a-no-sql-data-view-with-visual-joins-to-v1.md)) has to let users combine tables without choosing join types or writing join conditions. The catalog already loads every table's foreign keys on demand (`describe_table` / `describe_schema`, [Load schema objects and table details on demand](./202610091437-load-schema-objects-and-table-details-on-demand.md)), and the ER diagram draws from the same data.

Two things make joins hard for users in grid tools:

- **Row multiplication.** Joining orders to order items turns one order into *n* rows, so the grid no longer shows "orders", and editing becomes ambiguous.
- **Join mechanics.** Users have to pick inner vs. outer joins and write conditions, and an inner join silently drops orders that have no customer.

Options considered on 2026-10-09:

- **Free joins with a type picker**, like DataGrip's or DBeaver's visual builders. Most general, but it exposes both problems above.
- **Only many-to-one lookups.** Simple and safe, but "how many items does this order have?" would need the console.
- **Relationship paths: lookups for many-to-one, summaries or drill-down for one-to-many (chosen).** The user chose "both" for one-to-many.

## Decision

**Users add columns by following foreign-key relationships from the base table. The SQL is generated from those paths, so each grid row is always one row of the base table.**

- **Many-to-one** (the base table, or a table already reached, has an FK to another table): its columns can be added as *lookup columns*, labelled like `Customer › name`. Paths can chain (`Customer › Country › name`). Each distinct path becomes one `LEFT JOIN` on the FK's columns, with a generated alias, so rows without a match keep NULLs instead of disappearing. Lookup columns are read-only. Clicking one opens the referenced row in its own data view.
- **One-to-many** (another table has an FK to the base table) comes in two ways:
  - a **summary column** (`count`, `sum`, `min`, `max`, `avg` of a column, or `list` of distinct values), generated as a correlated subquery on the FK, so rows never multiply;
  - **drill-down**: "Show ‹children›" on a row opens the child table in a new data view, filtered on the FK to that row.
- **Filters and sorts** may use lookup and summary columns. They become `WHERE`/`ORDER BY` on the alias or the subquery.
- **Editing an FK column** of the base table uses a picker of referenced rows, showing a display column (the first unique text column, else the key).
- **Tables with no FK between them** can be joined through "Join another table…", where the user picks the matching columns. Savoia suggests pairs by name and type. This is treated as a many-to-one lookup on the chosen columns, and the view warns if the match isn't unique.
- **Self-references and several FKs to the same table** are separate paths, named after the FK's columns (`parent_id → Category`).

## Consequences

- Rows never multiply, so base-table columns stay editable even with related columns shown.
- `LEFT JOIN` and correlated subqueries are standard in both engines. Summary columns on large child tables cost one subquery per row of the page, which is bounded by the page size (200).
- Arbitrary joins (non-equi, `FULL`, `CROSS`, unions) stay in the console. "Open in console" hands over the generated SQL as a starting point.
- Joins follow only FKs the catalog knows about. MySQL tables without declared FKs (common with MyISAM or app-managed relations) need "Join another table…".
