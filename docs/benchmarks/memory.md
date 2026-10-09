# Memory

Budget from [PLAN.md](../PLAN.md): idle RAM under 150 MB.

## 2026-10-09, macOS (Apple Silicon), release build

**Real app, idle** (window open, no connection), measured 10 s after launch:

| Measure | Value |
| --- | --- |
| Physical footprint (`footprint <pid>`, Activity Monitor's "Memory") | 69 MB, of which 44 MB is graphics |
| Resident set (`ps -o rss`, the status-bar meter) | 104.6 MB |

**Headless scenario** over the Serie A sample, Postgres 17 in Docker:

```sh
SAVOIA_PG_URL=postgres://savoia:savoia@127.0.0.1:54317/savoia \
  cargo test -p savoia-app --release memory_benchmark -- --ignored --nocapture
```

The test platform draws nothing, so this leaves out GPU memory; compare the deltas, not the totals.

| Step | Resident memory | Δ | Took |
| --- | --- | --- | --- |
| start | 12.4 MB | | 0 ms |
| connected, explorer open | 27.2 MB | +14.8 MB | 76 ms |
| 150,000 rows × 6 columns in the grid | 79.7 MB | +52.5 MB | 382 ms |
| result replaced by `SELECT 1` | 70.9 MB | −8.7 MB | 75 ms |
| `serie_a` ER diagram (14 tables) drawn | 75.3 MB | +4.4 MB | 61 ms |

Findings:

- Idle is well under budget: 69 MB footprint, 105 MB resident.
- A fully loaded result costs about 350 bytes per row (6 short text cells). One million such rows would take about 350 MB, which is why the grid pauses results instead of loading them whole.
- Replacing a big result gave back only 8.7 MB of its 52.5 MB to the OS. Either the allocator keeps freed pages for reuse (common for macOS `malloc`) or something still holds the old rows. Not yet investigated.
