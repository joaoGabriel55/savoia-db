//! Crash reports, opt-in. A panic writes a plain-text report to the data
//! folder; nothing leaves the machine on its own. At the next start, if the
//! user opted in, Savoia offers to open a prefilled GitHub issue that they
//! read and submit themselves.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Reports kept on disk when the user hasn't opted in.
const KEEP: usize = 5;
/// GitHub rejects very long issue URLs; the report is cut to fit.
const MAX_BODY: usize = 6000;

pub const ISSUES_URL: &str = "https://github.com/joaoGabriel55/savoia-studio/issues/new";

fn dir() -> Option<PathBuf> {
    savoia_store::data_dir().ok().map(|d| d.join("crashes"))
}

/// Chains a hook in front of the default one that also writes the report.
pub fn install() {
    let Some(dir) = dir() else {
        return;
    };
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let report = format_report(&info.to_string(), &backtrace.to_string());
        // Best-effort: the app is already going down.
        let _ = write_report(&dir, &report);
        default(info);
    }));
}

fn format_report(panic: &str, backtrace: &str) -> String {
    format!(
        "Savoia Studio {} on {} {}\n\n{panic}\n\nBacktrace:\n{backtrace}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

fn write_report(dir: &Path, report: &str) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    fs::write(dir.join(format!("crash-{millis}.txt")), report)
}

/// Reports left by earlier runs, newest first.
fn reports(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        })
        .collect();
    // The millisecond stamp sorts by name once padded lengths match, which
    // they do for the next few centuries.
    found.sort_unstable_by(|a, b| b.cmp(a));
    found
}

/// The newest unreported crash, to offer when the user opted in. Every
/// report is consumed either way: an opted-in user is asked once per crash,
/// and the others keep only the last few on disk.
pub fn take_pending(offer: bool) -> Option<String> {
    let dir = dir()?;
    take_pending_in(&dir, offer)
}

fn take_pending_in(dir: &Path, offer: bool) -> Option<String> {
    let found = reports(dir);
    if !offer {
        for old in found.iter().skip(KEEP) {
            let _ = fs::remove_file(old);
        }
        return None;
    }
    let newest = found.first().and_then(|p| fs::read_to_string(p).ok());
    for report in &found {
        let _ = fs::remove_file(report);
    }
    newest
}

/// A new-issue link with the report as its body.
pub fn issue_url(report: &str) -> String {
    let mut body = String::from(
        "<!-- Check the report for anything private (table names, values) before submitting. -->\n\n\
         **What were you doing?**\n\n\n**Crash report**\n```\n",
    );
    let mut end = report.len().min(MAX_BODY);
    while !report.is_char_boundary(end) {
        end -= 1;
    }
    body.push_str(&report[..end]);
    if end < report.len() {
        body.push_str("\n… (cut)");
    }
    body.push_str("\n```\n");
    let title = report
        .lines()
        .find(|l| l.starts_with("panicked at"))
        .map_or("Crash report".to_owned(), |l| format!("Crash: {l}"));
    url::Url::parse_with_params(ISSUES_URL, [("title", title.as_str()), ("body", &body)])
        .map(String::from)
        .unwrap_or_else(|_| ISSUES_URL.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("savoia-crash-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn an_opted_in_user_gets_the_newest_report_once() {
        let dir = temp_dir("offer");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("crash-1000.txt"), "old").unwrap();
        fs::write(dir.join("crash-2000.txt"), "new").unwrap();

        assert_eq!(take_pending_in(&dir, true).as_deref(), Some("new"));
        assert_eq!(take_pending_in(&dir, true), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_opt_in_only_the_last_few_are_kept() {
        let dir = temp_dir("keep");
        fs::create_dir_all(&dir).unwrap();
        for i in 0..8 {
            fs::write(dir.join(format!("crash-{}.txt", 1000 + i)), "x").unwrap();
        }
        assert_eq!(take_pending_in(&dir, false), None);
        assert_eq!(reports(&dir).len(), KEEP);
        assert!(dir.join("crash-1007.txt").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_issue_link_carries_a_cut_report() {
        let report = format_report("panicked at src/x.rs:1:2:\nboom", &"frame\n".repeat(5000));
        let url = issue_url(&report);
        assert!(url.starts_with(ISSUES_URL));
        assert!(url.contains("cut"));
        assert!(url.len() < 3 * MAX_BODY + 1000);
    }
}
