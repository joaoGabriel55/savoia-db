#!/usr/bin/env bash
# Rebuilds and restarts Savoia Studio whenever its source changes.
#
#   scripts/dev.sh                          # reconnect the last-used data source
#   SAVOIA_DEV_OPEN=it_rel.orders scripts/dev.sh   # …and reopen that table's data view
#   SAVOIA_DEV_TRANSFER=export SAVOIA_DEV_OPEN=it_rel scripts/dev.sh   # …or the dump wizard
#
# A failed build leaves the running app alone, so a typo never closes it.
# Uses watchexec when installed (`brew install watchexec`), else polls.
set -u
cd "$(dirname "$0")/.."

export SAVOIA_DEV_RECONNECT="${SAVOIA_DEV_RECONNECT:-1}"
export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"

if command -v watchexec >/dev/null 2>&1; then
    exec watchexec --restart --exts rs,toml --watch crates --watch Cargo.toml \
        --debounce 300ms -- cargo run -p savoia-app
fi

app=""
marker="$(mktemp -t savoia-dev)"

stop() {
    if [ -n "$app" ] && kill -0 "$app" 2>/dev/null; then
        kill "$app" 2>/dev/null
        wait "$app" 2>/dev/null
    fi
    app=""
}

build_and_start() {
    touch "$marker"
    printf '\n\033[2m[dev] building…\033[0m\n'
    if cargo build -p savoia-app; then
        stop
        ./target/debug/savoia-studio &
        app=$!
        printf '\033[2m[dev] running (pid %s); watching crates/ for changes\033[0m\n' "$app"
    else
        printf '\033[33m[dev] build failed; the running app is unchanged\033[0m\n'
    fi
}

trap 'stop; rm -f "$marker"; exit 0' INT TERM

build_and_start
while true; do
    sleep 1
    changed="$(find crates Cargo.toml \( -name '*.rs' -o -name 'Cargo.toml' \) -newer "$marker" -print -quit 2>/dev/null)"
    if [ -n "$changed" ]; then
        # Let an editor finish writing a batch of files.
        sleep 0.3
        build_and_start
    fi
done
