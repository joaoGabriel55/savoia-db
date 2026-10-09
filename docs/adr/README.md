# Architecture Decision Records

This folder is the project's decision log. Each file records one architectural decision: the context that led to it, the decision itself, and its consequences. Commits record *what* changed; ADRs record *why*.

## When to write one

Write an ADR when:

- future developers will need to understand the "why" behind what's being done;
- the decision is architecturally significant, cross-team, or hard to reverse.

Skip it when:

- the decision isn't architectural, or is tiny (minimal-risk, self-contained, single-developer);
- it's already fully covered by an existing standard, policy, or document;
- it's temporary: a workaround, a proof of concept, or an experiment.

An ADR is not a changelog and not a feature spec.

## How to add one

1. Copy `template.md` to `YYYYMMDDHHMM-<decision-slug>.md` (`date +%Y%m%d%H%M`).
2. Name the decision, not the topic: `use-postgresql-as-primary-database`, not `database`. If the name needs "and", split it into two ADRs.
3. Fill in Context, Decision, and Consequences. Start with status Proposed.
4. Add a row to the index below.

## Statuses

| Status | Meaning |
| --- | --- |
| **Proposed** | Under discussion, not yet agreed. |
| **Accepted** | Agreed and in effect. |
| **Rejected** | Considered and turned down. Kept so nobody proposes it again blindly. |
| **Deprecated** | No longer applies, with nothing replacing it. |
| **Superseded** | Replaced by a newer decision. Always links to the ADR that replaced it. |

## ADRs are immutable

Once written, an ADR is not edited. Only its status changes. When a decision changes, write a new ADR that explains the new context, and set the old one's status to `Superseded by [title](./file.md)`. Older code may still follow the earlier decision; the log explains why.

## Index

Regenerate this table from the files rather than editing it by hand:

> Read every ADR in this folder (except `README.md` and `template.md`) and regenerate the table below with the date, a link to each decision, and its current status. For superseded ADRs, link to the ADR that replaced them.

| Date | Decision | Status |
| --- | --- | --- |
| 2026-10-09 | [Record architecture decisions with ADRs](./202610091004-record-architecture-decisions-with-adrs.md) | Accepted |
| 2026-10-09 | [Use Tauri v2 with a web frontend for the desktop shell](./202610091006-use-tauri-v2-with-web-frontend-for-desktop-shell.md) | Superseded by [Use GPUI Kit for a native, GPU-rendered UI](./202610091042-use-gpui-kit-for-a-native-gpu-rendered-ui.md) |
| 2026-10-09 | [Use native dump tools when available, with a built-in Rust fallback](./202610091007-use-native-dump-tools-with-built-in-rust-fallback.md) | Accepted |
| 2026-10-09 | [Use native async drivers behind a `Driver` trait](./202610091008-use-native-async-drivers-behind-a-driver-trait.md) | Accepted |
| 2026-10-09 | [Store connection secrets in the OS keychain](./202610091009-store-connection-secrets-in-os-keychain.md) | Superseded by [Store connection secrets in a user-only file](./202610091454-store-connection-secrets-in-a-user-only-file.md) |
| 2026-10-09 | [Use Svelte 5 and TypeScript for the frontend](./202610091010-use-svelte-5-and-typescript-for-the-frontend.md) | Rejected |
| 2026-10-09 | [Use GPUI Kit for a native, GPU-rendered UI](./202610091042-use-gpui-kit-for-a-native-gpu-rendered-ui.md) | Accepted |
| 2026-10-09 | [Follow a DataGrip-style IDE layout with Savoia branding](./202610091043-follow-a-datagrip-style-ide-layout-with-savoia-branding.md) | Superseded by [Brand Savoia with Olivetti-style green fields and a red Run action](./202610091120-brand-savoia-with-olivetti-style-green-fields-and-a-red-run-action.md) |
| 2026-10-09 | [Verify SSH host keys against known_hosts, with explicit trust](./202610091108-verify-ssh-host-keys-against-known-hosts-with-explicit-trust.md) | Accepted |
| 2026-10-09 | [Brand Savoia with Olivetti-style green fields and a red Run action](./202610091120-brand-savoia-with-olivetti-style-green-fields-and-a-red-run-action.md) | Superseded by [Use Savoy blue, not red, as the brand accent](./202610091125-use-savoy-blue-not-red-as-the-brand-accent.md) |
| 2026-10-09 | [Use Savoy blue, not red, as the brand accent](./202610091125-use-savoy-blue-not-red-as-the-brand-accent.md) | Accepted |
| 2026-10-09 | [Stream query results as server-rendered text pages](./202610091303-stream-query-results-as-server-rendered-text-pages.md) | Accepted |
| 2026-10-09 | [Serialize all work on one connection per session](./202610091309-serialize-all-work-on-one-connection-per-session.md) | Accepted |
| 2026-10-09 | [Load schema objects and table details on demand](./202610091437-load-schema-objects-and-table-details-on-demand.md) | Proposed |
| 2026-10-09 | [Draw ER diagrams natively with a built-in layered layout](./202610091437-draw-er-diagrams-natively-with-a-built-in-layered-layout.md) | Proposed |
| 2026-10-09 | [Store connection secrets in a user-only file](./202610091454-store-connection-secrets-in-a-user-only-file.md) | Accepted |
