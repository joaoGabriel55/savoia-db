<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.svg">
  <img alt="Savoia Studio" src="assets/logo.svg" width="420">
</picture>

# Savoia Studio

[![Support me on Ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/O5I528IC3A)

A lightweight but feature-rich desktop database client for PostgreSQL and MySQL. 100% Rust, with a native GPU-rendered UI built on [GPUI Kit](https://gpui-kit.com). No webview.

**[Download](https://github.com/joaoGabriel55/savoia-studio/releases/latest)** for macOS, Windows or Linux · **[Documentation](https://joaogabriel55.github.io/savoia-studio/)**

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
| `crates/savoia-transfer` | Dump and import: external client tools and the built-in engine |
| `docs/site` | The user documentation (mdBook), published to GitHub Pages |

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

### Working on the UI

`scripts/dev.sh` rebuilds and restarts the app whenever a source file changes, then reconnects the data source you used last. Set `SAVOIA_DEV_OPEN=schema.table` to also reopen that table's data view:

```sh
SAVOIA_DEV_OPEN=it_rel.orders scripts/dev.sh
```

It is a restart, not an in-place hot reload: GPUI has no hot-patching, so window state other than the reconnect and the data view starts fresh. A failed build leaves the running app as it was. `brew install watchexec` makes it react faster; without it the script polls once a second.

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

## Releasing

Push a `v*` tag; see [docs/RELEASING.md](docs/RELEASING.md).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
