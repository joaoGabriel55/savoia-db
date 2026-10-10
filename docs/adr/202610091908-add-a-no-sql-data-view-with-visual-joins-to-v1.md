# Add a no-SQL data view with visual joins to v1

## Status

Accepted

## Context

[PLAN.md](../PLAN.md) puts a "table data view" in v1: paginated browsing and inline edits, with pending changes shown as SQL before a commit (M3). Everything beyond that runs through the SQL console. Its risk table also says to hold the v1 list against scope creep: "New engines and features need an ADR or roadmap entry."

On 2026-10-09 the user asked for more than M3 describes: a view where users create, edit, delete **and query** table data without writing SQL, including joins, "with a UX that makes joins transparent". The users in [PRODUCT.md](../../PRODUCT.md) are backend developers who know SQL. Even so, the everyday loop of "look at these orders with their customer's name, filter by country, fix one row" costs a hand-written join in every tool we compete with, except the spreadsheet-like ones (Airtable, NocoDB, Metabase's notebook editor). Those keep each row "one record" and pull related fields in through relationships.

Options considered:

- **Keep M3 as planned; queries stay in the console.** It is the smallest scope, but it leaves out what the user asked for.
- **A general visual query builder** (tables on a canvas, drag lines between columns, pick join types). It is powerful, but it puts join mechanics in front of the user, which is the opposite of the request. It is also the largest UI to build on GPUI.
- **Extend the M3 table view into a no-SQL data view with relationship columns, filters and summaries (chosen).** It is the M3 view with more column sources. The user approved this plan on 2026-10-09.

## Decision

**The table data view becomes Savoia's no-SQL way to query and edit data, and it replaces the M3 "table browser" item.** A data view is a tab opened from a table ("Open data"). It holds:

- a base table, browsed in server-side pages;
- columns from the base table, or from related tables reached through foreign keys (see [Build joins from foreign-key relationship paths](./202610091908-build-joins-from-foreign-key-relationship-paths.md));
- filters and a sort, which can use any of those columns;
- optionally a summary (group by plus aggregates), which makes the view read-only;
- pending edits to base-table rows (see [Write data edits as generated SQL in one previewed transaction](./202610091908-write-data-edits-as-generated-sql-in-one-previewed-transaction.md)).

The view always produces one SQL statement, which the user can show or open in a console. It never hides what runs, which keeps principle 4 of PRODUCT.md ("Nothing destructive happens silently").

The query and edit models, and their SQL generation, live in `savoia-core` with no UI dependency. The view lives in `savoia-app`.

## Consequences

- v1 grows by roughly two weeks over M3 alone. M3 in PLAN.md and BACKLOG.md is rewritten around the data view.
- Every page load is a short query on the session's one connection ([Serialize all work on one connection per session](./202610091309-serialize-all-work-on-one-connection-per-session.md)), so a data view waits while a console query runs, and shows that.
- Data views open only in the database the session is connected to. Tables of other Postgres databases, browsed through catalog-only connections ([Open a catalog connection per other Postgres database](./202610091856-open-a-catalog-connection-per-other-postgres-database.md)), can't be opened as data views yet.
- Saved views (named, reopened later) are not part of this decision. They would need storage and an ADR of their own.
