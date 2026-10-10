# Startup

Budget from [PLAN.md](../PLAN.md): cold start under 1 s.

Measured from just before the process is started to the end of the first drawn frame, with the real startup path: store, saved connections, theme and a console. `scripts/bench.sh startup` builds a release binary with the `bench` feature and runs it in a real window.

```sh
scripts/bench.sh startup 10
```

## 2026-10-10, macOS 27 (Apple M3 Pro), release build

| Launch | Runs | Time to first frame |
| --- | --- | --- |
| Not running; this binary launched before | 17 | 202–247 ms, median about 205 ms |
| First launch of a binary macOS has never seen | 2 | 1,102 ms and 1,274 ms |

Findings:

- The budget holds for every launch except the first one of a new binary: about 205 ms.
- That first launch costs about 0.9 s more. A copy of the same binary at a new path reproduces it, then drops back to 202 ms on the next run, which points to macOS scanning a new executable (Gatekeeper/XProtect) rather than to Savoia. Users pay it once per install or update. It may shrink once builds are signed and notarized ([RELEASING.md](../RELEASING.md)); measure again then.
- Not measured: a cold disk cache (`sudo purge`) or Windows and Linux.
