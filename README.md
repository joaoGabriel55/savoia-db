# Savoia DB

A lightweight but feature-rich desktop database client for PostgreSQL and MySQL. 100% Rust, with a native GPU-rendered UI built on [GPUI Kit](https://gpui-kit.com). No webview.

See [docs/PLAN.md](docs/PLAN.md) for the roadmap and [docs/adr/](docs/adr/README.md) for architecture decisions.

## Layout

| Path | What |
| --- | --- |
| `crates/savoia-app` | The app binary (`savoia-db`): GPUI views, theme, icon assets |
| `crates/savoia-core` | Domain types, connection config + URL import, driver traits, errors; no UI dependency |
| `crates/savoia-pg` | PostgreSQL driver (tokio-postgres, rustls) |
| `crates/savoia-mysql` | MySQL/MariaDB driver (mysql_async, rustls) |
| `crates/savoia-store` | Saved connections (SQLite) and secrets (a user-only `0600` file) |
| `crates/savoia-tunnel` | SSH tunnels (russh) with known_hosts verification |

## Prerequisites

- Rust stable (`rustup`)
- Linux only: `libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libfontconfig-dev libx11-xcb-dev libssl-dev`
- Docker, for the test databases

## Commands

```sh
cargo run -p savoia-app            # run the app
cargo build -p savoia-app --release

cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

docker compose up -d --wait        # Postgres 17 :54317 (TLS on), MySQL 8.4 :33084, SSH bastion :2222 — all savoia/savoia
```

### Live tests

Driver, tunnel and UI integration tests talk to the compose services. Without these variables they **return early and report as passed**, so set them when you want real coverage:

```sh
SAVOIA_PG_URL=postgres://savoia:savoia@127.0.0.1:54317/savoia \
SAVOIA_MYSQL_URL=mysql://savoia:savoia@127.0.0.1:33084/savoia \
SAVOIA_SSH_TEST=1 cargo test --workspace
```

`cargo test -p savoia-store -- --ignored` also runs a round trip through your real OS keychain.

## UI conventions

- Colors come from `crates/savoia-app/src/theme.rs`; don't hard-code colors in views.
- Lucide icons beyond the default component set must be added to `crates/savoia-app/src/assets.rs`, otherwise they render blank.
- Only `savoia-app` depends on `gpui-kit`. Keep database, dump and storage logic in UI-free crates.
