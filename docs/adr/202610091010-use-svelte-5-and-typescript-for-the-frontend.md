# Use Svelte 5 and TypeScript for the frontend

## Status

Rejected

## Context

Under [Tauri v2](./202610091006-use-tauri-v2-with-web-frontend-for-desktop-shell.md) the UI is a web app. "Lightweight" is a product goal. The UI is mostly a grid, an editor, a tree and forms, and needs fine-grained updates while results stream in.

- **React + TypeScript.** Largest ecosystem (TanStack Table/Virtual, shadcn/ui, Monaco/CodeMirror wrappers). Heavier runtime, and re-render tuning is needed for streaming grids.
- **Svelte 5 + TypeScript.** Runes give fine-grained reactivity, the runtime is small, and the components need less boilerplate. TanStack Virtual and CodeMirror 6 are framework-agnostic and work well with it. Its ecosystem is smaller than React's.
- **SolidJS.** Similar performance profile to Svelte, but a smaller ecosystem than either.

## Decision

Svelte 5 (runes) + TypeScript + Vite, with Tailwind CSS for tokens and themes. Use CodeMirror 6 (`@codemirror/lang-sql`) for the editor and TanStack Virtual for the grid.

## Consequences

- Smaller bundle and simpler reactive code for streaming results.
- Fewer ready-made component kits than React, so some primitives (menus, dialogs) come from bits-ui/shadcn-svelte or are built in-house.
- If contributors strongly prefer React, revisit this before the frontend scaffold lands. Switching later is expensive.
