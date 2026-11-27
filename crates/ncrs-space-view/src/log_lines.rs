//! The rows `ncrs_space::append_log` wrote, read back for the log view.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub when: SystemTime,
    pub path: PathBuf,
    pub size: u64,
    /// The bytes freed; `None` for a row that was skipped.
    pub freed: Option<u64>,
    /// What the log wrote about a skipped row, as the log words it.
    pub skipped: Option<String>,
}

/// One tab-separated row of a log file; `None` for a line of another shape.
pub fn parse_line(line: &str) -> Option<LogLine> {
    let mut parts = line.splitn(5, '\t');
    let seconds: u64 = parts.next()?.parse().ok()?;
    let path = PathBuf::from(parts.next()?);
    let size: u64 = parts.next()?.parse().ok()?;
    let (freed, skipped) = match parts.next()? {
        "freed" => (Some(parts.next()?.parse().ok()?), None),
        "skipped" => (None, Some(parts.next()?.to_owned())),
        _ => return None,
    };
    Some(LogLine {
        when: UNIX_EPOCH + Duration::from_secs(seconds),
        path,
        size,
        freed,
        skipped,
    })
}

/// The lines that can be read, newest first.
pub fn parse_all(lines: &[String]) -> Vec<LogLine> {
    let mut parsed: Vec<LogLine> = lines.iter().filter_map(|line| parse_line(line)).collect();
    parsed.reverse();
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_freed_and_a_skipped_row_are_read_back() {
        let freed = parse_line("100\t/home/a/.yarn\t2048\tfreed\t2048").expect("freed row");
        assert_eq!(freed.freed, Some(2048));
        let skipped =
            parse_line("100\t/home/a/t\t10\tskipped\ttool-running cargo 7").expect("skipped row");
        assert_eq!(skipped.freed, None);
        assert_eq!(skipped.skipped.as_deref(), Some("tool-running cargo 7"));
        assert!(parse_line("# run\tstarted=1").is_none());
    }
}
