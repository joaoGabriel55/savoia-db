# Version releases with Changesets

## Status

Accepted

## Context

Savoia Studio 0.1.0 was released on 2026-10-10 by hand: bump `[workspace.package] version` in `Cargo.toml`, refresh `Cargo.lock`, move the notes in `CHANGELOG.md`, merge, then push a `v*` tag that starts [the release workflow](./202610100804-package-with-cargo-packager-and-update-from-github-releases.md). Each of these steps can be forgotten or done inconsistently. The release workflow already refuses a tag that doesn't match the crate version, which catches one failure but none of the others.

The user asked to automate versioning with [Changesets](https://github.com/changesets/changesets). Changesets is a Node tool built for npm packages. It knows nothing about Cargo, so it needs an npm package to version.

## Decision

Version Savoia Studio with **Changesets** (`@changesets/cli` 3.0.3, `changesets/action` v2.1.2), using a private root `package.json` as the single source of the version:

- A PR that users will notice adds a changeset (`npx changeset`): a short Markdown file with the bump type (patch, minor or major) and a line for the changelog. CI warns, but doesn't fail, when a PR has none.
- On every push to `main`, `.github/workflows/changesets.yml` keeps a "Release Savoia Studio" PR up to date. That PR applies the pending changesets: `changeset version` bumps `package.json` and writes `CHANGELOG.md` (with PR links, through `@changesets/changelog-github`), and `scripts/sync-version.mjs` copies the version into `Cargo.toml` and `Cargo.lock`.
- When that PR is merged and nothing is pending, the same workflow tags `vX.Y.Z` (`scripts/tag-release.mjs`) and dispatches `release.yml` on the tag. A tag pushed with `GITHUB_TOKEN` doesn't start workflows, but `workflow_dispatch` is the documented exception, so no personal token is needed. The release workflow is unchanged otherwise: it builds, signs and drafts the GitHub Release, now with only that version's changelog section as its notes.
- Publishing the draft stays manual. It is the last check before installed copies update themselves.

Alternatives considered:

- **release-plz.** Cargo-native: it reads conventional commits or changelogs and bumps `Cargo.toml` with no Node involved. It wasn't the tool requested, and it centers on publishing crates to crates.io, which Savoia doesn't do.
- **cargo-release by hand.** This automates the bump and tag, but someone still has to decide the version and write the notes at release time, instead of in each PR.
- **Version in `Cargo.toml` only, Changesets reading it.** Not possible: Changesets only versions `package.json` files.

## Consequences

- Release notes are written when the change is made, by the person who made it, and the version bump follows from them.
- The repository gains a Node toolchain (`package.json`, `package-lock.json`, `.changeset/`) used only for releases. Building the app still needs only Cargo.
- The version lives in two files, `package.json` and `Cargo.toml`. Only the version PR may change it; `sync-version.mjs` keeps them equal, and the release workflow still checks the tag against the crate.
- The repository must allow GitHub Actions to create pull requests. PRs opened with `GITHUB_TOKEN` don't trigger other workflows, so CI doesn't run on the version PR. It only changes versions and the changelog.
