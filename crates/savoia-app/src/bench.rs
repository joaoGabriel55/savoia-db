//! The release-check benchmarks that need a real, GPU-drawn window (the test
//! platform draws nothing). Built only with `--features bench`; driven by
//! `scripts/bench.sh`, which see. Results go to stdout as `bench …` lines.
//!
//! - `SAVOIA_BENCH=startup`: time from `SAVOIA_BENCH_T0` (Unix ns, taken by
//!   the script just before it starts the process) to the end of the first
//!   frame, then quit.
//! - `SAVOIA_BENCH=scroll`: connect to `SAVOIA_PG_URL`, load a 1M-row result
//!   into a console, scroll its grid every frame and report frame intervals.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui_kit::*;
use savoia_store::{ConnectionStore, MemorySecrets};
use savoia_tunnel::HostKeyPolicy;

use crate::data_sources::{DataSources, SourceState};
use crate::workspace::Workspace;

pub enum Bench {
    Startup,
    Scroll,
}

pub fn from_env() -> Option<Bench> {
    match std::env::var("SAVOIA_BENCH").as_deref() {
        Ok("startup") => Some(Bench::Startup),
        Ok("scroll") => Some(Bench::Scroll),
        _ => None,
    }
}

/// Calls `done` once the first frame has been drawn: the second frame's
/// callback runs only after the first one is presented.
pub fn after_first_frame(window: &Window, done: impl FnOnce(&mut App) + 'static) {
    window.on_next_frame(move |window, _| {
        window.on_next_frame(move |_, cx| done(cx));
        window.refresh();
    });
}

pub fn report_startup(cx: &mut App) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    match std::env::var("SAVOIA_BENCH_T0")
        .ok()
        .and_then(|t| t.parse::<u128>().ok())
    {
        Some(t0) => println!("bench startup_ms {:.1}", (now - t0) as f64 / 1e6),
        None => println!("bench startup_ms unknown (SAVOIA_BENCH_T0 unset)"),
    }
    cx.quit();
}

/// A store in memory, so a run never touches the user's connections.
pub fn data_sources(cx: &mut App) -> Entity<DataSources> {
    cx.new(|_| {
        DataSources::with_stores(
            ConnectionStore::open_in_memory(),
            Arc::new(MemorySecrets::default()),
        )
    })
}

/// Six columns of mixed types, like a typical wide-enough table.
const SCROLL_SQL: &str = "SELECT g AS id, md5(g::text) AS hash, \
    timestamp '2026-01-01' + g * interval '1 second' AS at, g % 97 AS bucket, \
    (g % 2 = 0) AS even, repeat('x', g % 40) AS pad \
    FROM generate_series(1, 1000000) g";
const ROWS: usize = 1_000_000;

/// Drives the scroll benchmark on `workspace`, then quits.
pub fn scroll(
    workspace: Entity<Workspace>,
    data_sources: Entity<DataSources>,
    window: &mut Window,
    cx: &mut App,
) {
    let url = std::env::var("SAVOIA_PG_URL").expect("SAVOIA_PG_URL");
    let (mut config, secrets) = savoia_core::parse_url(&url).expect("a valid SAVOIA_PG_URL");
    config.save_password = false;
    let id = config.id;
    data_sources.update(cx, |ds, cx| {
        ds.save(config, secrets, cx).expect("save");
        ds.connect(id, HostKeyPolicy::KnownOnly, cx);
    });

    window
        .spawn(cx, async move |cx| {
            let executor = cx.background_executor().clone();
            let tick = || executor.timer(Duration::from_millis(50));
            // Connected?
            loop {
                let state = cx.update(|_, cx| match data_sources.read(cx).state(id) {
                    SourceState::Connected(_) => Ok(true),
                    SourceState::Failed(e) => Err(e.to_string()),
                    _ => Ok(false),
                });
                match state {
                    Ok(Ok(true)) => break,
                    Ok(Err(e)) => panic!("connect failed: {e}"),
                    _ => tick().await,
                }
            }
            let console = cx
                .update(|window, cx| {
                    workspace.update(cx, |w, cx| {
                        let console = w.open_console(Some(id), window, cx);
                        console.update(cx, |c, cx| c.insert_sql(SCROLL_SQL, true, window, cx));
                        console
                    })
                })
                .unwrap();
            let loading = Instant::now();
            // Load all of it, then wait until the run ends.
            loop {
                let done = cx
                    .update(|_, cx| {
                        let console = console.read(cx);
                        let Some(table) = console.results().into_iter().next() else {
                            return false;
                        };
                        let set = table.read(cx).delegate();
                        if let Some(pacer) = set.pacer() {
                            pacer.load_all();
                        }
                        !console.is_running() && set.len() >= ROWS
                    })
                    .unwrap();
                if done {
                    break;
                }
                tick().await;
            }
            println!(
                "bench rows_loaded {ROWS} in {:.1} s",
                loading.elapsed().as_secs_f64()
            );
            cx.update(|window, cx| {
                let table = console.read(cx).results()[0].clone();
                run_scroll(table, window, cx);
            })
            .ok();
        })
        .detach();
}

