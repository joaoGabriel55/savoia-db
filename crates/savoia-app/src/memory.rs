//! How much memory the app uses, for the status bar and the benchmark.

use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex};
use gpui_kit::*;

/// How often the status bar samples.
const SAMPLE_EVERY: Duration = Duration::from_secs(1);

/// The process's resident memory (RSS) in bytes: what it holds in RAM right
/// now, shared libraries and GPU mappings included. `None` where the
/// platform can't tell.
pub fn resident_bytes() -> Option<u64> {
    memory_stats::memory_stats().map(|m| m.physical_mem as u64)
}

/// e.g. "87.3 MB" or "1.24 GB" (decimal units, as Activity Monitor uses).
pub fn format_bytes(bytes: u64) -> String {
    let mb = bytes as f64 / 1e6;
    if mb >= 1000. {
        format!("{:.2} GB", mb / 1000.)
    } else {
        format!("{mb:.1} MB")
    }
}

/// Status bar item showing resident memory, refreshed every second.
pub struct MemoryMeter {
    bytes: Option<u64>,
    /// The most seen since launch.
    peak: u64,
    _sampler: Task<()>,
}

impl MemoryMeter {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let sampler = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SAMPLE_EVERY).await;
                if this.update(cx, |this, cx| this.sample(cx)).is_err() {
                    break;
                }
            }
        });
        let bytes = resident_bytes();
        Self {
            bytes,
            peak: bytes.unwrap_or(0),
            _sampler: sampler,
        }
    }

    fn sample(&mut self, cx: &mut Context<Self>) {
        let bytes = resident_bytes();
        // Redraw only for a visible change (0.1 MB).
        let shown = |b: Option<u64>| b.map(|b| b / 100_000);
        if shown(bytes) != shown(self.bytes) {
            cx.notify();
        }
        self.bytes = bytes;
        self.peak = self.peak.max(bytes.unwrap_or(0));
    }
}

impl Render for MemoryMeter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let Some(bytes) = self.bytes else {
            return div().into_any_element();
        };
        h_flex()
            .id("memory-meter")
            .gap_1()
            .text_xs()
            .text_color(muted)
            .child(Icon::new(Lucide::MemoryStick).xsmall())
            .child(format_bytes(bytes))
            .tooltip({
                let peak = format_bytes(self.peak);
                move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(format!(
                        "Memory in RAM (resident set), peak {peak}"
                    ))
                    .build(window, cx)
                }
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{format_bytes, resident_bytes};

    #[test]
    fn formats_in_decimal_units() {
        assert_eq!(format_bytes(87_340_000), "87.3 MB");
        assert_eq!(format_bytes(1_240_000_000), "1.24 GB");
    }

    #[test]
    fn reads_this_process() {
        let bytes = resident_bytes().expect("supported platform");
        assert!(bytes > 1_000_000, "{bytes}");
    }
}
