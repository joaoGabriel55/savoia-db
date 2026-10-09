# Use Tauri v2 with a web frontend for the desktop shell

## Status

Superseded by [Use GPUI Kit for a native, GPU-rendered UI](./202610091042-use-gpui-kit-for-a-native-gpu-rendered-ui.md)

## Context

Savoia DB is a lightweight desktop database client that should still be feature-rich. It targets MySQL and PostgreSQL first, and the UI takes Beekeeper Studio as its reference. The app has to run on macOS, Windows and Linux, and the user chose Rust for the core.

The hardest UI parts are a virtualized result grid that can scroll through hundreds of thousands of rows, a SQL editor with highlighting and autocomplete, and many forms and dialogs (connection form, dump/import wizards). These need mature components.

Options evaluated (October 2026):

- **Tauri v2 + web UI.** Rust backend, OS webview for the frontend. Mature libraries exist for grids (TanStack Virtual/Table) and editors (CodeMirror 6). Bundles are a few MB. Several current Rust DB clients use it (Tabularis, QoreDB, IceQL). The known cost is rendering differences between webviews, especially WebKitGTK on Linux.
- **GPUI + gpui-component.** Pure Rust, GPU-rendered, with a virtualized DataTable and editor. Fastest option, but pre-1.0, few examples, and Windows/Linux support is less proven. dbflux is the one DB client built on it.
- **Iced.** Pure Rust, Elm architecture, stable (COSMIC, Kraken). The grid and SQL editor would have to be built from scratch.
- **egui / Slint.** Rejected early. egui's immediate mode suits complex stateful apps poorly, and Slint is aimed at embedded/declarative UIs and has few data-heavy widgets.
- **Electron.** Rejected because it contradicts the "lightweight" goal and the Rust requirement.

## Decision

Build the app with **Tauri v2**. All database work (drivers, pooling, query execution, streaming, dumps/imports, SSH tunnels, secrets) lives in Rust. The web frontend only renders and sends typed commands. Data moves over Tauri IPC (commands + channels), and large results are streamed in pages rather than sent whole.

## Consequences

- Fast UI iteration with off-the-shelf grid and editor components. Theming like Beekeeper (dark, compact) is simple to do in CSS.
- A second language (TypeScript) and toolchain (Node/pnpm) in the repo.
- Every UI change must be checked on WebKit (macOS), WebView2 (Windows) and WebKitGTK (Linux). CI should build all three.
- The IPC boundary becomes a contract. Commands and payload types should be generated from Rust (e.g. `specta`/`tauri-specta`) so they stay in sync.
- Follow-up decisions: frontend framework, IPC typing approach, grid/editor libraries.
