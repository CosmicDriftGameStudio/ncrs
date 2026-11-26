//! The log of a run: one line per command, newest last.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::SystemTime;

use ncrs_ui_kit::format::local_time;
use ncrs_ui_kit::git_state::RunOutcome;

/// Lines kept; older ones drop off the top.
pub const LOG_LIMIT: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// `hh:mm:ss`, worked out when the line is written so the view stays pure.
    pub time: String,
    pub path: PathBuf,
    pub repo: String,
    pub command: String,
    pub result: String,
    pub outcome: RunOutcome,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Log {
    lines: VecDeque<LogLine>,
}

impl Log {
    pub fn push(&mut self, line: LogLine) {
        if self.lines.len() >= LOG_LIMIT {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The lines, oldest first; with `only` the ones of that repository.
    pub fn lines<'a>(&'a self, only: Option<&'a PathBuf>) -> impl Iterator<Item = &'a LogLine> {
        self.lines
            .iter()
            .filter(move |line| only.is_none_or(|path| &line.path == path))
    }
}

/// `hh:mm:ss` in the local zone.
pub fn clock_text(at: SystemTime) -> String {
    local_time(at)
        .map(|moment| moment.format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(number: usize) -> LogLine {
        LogLine {
            time: String::new(),
            path: PathBuf::from(format!("/r/{}", number % 2)),
            repo: number.to_string(),
            command: "git fetch".to_owned(),
            result: "ok".to_owned(),
            outcome: RunOutcome::Updated,
        }
    }

    #[test]
    fn the_log_keeps_the_newest_lines() {
        let mut log = Log::default();
        for number in 0..LOG_LIMIT + 3 {
            log.push(line(number));
        }
        assert_eq!(log.len(), LOG_LIMIT);
        assert_eq!(log.lines(None).next().unwrap().repo, "3");
    }

    #[test]
    fn a_filter_keeps_one_repository() {
        let mut log = Log::default();
        for number in 0..4 {
            log.push(line(number));
        }
        let wanted = PathBuf::from("/r/1");
        assert_eq!(log.lines(Some(&wanted)).count(), 2);
    }
}
