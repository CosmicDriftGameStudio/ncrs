//! What the filing status page knows: the latest snapshot of the pipeline, how
//! far the current stretch of work has come, the counts of Hermes and the
//! questions that wait for the user.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use ncrs_index::HermesCounts;
use ncrs_pipeline::{ErrorEntry, RunnerState, Snapshot, RATE_MINUTES};

pub const SNAPSHOT_EVERY: Duration = Duration::from_secs(1);
/// While nothing runs and no page shows the numbers, the tick only has to
/// notice new work; every tick rebuilds the whole view.
pub const IDLE_SNAPSHOT_EVERY: Duration = Duration::from_secs(5);
/// The counts of Hermes are read this often, also while its page is closed.
pub const HERMES_COUNTS_EVERY_TICKS: u32 = 5;

/// How many open questions a source has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceQuestions {
    pub source: PathBuf,
    pub count: usize,
}

#[derive(Debug)]
pub struct FilingStatus {
    snapshot: Snapshot,
    taken_at: SystemTime,
    page_open: bool,
    /// The last counts that could be read; kept when a later read fails.
    counts: Option<HermesCounts>,
    /// The last questions that could be read, like the counts; only of the
    /// sources that have any.
    questions: Vec<SourceQuestions>,
    counts_in_flight: bool,
    ticks_since_counts: u32,
}

impl Default for FilingStatus {
    fn default() -> Self {
        Self {
            snapshot: Snapshot::off(),
            taken_at: SystemTime::UNIX_EPOCH,
            page_open: false,
            counts: None,
            questions: Vec::new(),
            counts_in_flight: false,
            ticks_since_counts: HERMES_COUNTS_EVERY_TICKS,
        }
    }
}

