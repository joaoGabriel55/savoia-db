# Use native dump tools when available, with a built-in Rust fallback

## Status

Accepted

## Context

Dumping and importing databases is a core feature, not an add-on. Users expect output they can restore with standard tools (`psql`, `pg_restore`, `mysql`).

As of October 2026, no mature pure-Rust crate replaces `pg_dump` or `mysqldump`:

- `libpgdump` reads and writes the pg custom archive format but does not export a live database.
- `rust-mysqldump` is a small CLI with unclear fidelity.
- `pg_dbmigrator` and `libruster` just shell out to or FFI into the official tools.

A hand-written exporter is easy for tables and data (Postgres `COPY ... TO STDOUT`, MySQL streaming `SELECT`). Getting full fidelity (sequences, extensions, triggers, grants, routines, dependency ordering) is a long tail, and the official tools already handle it.

On the other hand, many users won't have the client tools installed, or will have the wrong major version: `pg_dump` must be at least as new as the server.

## Decision

Use a hybrid strategy behind one `DumpEngine` interface:

1. **External engine (preferred).** Detect `pg_dump`/`pg_restore`/`psql` and `mysqldump`/`mysql` on PATH or at a path the user configures. Check the version against the server. Run them as child processes with credentials passed through env/temp option files, never on argv. Stream progress and stderr to the UI, and support cancellation.
2. **Built-in engine (fallback).** A Rust exporter that writes plain SQL (DDL from catalog queries plus data) and CSV/JSON per table. It is clearly labelled in the UI as lower fidelity.

Imports run `.sql` files through the same split: external CLI when available, otherwise a built-in statement splitter and executor that stops or continues on error, as the user chooses.

Alternatives rejected:

- **Native only.** Simpler to install, but users would get incomplete dumps that silently miss objects.
- **External only.** Full fidelity, but the feature doesn't work out of the box.

## Consequences

- The app works out of the box and gives best-in-class output when the tools exist.
- Two code paths to test. Integration tests need dockerized PG/MySQL plus the CLIs.
- Need a tools-detection/settings screen and clear messaging on which engine ran.
- Bundling the official binaries per OS remains an open follow-up decision (licensing allows it, but it adds size and per-platform packaging).
