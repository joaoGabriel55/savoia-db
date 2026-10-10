# Package with cargo-packager and update from GitHub Releases

## Status

Accepted

## Context

Backlog task 5.4 asks for dmg, msi/nsis, AppImage/deb/rpm installers, macOS notarization, Windows signing and auto-update. The project started on Tauri, whose bundler and updater would have covered this, but [GPUI Kit replaced it](./202610091042-use-gpui-kit-for-a-native-gpu-rendered-ui.md), so Savoia is a plain Rust binary and needs its own packaging story.

Constraints, as of October 2026:

- One maintainer, no release infrastructure beyond GitHub Actions.
- PLAN.md budgets the installer at under 15 MB. A release build with default settings gave a 15.6 MB dmg on Apple silicon.
- Updates must be verifiable without trusting the download host alone.

## Decision

Package with **cargo-packager 0.11** and update with **cargo-packager-updater 0.2**, from the Tauri team but with no Tauri runtime.

- The config lives in `[package.metadata.packager]` in `crates/savoia-app/Cargo.toml`. Formats: `app` + `dmg` (macOS), `nsis` + `wix` (Windows), `deb` + `appimage` (Linux). cargo-packager has no RPM output, so `cargo-generate-rpm` builds it from `[package.metadata.generate-rpm]`.
- `.github/workflows/release.yml` builds every format on a `v*` tag, signs each package with a minisign key, writes `latest.json` with `scripts/release-manifest.py`, and opens a draft GitHub Release.
- The app reads `releases/latest/download/latest.json`. The minisign public key is compiled in from `SAVOIA_UPDATER_PUBKEY`. A build without it, which includes every dev build, never checks. Installing waits for the user's click, then restarts.
- The release profile uses fat LTO, one codegen unit and stripped debuginfo. The dmg dropped to 12.9 MB, and symbol names stay for crash backtraces.

Alternatives considered:

- **cargo-dist.** Strong at GitHub Releases and shell/PowerShell installers, but weak on dmg and msi GUI installers, and it has no in-app updater.
- **Per-platform tools** (cargo-bundle, cargo-wix, cargo-deb) plus a hand-rolled updater. This gives the most control, but means three configs and our own signature checking.

## Consequences

- One config and one workflow produce every installer. Updates are signature-checked against a key we hold.
- Code signing needs secrets the repository doesn't have yet. macOS signing and notarization run once the `APPLE_*` secrets are set. Windows signing needs a certificate (or Azure Trusted Signing) and a `signCommand`. Until then Windows SmartScreen and macOS Gatekeeper warn on first launch.
- The updater pulls in `reqwest` (blocking, rustls), which adds to the binary. The 15 MB budget held after the release-profile change.
- Losing the minisign private key strands installed copies on their version. Back it up outside GitHub.