impl FilingStatus {
    pub fn set_snapshot(&mut self, snapshot: Snapshot, now: SystemTime) {
        self.snapshot = snapshot;
        self.taken_at = now;
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// How often the snapshot is taken: every second while a run or an open
    /// page shows it, slowly while the pipeline idles.
    pub fn tick_every(&self) -> Duration {
        if self.page_open || self.snapshot.state == RunnerState::Running {
            SNAPSHOT_EVERY
        } else {
            IDLE_SNAPSHOT_EVERY
        }
    }

    pub fn taken_at(&self) -> SystemTime {
        self.taken_at
    }

    /// Averaged over the full minutes back to the oldest one with documents,
    /// so a runner that has just started has a value after its first full
    /// minute. The running minute is left out: it is not over yet.
    pub fn documents_per_minute(&self) -> Option<usize> {
        let full_minutes = self.snapshot.documents_per_minute.get(1..RATE_MINUTES)?;
        let span = full_minutes.iter().rposition(|&count| count > 0)? + 1;
        let documents: usize = full_minutes.iter().take(span).sum();
        Some(documents.div_ceil(span))
    }

    /// Without the errors of files that have been read since.
    pub fn errors_newest_first(&self) -> impl Iterator<Item = &ErrorEntry> {
        self.snapshot
            .errors
            .iter()
            .rev()
            .filter(move |error| !self.snapshot.is_superseded(error))
    }

    pub fn page_open(&self) -> bool {
        self.page_open
    }

    pub fn open_page(&mut self) {
        self.page_open = true;
    }

    pub fn close_page(&mut self) {
        self.page_open = false;
    }

    pub fn hermes_counts(&self) -> Option<&HermesCounts> {
        self.counts.as_ref()
    }

    pub fn questions(&self) -> &[SourceQuestions] {
        &self.questions
    }

    /// Files that wait for an answer of the user, over all sources.
    pub fn waiting(&self) -> usize {
        self.questions.iter().map(|asked| asked.count).sum()
    }

    pub fn questions_arrived(&mut self, read: Result<Vec<SourceQuestions>, String>) {
        if let Ok(questions) = read {
            self.questions = questions;
            self.questions.retain(|asked| asked.count > 0);
        }
    }

    /// Counts a tick; true when the counts are due.
    pub fn tick_wants_counts(&mut self) -> bool {
        self.ticks_since_counts = self.ticks_since_counts.saturating_add(1);
        self.ticks_since_counts >= HERMES_COUNTS_EVERY_TICKS && !self.counts_in_flight
    }

    pub fn counts_requested(&mut self) {
        self.counts_in_flight = true;
        self.ticks_since_counts = 0;
    }

    pub fn counts_arrived(&mut self, read: Result<HermesCounts, String>) {
        if let Ok(counts) = read {
            self.counts = Some(counts);
        }
        self.counts_in_flight = false;
        self.ticks_since_counts = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ncrs_pipeline::{
        Busy, ChangeOutcome, ChangedPath, RunnerState, StepSnapshot, Task, WorkerSnapshot,
    };

    use super::*;

    fn at() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    fn snapshot(state: RunnerState, left: usize, waiting: usize) -> Snapshot {
        let mut snapshot = Snapshot::off();
        snapshot.state = state;
        snapshot.left_pipeline = left;
        snapshot.steps = vec![StepSnapshot {
            name: "probe",
            waiting,
            done: 0,
            failed: 0,
            unchanged: 0,
        }];
        snapshot
    }

    fn with_rate(rate: &[(usize, usize)], waiting: usize) -> FilingStatus {
        let mut snapshot = snapshot(RunnerState::Running, 0, waiting);
        for &(minute, count) in rate {
            snapshot.documents_per_minute[minute] = count;
        }
        let mut status = FilingStatus::default();
        status.set_snapshot(snapshot, at());
        status
    }

    #[test]
    fn the_tick_slows_down_only_while_nothing_runs_and_no_page_is_open() {
        let mut status = FilingStatus::default();
        status.set_snapshot(snapshot(RunnerState::Idle, 0, 0), at());
        assert_eq!(status.tick_every(), IDLE_SNAPSHOT_EVERY);
        status.open_page();
        assert_eq!(status.tick_every(), SNAPSHOT_EVERY);
        status.close_page();
        status.set_snapshot(snapshot(RunnerState::Running, 0, 1), at());
        assert_eq!(status.tick_every(), SNAPSHOT_EVERY);
    }

    fn asked(source: &str, count: usize) -> SourceQuestions {
        SourceQuestions {
            source: PathBuf::from(source),
            count,
        }
    }

    #[test]
    fn waiting_questions_do_not_speed_up_an_idle_tick() {
        let mut status = FilingStatus::default();
        status.set_snapshot(snapshot(RunnerState::Idle, 0, 0), at());
        status.questions_arrived(Ok(vec![asked("/in", 3)]));
        assert_eq!(status.tick_every(), IDLE_SNAPSHOT_EVERY);

        status.counts_requested();
        assert!((0..HERMES_COUNTS_EVERY_TICKS * 2).all(|_| !status.tick_wants_counts()));
        status.counts_arrived(Ok(HermesCounts::default()));
        let due = (0..HERMES_COUNTS_EVERY_TICKS)
            .filter(|_| status.tick_wants_counts())
            .count();
        assert_eq!(due, 1);
    }

    #[test]
    fn the_questions_of_all_sources_are_added_up_and_outlive_a_failed_read() {
        let mut status = FilingStatus::default();
        assert_eq!(status.waiting(), 0);

        status.questions_arrived(Ok(vec![
            asked("/in", 3),
            asked("/calm", 0),
            asked("/scans", 4),
        ]));
        assert_eq!(status.waiting(), 7);
        assert_eq!(status.questions(), [asked("/in", 3), asked("/scans", 4)]);

        status.questions_arrived(Err("no index".to_owned()));
        assert_eq!(status.waiting(), 7);
        status.questions_arrived(Ok(Vec::new()));
        assert_eq!(status.waiting(), 0);
    }

    #[test]
    fn a_file_in_a_step_counts_as_open_but_a_scan_does_not() {
        let mut busy = snapshot(RunnerState::Running, 0, 0);
        busy.workers = vec![WorkerSnapshot {
            id: 0,
            busy: Some(Busy {
                file: PathBuf::from("/tmp/inbox"),
                task: Task::Scan,
                started_at: at(),
            }),
        }];
        let mut status = FilingStatus::default();
        status.set_snapshot(busy, at());
        assert_eq!(status.snapshot().open_files(), 0);
    }

    #[test]
    fn an_error_is_gone_once_its_file_was_read_since() {
        let error = |file: &str| ErrorEntry {
            file: PathBuf::from(file),
            task: Task::Scan,
            message: String::new(),
            at: at(),
            sequence: 1,
        };
        let change = |file: &str, sequence, outcome| ChangedPath {
            sequence,
            path: PathBuf::from(file),
            outcome,
        };
        let mut seen = snapshot(RunnerState::Running, 0, 0);
        seen.errors = vec![error("/read.pdf"), error("/failed.pdf"), error("/left.pdf")];
        seen.changed_paths = vec![
            change("/read.pdf", 2, ChangeOutcome::Finished),
            change("/failed.pdf", 3, ChangeOutcome::Failed),
        ];
        let mut status = FilingStatus::default();
        status.set_snapshot(seen, at());
        let left: Vec<_> = status
            .errors_newest_first()
            .map(|entry| entry.file.as_path())
            .collect();
        assert_eq!(
            left,
            [
                std::path::Path::new("/left.pdf"),
                std::path::Path::new("/failed.pdf")
            ]
        );
    }

    #[test]
    fn the_rate_averages_the_full_minutes_back_to_the_oldest_with_documents() {
        let status = with_rate(&[(0, 50), (1, 10), (2, 0), (3, 20)], 60);
        assert_eq!(status.documents_per_minute(), Some(10));
    }

    #[test]
    fn there_is_no_rate_without_a_full_minute_with_documents() {
        assert_eq!(with_rate(&[], 5).documents_per_minute(), None);
        assert_eq!(with_rate(&[(0, 9)], 5).documents_per_minute(), None);
    }
}
