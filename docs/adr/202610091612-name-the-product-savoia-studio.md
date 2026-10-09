# Name the product Savoia Studio

## Status

Accepted

## Context

The app shipped through M1 as "Savoia DB", with "final naming" listed as an open decision in `docs/PLAN.md` §6. By M2 the product covers more than a database connection: it has a query console, result grids, and table actions, and dump/import and an ER diagram are planned. The owner asked (2026-10-09) to rename the app and the GitHub repository to `savoia-studio`.

One identifier already carries the old name on users' machines: the app data directory `<data dir>/savoia-db/`. It holds `savoia.sqlite` with saved connections and `secrets.json` with saved passwords (see [Store connection secrets in a user-only file](./202610091454-store-connection-secrets-in-a-user-only-file.md)).

## Decision

The product is named **Savoia Studio**. Everything a user or server sees uses the new name: the window title, the binary (`savoia-studio`), the Postgres `application_name`, crate descriptions, docs, and the repository (`joaoGabriel55/savoia-studio`).

The data directory keeps its old name, `savoia-db`. It is internal, and changing it would silently orphan every saved connection and password.

Alternatives considered:

- **Rename the data directory too, with a one-time migration.** Cleaner on disk, but it adds move-on-startup code (and its partial-failure cases) before there are any public users to migrate. It can still be done later if packaging needs a bundle-id-aligned path.
- **Keep "Savoia DB"**. Rejected by the owner: "DB" undersells a studio-style IDE.

## Consequences

- Postgres sessions now show `application_name = 'Savoia Studio'`; anything filtering `pg_stat_activity` on the old name must change.
- The data directory name no longer matches the product name. Packaging (M5) should decide whether to migrate them alongside the bundle identifier.
- Older ADRs keep saying "Savoia DB"; they are immutable and describe the product as it was named then.
