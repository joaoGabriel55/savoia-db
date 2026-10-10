# Report crashes as user-reviewed GitHub issues

## Status

Proposed

## Context

Backlog task 5.5 asks for opt-in crash reporting. A database client handles credentials, schema names and row data, and panic messages can contain any of them. PRODUCT.md promises a light, local tool, and there is no backend to receive reports.

## Decision

- A panic hook writes a plain-text report (version, OS, panic message and location, backtrace) to `<data dir>/crashes/`.
- Settings › General › Privacy has **Offer to report crashes**, off by default. When it is on, the next start shows a notification whose button opens a prefilled GitHub "new issue" page. The user reads the report there and submits it, or doesn't.
- Nothing is sent automatically. When the option is off, only the last five reports are kept on disk.

Alternatives considered:

- **Sentry or a similar service.** It would capture more crashes, but every report leaves the machine on its own, and it adds a vendor, a DSN in the binary and a privacy policy.
- **No reporting.** Crashes would surface only when users happen to file issues, without backtraces.

## Consequences

- No data leaves the machine without the user reading it first, and there is no service to run.
- We see fewer reports than with automatic upload, and only from users with a GitHub account.
- Native crashes that abort without a Rust panic (GPU driver faults, for example) are not captured.
