# Product

<!-- impeccable:product-schema 1 -->

## Platform

desktop

## Users

Backend developers who work in PostgreSQL and MySQL every day. They write and run queries, inspect schemas, and fix data, often switching between local, staging and production connections in the same session. They are keyboard-fluent and used to IDEs.

## Product Purpose

Savoia Studio is a desktop database client for PostgreSQL and MySQL (MariaDB later). It covers the daily loop of connecting, exploring a schema, querying, browsing and editing table data, plus dumping and importing databases. Success means a developer keeps it open all day instead of a heavier IDE or a thinner query tool, because it starts instantly, stays out of the way and never puts production data at risk.

## Positioning

Native and lightweight. Savoia is 100% Rust with a GPU-rendered UI: no JVM, no webview, no Electron. Its performance budget is a cold start under 1 s, idle memory under 150 MB, an installer under 15 MB, and 60 fps scrolling through 1M-row results. DataGrip, DBeaver and Beekeeper cannot make that claim with their current stacks.

These are budgets from `docs/PLAN.md`, not measured results yet. Don't present them as benchmarks until they are measured.

## Operating Context

- A native cross-platform desktop app for macOS, Windows and Linux. It uses one consistent Savoia design language on every OS rather than per-OS native chrome. The UI is built on GPUI Kit (`gpui-kit` 0.7.1), so design work means Rust views and theme tokens, not HTML/CSS.
- The main window follows a DataGrip-style IDE layout: title bar, Database Explorer tree on the left, query console tabs with a SQL editor over a result grid in the center, and a status bar. Connection setup is a focused, Beekeeper-style form. See `docs/adr/202610091043-follow-a-datagrip-style-ide-layout-with-savoia-branding.md`.
- Users connect directly, over TLS, or through SSH tunnels. Several connections may be open at once, and some of them are production.

## Capabilities and Constraints

- **Shipped (M0–M1):** the IDE shell, a connection manager (saved/recent, color tags, URL import, test/connect), SSL/TLS modes, SSH tunnels with known_hosts trust, read-only sessions, a catalog explorer, and secrets in the OS keychain.
- **v1 scope** (`docs/PLAN.md`): schema explorer with DDL, SQL editor with schema-aware completion and history, a streamed virtualized result grid, inline table edits (pending changes → SQL preview → commit), dump/import, light and dark themes, keyboard-first use and a command palette.
- **Later:** ER diagram, table designer, schema diff, saved queries, more engines, AI assistant, query plan visualizer. New engines or features need an ADR or roadmap entry.
- **Constraints:** the UI never blocks; colors come only from `crates/savoia-app/src/theme.rs` tokens; Lucide icons beyond the default set must be registered in `crates/savoia-app/src/assets.rs`; only `savoia-app` depends on `gpui-kit`.
- **Undecided:** packaging/updater, distribution channel and license (see `docs/PLAN.md` §6).

## Brand Commitments

- Name: **Savoia Studio** (formerly Savoia DB; `docs/adr/202610091612-name-the-product-savoia-studio.md`).
- Italian identity rooted in Savoy Piedmont: Ivrea green is the primary color, Savoy blue (azzurro Savoia) the accent, and status colors never reuse either brand hue (`docs/adr/202610091125-use-savoy-blue-not-red-as-the-brand-accent.md`). The previous amber look is archived in `docs/design/amber-baseline.md`.
- Connection colors carry through the explorer, console tab and status bar, so a production connection is recognizable everywhere (accepted ADR).
- Dump and import are first-class actions, not buried in menus (accepted ADR).

## Evidence on Hand

- App icon: `assets/app-icon.svg`.
- Roadmap and performance budget: `docs/PLAN.md`. Decision log: `docs/adr/`.
- No users, testimonials, measured benchmarks, screenshots, pricing or release yet. Future work must not fabricate any of these.

## Product Principles

1. **Speed is the feature.** Anything that costs startup time, memory or frame rate has to justify itself against the native-and-lightweight claim.
2. **Dense, but never in the way.** The UI offers IDE-grade information density and runs keyboard-first. Rarely used options go to dialogs and the command palette, not permanent chrome.
3. **Production is always obvious.** Which connection you are on, and whether it is read-only, must be visible wherever you can change data.
4. **Nothing destructive happens silently.** Edits, imports and restores show what will run before they run, and can be cancelled.
