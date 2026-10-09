# Store connection secrets in the OS keychain

## Status

Superseded by [Store connection secrets in a user-only file](./202610091454-store-connection-secrets-in-a-user-only-file.md)

## Context

Saved connections include passwords, SSH key passphrases and TLS client keys. Beekeeper-style clients offer "Save passwords". Storing them in plain JSON or SQLite in the app data directory exposes them to any process or backup that can read the user's files.

## Decision

Store connection metadata (name, color, host, port, user, options) in a local SQLite file in the app data directory. Store secrets only in the OS credential store via the `keyring` crate (macOS Keychain, Windows Credential Manager, Secret Service on Linux), keyed by connection id. If the user unticks "Save passwords", prompt on connect and keep the secret in memory only. Secrets never cross IPC back to the frontend once saved.

Alternatives rejected:

- **Encrypted file with a master password.** Adds UX friction and requires key management.
- **Plain file.** Insecure.

## Consequences

- Secure by default with no master password.
- Linux without a Secret Service daemon needs a fallback: a prompt each time, or an opt-in encrypted file. That is a follow-up decision.
- Export/import of connections excludes secrets.
