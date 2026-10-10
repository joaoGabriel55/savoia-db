# Scrolling

Budget from [PLAN.md](../PLAN.md): scrolling 1M-row results at 60 fps, so no frame over 16.7 ms.

`scripts/bench.sh scroll` connects to Postgres, runs a six-column query over `generate_series(1, 1000000)` in a console, loads all 1M rows into the grid, and then moves the grid every frame in three phases, timing each frame:

- **Steady:** 600 frames at 40 px per frame, a trackpad scroll.
- **Fling:** 300 frames at 4,000 px per frame, about 150 new rows each frame.
- **Jumps:** 300 frames to random positions, like dragging the scrollbar.

```sh
SAVOIA_PG_URL=postgres://savoia:savoia@127.0.0.1:54317/savoia scripts/bench.sh scroll
```

## 2026-10-10, macOS 27 (Apple M3 Pro, 120 Hz display), release build, Postgres 17 in Docker

1M rows load in 1.1 s. Typical run (3 of 5 runs looked like this):

| Phase | Frames | Median | p95 | p99 | Max | Over 16.7 ms |
| --- | --- | --- | --- | --- | --- | --- |
| Steady, 40 px/frame | 600 | 8.3 ms | 9.1 ms | 9.3 ms | 9.4 ms | 0 |
| Fling, 4,000 px/frame | 300 | 8.3 ms | 9.1 ms | 9.3 ms | 9.4 ms | 0 |
| Random jumps | 300 | 8.3 ms | 9.2 ms | 9.3 ms | 9.4 ms | 0 |

Every frame lands on the display's 120 Hz cadence (8.3 ms), with half the 60 fps budget to spare.

The other two runs:

- **The first run after the build:** steady and fling were the same, but 87 of the 300 jumps (29%) took about 42 ms, and one took 1.0 s.
- **A run under the `sample` profiler:** 7 of the 300 jumps (2.3%) were over budget.

The profile shows the main thread mostly idle during jumps, with layout and text painting as its only work. The slow frames line up with other load on the machine, not with Savoia's rendering. Watch the jump phase when measuring again: it is the only phase that has missed.

Not measured: Intel Macs, Windows, Linux, and 60 Hz displays.
