#!/usr/bin/env bash
# Release-check benchmarks in a real window (see docs/benchmarks/).
#
#   scripts/bench.sh startup [runs]   # launch → first frame, default 10 runs
#   scripts/bench.sh scroll           # 1M-row grid scroll; needs SAVOIA_PG_URL
#
# Builds a release binary with the `bench` feature first. The first startup
# run follows the build, so the binary is in the file cache: these are warm
# starts of a not-running app, which is what users meet after the first launch.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build -p savoia-app --release --features bench --quiet
bin=target/release/savoia-studio
now_ns() { python3 -c 'import time; print(time.time_ns())'; }

case "${1:-}" in
  startup)
    runs=${2:-10}
    for _ in $(seq "$runs"); do
      SAVOIA_BENCH=startup SAVOIA_BENCH_T0=$(now_ns) "$bin" | grep '^bench startup_ms' | awk '{print $3}'
    done | sort -n | awk '{ v[NR] = $1 } END {
      printf "startup over %d runs: min %.0f ms, median %.0f ms, max %.0f ms\n", NR, v[1], v[int((NR + 1) / 2)], v[NR] }'
    ;;
  scroll)
    : "${SAVOIA_PG_URL:?set SAVOIA_PG_URL, e.g. postgres://savoia:savoia@127.0.0.1:54317/savoia}"
    SAVOIA_BENCH=scroll "$bin" | grep '^bench' | sed 's/^bench //'
    ;;
  *)
    echo "usage: $0 startup [runs] | scroll" >&2
    exit 2
    ;;
esac
