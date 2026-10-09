# Verify SSH host keys against known_hosts, with explicit trust

## Status

Accepted

## Context

Connections can go through an SSH bastion (`savoia-tunnel`, built on `russh` 0.64). Database passwords and query data then travel inside that SSH session. russh leaves host-key checking to the application, and its own examples accept any key, which allows a man-in-the-middle attack on the bastion.

Users already trust hosts through OpenSSH's `~/.ssh/known_hosts`. DataGrip and Beekeeper both prompt for an unknown host and refuse a changed key.

## Decision

- Check every SSH server key against the user's `~/.ssh/known_hosts`, shared with OpenSSH, so hosts trusted in a terminal are trusted here and the reverse.
- **Known key:** connect.
- **Unknown host:** fail with `AppError::UnknownHostKey { host, fingerprint }`. The UI shows the SHA256 fingerprint and asks the user to trust it. Only on confirmation does it reconnect with `HostKeyPolicy::TrustUnknown`, which records the key in known_hosts.
- **Changed key:** always fail, with a man-in-the-middle warning. The app never offers to overwrite it; the user must fix known_hosts deliberately.
- Certificate host keys are checked by their public key like plain keys. Trusting a host CA (`@cert-authority`) is not supported yet.
- Tests use a temporary known_hosts file (`KnownHosts::File`) and never touch the user's.

Alternatives rejected:

- **Accept any key** (the russh example). Insecure.
- **A separate app-private known_hosts.** Users would have to trust every bastion twice, and a key changed in OpenSSH wouldn't be noticed here.

## Consequences

- The first connection through a new bastion needs one extra confirmation.
- The app writes to `~/.ssh/known_hosts` when the user trusts a host. That's the same file `ssh` uses, which users expect.
- Follow-up: support `@cert-authority` lines and hashed-hostname edge cases if users need them.
