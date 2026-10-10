# Releasing

Releases are built by `.github/workflows/release.yml` when a `v*` tag is pushed. Why it works this way: see the [packaging ADR](./adr/202610100804-package-with-cargo-packager-and-update-from-github-releases.md).

## One-time setup

1. **Updater key.** Run `cargo install cargo-packager --locked`, then `cargo packager signer generate --path ~/.savoia/updater.key`. Back up the private key outside GitHub: losing it strands every installed copy on its version.
   - Repository **variable** `SAVOIA_UPDATER_PUBKEY`: the contents of `updater.key.pub`.
   - Repository **secrets** `CARGO_PACKAGER_SIGN_PRIVATE_KEY` (the contents of `updater.key`) and `CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD`.
2. **macOS signing and notarization** (optional; without it Gatekeeper warns on first launch). Set the secrets `APPLE_CERTIFICATE` (base64 `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`.
3. **Windows signing** is not wired up yet. It needs a code-signing certificate or Azure Trusted Signing, plus `signCommand` under `[package.metadata.packager.windows]`.
4. **GitHub Pages.** In Settings › Pages, set the source to *GitHub Actions* for the docs site.

## Each release

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`) and run `cargo check` to update `Cargo.lock`.
2. Move the *Unreleased* notes in `CHANGELOG.md` under the new version.
3. Merge to `main`, then tag: `git tag v0.1.0 && git push origin v0.1.0`.
4. Wait for the workflow. It fails if the tag doesn't match the crate version.
5. Review the draft release: every installer, the `.sig` files and `latest.json` should be there. Then publish it. Installed copies see the update only after it is published.

## Building an installer locally

```sh
cargo install cargo-packager --locked
cd crates/savoia-app
cargo packager --release --formats app,dmg   # or nsis,wix / deb,appimage
```

The packages land in `target/packages/`. Without `SAVOIA_UPDATER_PUBKEY` the build has no update key and never checks for updates.
