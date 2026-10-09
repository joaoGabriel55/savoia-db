# Store connection secrets in a user-only file

## Status

Accepted

## Context

[Store connection secrets in the OS keychain](./202610091009-store-connection-secrets-in-os-keychain.md) put saved passwords in the OS credential store through the `keyring` crate, with one item per connection and secret (password, SSH password, SSH key passphrase).

In use on macOS (2026-10-09), that meant a Keychain access dialog over and over:

- Keychain access lists are bound to the app's code signature. Every `cargo build` produces a new ad-hoc-signed binary, which macOS treats as a different app, so "Always Allow" is lost on every rebuild.
- Each connection has up to three items, so connecting could ask up to three times.
- Released builds would be signed and so would prompt less, but would still prompt once per item and again after updates that change the signing identity.

The user asked for no keychain dialog at all. Options considered:

- **Keep the keychain and sign dev builds with a stable local certificate.** One approval per connection, then none. It keeps the dialog, though, and needs a certificate set up on every developer machine.
- **Keychain in release builds, file in debug builds.** No dialogs while developing, but two code paths, and users still get the dialogs.
- **Don't save passwords; ask on connect.** No stored secrets, but typing a password on every connect is what "Save passwords" exists to avoid.
- **A file only the user can read (chosen).** The same model as `~/.pgpass`, `~/.my.cnf` and `~/.ssh` keys, and DataGrip's "store passwords in a file" option. The user chose it.

## Decision

**Saved secrets live in `<data dir>/savoia-db/secrets.json`, a JSON object from connection id to that connection's secrets. On Unix the file is created with mode `0600` in a directory with mode `0700`.**

- Writes go to a temporary file in the same directory, which is then renamed over the old one, so a crash never leaves a half-written file.
- Reads and writes are serialized within the process.
- The keychain code and the `keyring` dependency are removed. Passwords saved in the keychain before this change are not migrated, because reading them would show the dialogs this ADR removes. Users enter them once more.
- Unticking "Save passwords" still keeps secrets in memory only, as before.

## Consequences

- No OS dialogs, on any platform, in dev or release. The Linux "no Secret Service" fallback question from the old ADR goes away.
- **Secrets are protected only by file permissions.** Any process running as the user, and any backup or sync of the data directory (Time Machine, iCloud or Dropbox sync of `~/Library/Application Support`), can read them in clear text. This is the same exposure `~/.pgpass` has.
- On Windows the file gets the default ACL of the user's profile directory, which is private to the user, but no extra restriction.
- Encrypting the file with a key kept somewhere else (keychain, master password) can be added later without changing the `SecretStore` trait.
