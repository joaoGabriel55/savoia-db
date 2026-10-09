# Use GPUI Kit for a native, GPU-rendered UI

## Status

Accepted

## Context

[Use Tauri v2 with a web frontend](./202610091006-use-tauri-v2-with-web-frontend-for-desktop-shell.md) was accepted earlier the same day and a Tauri + Svelte scaffold was built. The user then ruled out a browser/webview-based UI and asked to use [Rust/UI](https://rust-ui.com/), with JetBrains DataGrip as UI inspiration.

Findings (October 2026):

- **Rust/UI** is a Tailwind/HTML component registry. Its homepage says it targets Dioxus, but its install docs say "Currently, we support Leptos" with Dioxus planned. On desktop it renders HTML/CSS in the OS webview (wry, the same engine as Tauri). So it does not meet the "no webview" requirement.
- **Dioxus Native (Blitz)** renders HTML/CSS without a webview, but its README calls it pre-alpha and "would not yet recommend building apps with it". Tailwind support on it is unverified.
- **GPUI Kit** (`gpui-kit` 0.7.1, Apache-2.0, by Longbridge on top of Zed's GPUI) renders natively on the GPU. It ships what a DataGrip-style IDE needs: resizable/dock layouts, a virtualized tree, a virtualized `DataTable` with fixed and resizable columns and sorting, tabs, a code editor with tree-sitter SQL highlighting and LSP-style completion hooks, title bar, status bar, a themeable color system and Lucide icons.

The user chose GPUI Kit over Dioxus desktop + Rust/UI and over Blitz.

## Decision

Build the UI natively with **`gpui-kit`**, as the single UI dependency (it re-exports GPUI, `gpui-base` and `gpui-component`). The app is a plain Rust binary crate, `crates/savoia-app`. There is no webview, no JS/TS toolchain and no IPC layer. UI views call into the core crates directly, and long-running database work runs on background executors and updates views through GPUI entities.

This supersedes the Tauri ADR. The Svelte frontend ADR is rejected along with it.

## Consequences

- One language and one toolchain. The UI and core share types directly, with no binding generation.
- Native performance, low memory use, and no differences between platform webviews.
- GPUI is pre-1.0 and its API moves. Pin `gpui-kit` exactly and upgrade deliberately.
- The ecosystem is smaller than the web's. Missing widgets (e.g. a schema-aware completion provider, ER diagrams) must be built on GPUI primitives.
- Icons beyond the default component set must be registered in `crates/savoia-app/src/assets.rs` or they render blank.
- Linux builds need the system libraries GPUI requires (xkbcommon, wayland/x11, vulkan/fontconfig). CI installs them.
- References to IPC in earlier Proposed ADRs (secrets "never cross IPC") now mean "never handed to UI views except to a password field the user is editing".
- Packaging (app bundles, signing, auto-update) no longer comes from Tauri. It needs its own decision (e.g. `cargo-packager`).
