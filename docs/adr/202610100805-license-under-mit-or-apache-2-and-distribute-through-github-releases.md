# License under MIT or Apache-2.0 and distribute through GitHub Releases

## Status

Accepted

## Context

Backlog task 5.5 (v0.1 release) was blocked on choosing a license and a distribution channel. `Cargo.toml` already declared `MIT OR Apache-2.0`, but the repository had no license files, so the code was not actually licensed to anyone.

Savoia is a free tool funded by donations (Ko-fi) with no paid tier planned. Its dependencies (GPUI Kit, tokio-postgres, mysql_async, russh) are MIT and/or Apache-2.0.

## Decision

- License Savoia Studio under **MIT OR Apache-2.0**, the Rust ecosystem's usual dual license, with `LICENSE-MIT` and `LICENSE-APACHE` at the repository root.
- Distribute installers through **GitHub Releases** only. The [updater](./202610100804-package-with-cargo-packager-and-update-from-github-releases.md) reads the same releases.

Alternatives considered:

- **GPL-3.0.** This would keep forks open, but it complicates reuse of the crates (`savoia-core`, the drivers) as libraries. Copyleft wasn't asked for.
- **Package managers at launch** (Homebrew cask, winget, Flathub). Each is another manifest to keep current on every release. They can be added later, pointing at the same release assets.

## Consequences

- Anyone may use, fork and embed the code under either license.
- Users find Savoia through the GitHub page and docs site, not through their package manager, until someone adds those manifests.
