//! The bridge between `log` and the sheet's log band.
//!
//! Worker threads log from wherever they run; this sink turns each record into
//! a ready-to-render row and hands it to the UI thread over a channel, which is
//! drained by a timer. Nothing here touches a Slint model.
//!
//! Rows are stamped with the time since launch (`+MM:SS`), not a wall clock:
//! `std` exposes no local offset, the GUI adds no crate to get one, and a UTC
//! clock shown to a KST user is worse than no clock at all. Elapsed time is
//! also what the reader actually wants from a build log.

use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use log::{Level, LevelFilter, Metadata, Record};

/// The most rows the band keeps. Older rows are dropped from the front.
pub const MAX_ROWS: usize = 400;

/// One rendered log row: the four columns `LogRowData` carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub time: String,
    pub target: String,
    pub message: String,
    /// `error`, `warn` or `info`; the band colours the row from it.
    pub level: String,
}

struct UiLog {
    tx: Sender<LogLine>,
    start: Instant,
    /// What `nebula gui -v` / `-q` asked for. `log::set_max_level` already
    /// filters, but a `Log` is asked directly by `log_enabled!`, so the sink
    /// has to agree with it.
    level: LevelFilter,
    /// Where the mirrored text goes: stderr when this is `None`, which is what
    /// `nebula gui` wants, and the file `nebula-gui` opens when it is not,
    /// because a windowless process has no console to mirror to.
    file: Option<Mutex<std::fs::File>>,
}

impl log::Log for UiLog {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let time = elapsed(self.start.elapsed());
        let target = short_target(record.target());
        let level = level_name(record.level());
        let body = record.args().to_string();

        // stderr keeps the CLI's shape so `nebula gui 2> log.txt` stays useful.
        match &self.file {
            Some(file) => {
                if let Ok(mut file) = file.lock() {
                    let _ = writeln!(
                        file,
                        "[{time}] [{}] [{}]: {body}",
                        record.level(),
                        record.target()
                    );
                }
            }
            None => {
                let _ = writeln!(
                    std::io::stderr(),
                    "[{time}] [{}] [{}]: {body}",
                    record.level(),
                    record.target()
                );
            }
        }

        // Boxed warnings arrive as one multi-line record; the band is a table.
        for line in body.lines() {
            let _ = self.tx.send(LogLine {
                time: time.clone(),
                target: target.clone(),
                message: line.to_owned(),
                level: level.to_owned(),
            });
        }
    }

    fn flush(&self) {
        match &self.file {
            Some(file) => {
                if let Ok(mut file) = file.lock() {
                    let _ = file.flush();
                }
            }
            None => {
                let _ = std::io::stderr().flush();
            }
        }
    }
}

/// Installs the sink at `level` — the CLI's own `-v`/`-q` verdict, so
/// `nebula gui -v` shows debug lines in the band as well as on stderr — and
/// returns the end the UI thread drains.
///
/// `log_file` replaces the stderr mirror, for the windowless binary that has
/// no console. A file that cannot be opened costs the mirror, not the band.
///
/// A second call is a no-op beyond returning a dead receiver: `log` only
/// accepts one logger per process.
pub fn install(level: LevelFilter, log_file: Option<&Path>) -> Receiver<LogLine> {
    let (tx, rx) = channel();
    let sink = UiLog {
        tx,
        start: Instant::now(),
        level,
        file: log_file.and_then(open_log).map(Mutex::new),
    };
    match log::set_boxed_logger(Box::new(sink)) {
        Ok(()) => log::set_max_level(level),
        // The band would stay empty with no diagnostic, so say it where the
        // logger cannot: another logger is already installed for this process.
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "로그를 연결하지 못했습니다: {error}");
        }
    }
    rx
}

/// The log file of one run. The config directory may not exist yet on a first
/// run, and the file is truncated rather than appended: what a user is asked
/// for is the log of the run that just failed, and one file cannot then grow
/// without bound.
fn open_log(path: &Path) -> Option<std::fs::File> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::File::create(path).ok()
}

/// `+MM:SS` since launch. Past an hour the minutes simply keep counting: a
/// build that long is worth reading as `+72:14`, not as a second clock.
pub fn elapsed(since: Duration) -> String {
    let secs = since.as_secs();
    format!("+{:02}:{:02}", secs / 60, secs % 60)
}

/// `1:23` for the busy pill (DESIGN-v2 §5), rendered in the mono face beside
/// the job name. Minutes keep counting past an hour, as they do in the log.
pub fn elapsed_mono(since: Duration) -> String {
    let secs = since.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn level_name(level: Level) -> &'static str {
    match level {
        Level::Error => "error",
        Level::Warn => "warn",
        _ => "info",
    }
}

/// `nebula_core::resolver::forge` renders as `forge`: the column is 52 px and
/// the crate path is the same on every row.
fn short_target(target: &str) -> String {
    target.rsplit("::").next().unwrap_or(target).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_elapsed_time_not_a_clock() {
        assert_eq!(elapsed(Duration::from_secs(0)), "+00:00");
        assert_eq!(elapsed(Duration::from_millis(1_900)), "+00:01");
        assert_eq!(elapsed(Duration::from_secs(83)), "+01:23");
        // Past an hour the minutes keep counting instead of wrapping.
        assert_eq!(elapsed(Duration::from_secs(4_334)), "+72:14");
    }

    #[test]
    fn the_busy_pill_counts_in_minutes_and_seconds() {
        assert_eq!(elapsed_mono(Duration::from_secs(0)), "0:00");
        assert_eq!(elapsed_mono(Duration::from_secs(9)), "0:09");
        assert_eq!(elapsed_mono(Duration::from_secs(59)), "0:59");
        assert_eq!(elapsed_mono(Duration::from_secs(83)), "1:23");
        // Past an hour the minutes keep counting rather than wrapping, so the
        // pill and the log column agree about what a long build looks like.
        assert_eq!(elapsed_mono(Duration::from_secs(3_600)), "60:00");
    }

    #[test]
    fn keeps_only_the_last_module_of_a_target() {
        assert_eq!(short_target("nebula_core::resolver::forge"), "forge");
        assert_eq!(short_target("files"), "files");
    }

    #[test]
    fn splits_multi_line_records_into_rows() {
        let (tx, rx) = channel();
        let sink = UiLog {
            tx,
            start: Instant::now(),
            level: LevelFilter::Info,
            file: None,
        };
        log::Log::log(
            &sink,
            &Record::builder()
                .args(format_args!("one\ntwo"))
                .level(Level::Warn)
                .target("nebula_core::server")
                .build(),
        );
        let rows: Vec<LogLine> = rx.try_iter().collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].message, "one");
        assert_eq!(rows[1].message, "two");
        assert_eq!(rows[0].target, "server");
        // The level travels with every row of a boxed record, not just the first.
        assert_eq!(rows[0].level, "warn");
        assert_eq!(rows[1].level, "warn");
        assert!(rows[0].time.starts_with("+00:"), "{}", rows[0].time);
    }

    #[test]
    fn drops_records_below_info() {
        let (tx, rx) = channel();
        let sink = UiLog {
            tx,
            start: Instant::now(),
            level: LevelFilter::Info,
            file: None,
        };
        log::Log::log(
            &sink,
            &Record::builder()
                .args(format_args!("noisy"))
                .level(Level::Debug)
                .target("nebula_core")
                .build(),
        );
        assert!(rx.try_iter().next().is_none());
    }
}