/// Phases of the scroll, in frames: a steady trackpad-like scroll, a fast
/// fling, then random jumps as when dragging the scrollbar.
const STEADY: usize = 600;
const FLING: usize = 300;
const JUMPS: usize = 300;

struct Run {
    frame: usize,
    last: Option<Instant>,
    intervals: Vec<(usize, Duration)>,
    seed: u64,
}

fn run_scroll(
    table: Entity<gpui_kit::component::table::TableState<crate::results::ResultSet>>,
    window: &mut Window,
    _: &mut App,
) {
    let state = Rc::new(RefCell::new(Run {
        frame: 0,
        last: None,
        intervals: Vec::with_capacity(STEADY + FLING + JUMPS),
        seed: 0x9E37_79B9_7F4A_7C15,
    }));
    step(table, state, window);
}

fn step(
    table: Entity<gpui_kit::component::table::TableState<crate::results::ResultSet>>,
    state: Rc<RefCell<Run>>,
    window: &mut Window,
) {
    window.on_next_frame(move |window, cx| {
        let mut run = state.borrow_mut();
        let now = Instant::now();
        if let Some(last) = run.last {
            let phase = run.frame;
            run.intervals.push((phase, now - last));
        }
        run.last = Some(now);
        run.frame += 1;
        if run.frame > STEADY + FLING + JUMPS {
            report_scroll(&run.intervals);
            cx.quit();
            return;
        }
        let handle = table
            .read(cx)
            .vertical_scroll_handle
            .0
            .borrow()
            .base_handle
            .clone();
        let max = handle.max_offset().y;
        let y = -handle.offset().y;
        let next = if run.frame <= STEADY {
            y + px(40.)
        } else if run.frame <= STEADY + FLING {
            y + px(4000.)
        } else {
            run.seed ^= run.seed << 13;
            run.seed ^= run.seed >> 7;
            run.seed ^= run.seed << 17;
            max * ((run.seed % 10_000) as f32 / 10_000.)
        };
        handle.set_offset(point(px(0.), -next.min(max)));
        drop(run);
        table.update(cx, |_, cx| cx.notify());
        step(table, state, window);
    });
}

fn report_scroll(intervals: &[(usize, Duration)]) {
    let phases = [
        ("steady 40px/frame", 1..=STEADY),
        ("fling 4000px/frame", STEADY + 1..=STEADY + FLING),
        ("random jumps", STEADY + FLING + 1..=STEADY + FLING + JUMPS),
    ];
    println!(
        "bench scroll | phase | frames | median ms | p95 ms | p99 ms | max ms | over 16.7 ms |"
    );
    for (name, range) in phases {
        let mut ms: Vec<f64> = intervals
            .iter()
            .filter(|(f, _)| range.contains(f))
            .map(|(_, d)| d.as_secs_f64() * 1e3)
            .collect();
        ms.sort_by(f64::total_cmp);
        let at = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
        let over = ms.iter().filter(|&&m| m > 1000. / 60.).count();
        println!(
            "bench scroll | {name} | {} | {:.1} | {:.1} | {:.1} | {:.1} | {} ({:.1}%) |",
            ms.len(),
            at(0.5),
            at(0.95),
            at(0.99),
            ms[ms.len() - 1],
            over,
            100. * over as f64 / ms.len() as f64
        );
    }
}
