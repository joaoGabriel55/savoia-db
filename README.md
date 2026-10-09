# Savoia Studio

A lightweight but feature-rich desktop database client for PostgreSQL and MySQL. 100% Rust, with a native GPU-rendered UI built on [GPUI Kit](https://gpui-kit.com). No webview.

See [docs/PLAN.md](docs/PLAN.md) for the roadmap and [docs/adr/](docs/adr/README.md) for architecture decisions.

## Layout

| Path | What |
| --- | --- |
| `crates/savoia-app` | The app binary (`savoia-studio`): GPUI views, theme, icon assets |
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


### Sample data

`samples/` has a Serie A 2025-26 database for trying the explorer, ER diagram and console by hand. Clubs and stadiums are real (figures approximate); every person is invented. It has 20 clubs, 490 players, 380 fixtures (20 matchdays played, so later scores are `NULL`), views, a function, a procedure, and 150,000 `match_events` rows for paging and cancel.

```sh
docker exec -i savoia-db-postgres-17-1 psql -U savoia -d savoia < samples/serie_a.postgres.sql   # schema serie_a
docker exec -i savoia-db-mysql-8.4-1 mysql -uroot -psavoia < samples/serie_a.mysql.sql          # database serie_a
```

To change the data, edit `samples/generate_serie_a.py` and run it; it rewrites both files with the same rows.

## UI conventions

- Colors come from `crates/savoia-app/src/theme.rs`; don't hard-code colors in views.
- Lucide icons beyond the default component set must be added to `crates/savoia-app/src/assets.rs`, otherwise they render blank.
- Only `savoia-app` depends on `gpui-kit`. Keep database, dump and storage logic in UI-free crates.
