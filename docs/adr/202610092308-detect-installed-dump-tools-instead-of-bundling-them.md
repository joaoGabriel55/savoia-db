# Detect installed dump tools instead of bundling them

## Status

Accepted

## Context

[Use native dump tools when available, with a built-in Rust fallback](./202610091007-use-native-dump-tools-with-built-in-rust-fallback.md) left one question open: ship `pg_dump`/`pg_restore`/`psql` and `mysqldump`/`mysql` inside the installers, or only use copies the user already has. Backlog task 4.1 (tool detection) cannot start until this is settled.

What weighs on it, as of October 2026:

- PLAN.md budgets the installer at under 15 MB. The Postgres client tools plus `libpq`, OpenSSL and their other shared libraries take roughly 10 MB per OS; the MySQL client adds more. Bundling both breaks the budget on its own.
- `pg_dump` must be at least as new as the server. A bundled copy goes stale with every Postgres major release, so we would also take on shipping a new Savoia build each time.
- Each bundled binary needs per-platform packaging, macOS notarization of the extra Mach-O files and Windows signing (task 5.4, not started).
- Our users are developers. Most already have the clients from Homebrew (`libpq`, `mysql-client`), Postgres.app, distro packages or the EDB/Oracle Windows installers. These often sit off `PATH`: Homebrew's `libpq` is keg-only, and macOS apps launched from Finder get a minimal `PATH`.
- The built-in Rust exporter (task 4.3) already covers users with no tools at all.

## Decision

Savoia Studio does not bundle the dump tools. The `savoia-transfer` crate detects them:

1. A directory the user configures, if any. A tool found there wins.
2. Otherwise every copy found on `PATH` and in well-known install directories per OS (Homebrew `libpq`/`mysql-client`/`postgresql@*`, Postgres.app, `/usr/lib/postgresql/*/bin`, `/usr/pgsql-*/bin`, `C:\Program Files\PostgreSQL\*\bin`, `C:\Program Files\MySQL\MySQL Server *\bin`). The newest version wins.
3. Each candidate's `--version` output is parsed and checked against the server: a `pg_dump` older than the server is incompatible; other mismatches (older `psql`/`pg_restore`/MySQL clients, MariaDB tools against MySQL) are warnings.

When no usable tool is found, the UI falls back to the built-in engine and says so.

Alternatives considered:

- **Bundle the tools per OS.** Works out of the box at full fidelity, but breaks the installer budget, goes stale with each server release and adds signing work.
- **Detect, plus an optional one-click download** of the official binaries into the app data directory. Best experience for users without tools, but it needs a downloader, checksum verification and a decision on where binaries are hosted. Can be added later on top of detection.

## Consequences

- Installers stay small and we never ship an outdated `pg_dump`.
- Users without the tools get the lower-fidelity built-in engine until they install them; the UI must say how (e.g. `brew install libpq`).
- Detection has to know install layouts per OS and keep that list current.
- The "custom tools directory" setting needs a home: the dump wizard (4.5) for now, the settings screen (5.2) later.
- The optional download remains a possible follow-up, which would need its own ADR.
