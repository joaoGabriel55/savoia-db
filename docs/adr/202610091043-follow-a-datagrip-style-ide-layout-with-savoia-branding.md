# Follow a DataGrip-style IDE layout with Savoia branding

## Status

Superseded by [Brand Savoia with Olivetti-style green fields and a red Run action](./202610091120-brand-savoia-with-olivetti-style-green-fields-and-a-red-run-action.md)

## Context

The user gave two references. Beekeeper Studio is clean and friendly, centered on a connection form with saved/recent lists and color-tagged connections. JetBrains DataGrip is a dense, keyboard-driven IDE: a Database Explorer tree with object counts, query consoles in tabs, a console toolbar (execute, cancel, transaction mode), a code editor with smart completion, and a result grid under the editor. The user wants the UI to draw on these but be its own.

## Decision

The main window follows the DataGrip IDE structure:

- **Title bar** shows the app and active connection. **Status bar** shows connection state and editor info.
- **Left: Database Explorer.** Toolbar (add source, refresh, disconnect, new console, DDL, dump, import) over a lazy tree: data source → database → schema → object groups (tables, views, functions, sequences…) with counts.
- **Center: query console tabs.** Each has a toolbar (run, cancel, history, `Tx:` mode, explain, current schema), the SQL editor, and a resizable result area with Output / Result N tabs and a virtualized grid with row numbers.

What makes it Savoia's own:

- **Amber accent** (`#E8B84A`, also in the app icon) for focus, caret, active-cell border and primary actions, on a neutral graphite palette.
- **Connection colors (from Beekeeper)** carry through the explorer node, console tab and status bar, so a production connection is recognizable everywhere.
- **Dump/import are first-class** buttons in the explorer toolbar, not buried in menus.
- **Connection setup** uses a Beekeeper-style focused form (dialog or sheet) rather than DataGrip's dense data-source settings.

Theme tokens live in `crates/savoia-app/src/theme.rs`. Views use theme colors, never hard-coded colors, except brand constants defined there.

## Consequences

- Power users get the familiar IDE layout. New users get a simple connection flow.
- The density of a DataGrip-style UI needs keyboard shortcuts and a command palette to stay usable. Plan them alongside each feature.
- A light theme must redefine the same tokens. Adding views that bypass the theme breaks it.
