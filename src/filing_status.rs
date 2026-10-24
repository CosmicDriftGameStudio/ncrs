//! What the filing status page knows: the latest snapshot of the pipeline, how
//! far the current stretch of work has come, and the index figures.

use std::path::Path;
use std::time::{Duration, SystemTime};

use ncrs_pipeline::{ErrorEntry, RunnerState, Snapshot, RATE_MINUTES};

use crate::filing::IndexFigures;

pub const SNAPSHOT_EVERY: Duration = Duration::from_secs(1);
pub const INDEX_FIGURES_EVERY_TICKS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilingProgress {
    pub done: usize,
    pub total: usize,
    pub paused: bool,
}

#[derive(Debug)]
pub struct FilingStatus {
    snapshot: Snapshot,
    taken_at: SystemTime,
    /// `left_pipeline` when the current stretch of work began.
    progress_baseline: Option<usize>,
    page_open: bool,
    index_figures: Option<Result<IndexFigures, String>>,
    figures_in_flight: bool,
    ticks_since_figures: u32,
    /// Position in the list as shown, newest first.
    selected_error: Option<usize>,
}

impl Default for FilingStatus {
    fn default() -> Self {
        Self {
            snapshot: Snapshot::off(),
            taken_at: SystemTime::UNIX_EPOCH,
            progress_baseline: None,
            page_open: false,
            index_figures: None,
            figures_in_flight: false,
            ticks_since_figures: 0,
            selected_error: None,
        }
    }
}

/// A paused pipeline with files left still has work.
fn has_work(snapshot: &Snapshot) -> bool {
    snapshot.state != RunnerState::Off && snapshot.open_files() > 0
}

impl FilingStatus {
    pub fn set_snapshot(&mut self, snapshot: Snapshot, now: SystemTime) {
        if !has_work(&snapshot) {
            self.progress_baseline = None;
        } else if self.progress_baseline.is_none() {
            self.progress_baseline = Some(snapshot.left_pipeline);
        }
        self.snapshot = snapshot;
        self.taken_at = now;
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    pub fn taken_at(&self) -> SystemTime {
        self.taken_at
    }

    pub fn progress(&self) -> Option<FilingProgress> {
        if !has_work(&self.snapshot) {
            return None;
        }
        let baseline = self.progress_baseline?;
        let done = self.snapshot.left_pipeline.saturating_sub(baseline);
        Some(FilingProgress {
            done,
            total: done + self.snapshot.open_files(),
            paused: self.snapshot.state == RunnerState::Paused,
        })
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

    /// Files are open, but no full minute has been measured, so there is
    /// neither a rate nor a time left yet.
    pub fn awaiting_first_minute(&self) -> bool {
        self.snapshot.open_files() > 0 && self.documents_per_minute().is_none()
    }

    pub fn remaining_estimate(&self) -> Option<Duration> {
        let open = self.snapshot.open_files();
        if open == 0 {
            return None;
        }
        let rate = self.documents_per_minute()?;
        let minutes = open.div_ceil(rate);
        Some(Duration::from_secs(
            u64::try_from(minutes).ok()?.checked_mul(60)?,
        ))
    }

    pub fn errors_newest_first(&self) -> impl Iterator<Item = &ErrorEntry> {
        self.snapshot.errors.iter().rev()
    }

    pub fn selected_error(&self) -> Option<usize> {
        self.selected_error
            .filter(|&position| position < self.snapshot.errors.len())
    }

    pub fn move_error_selection(&mut self, delta: i32) {
        let Some(last) = self.snapshot.errors.len().checked_sub(1) else {
            self.selected_error = None;
            return;
        };
        let current = self.selected_error().unwrap_or(0);
        let moved = i64::try_from(current)
            .unwrap_or(0)
            .saturating_add(i64::from(delta));
        self.selected_error = Some(usize::try_from(moved.max(0)).unwrap_or(0).min(last));
    }

    pub fn selected_error_file(&self) -> Option<&Path> {
        let position = self.selected_error()?;
        self.errors_newest_first()
            .nth(position)
            .map(|entry| entry.file.as_path())
    }

    pub fn page_open(&self) -> bool {
        self.page_open
    }

    pub fn open_page(&mut self) {
        self.page_open = true;
        self.selected_error = None;
    }

    pub fn close_page(&mut self) {
        self.page_open = false;
    }

    pub fn index_figures(&self) -> Option<&Result<IndexFigures, String>> {
        self.index_figures.as_ref()
    }

    #[cfg(test)]
    pub fn figures_in_flight(&self) -> bool {
        self.figures_in_flight
    }

    /// Counts a tick of an open page; true when figures are due.
    pub fn tick_wants_figures(&mut self) -> bool {
        if !self.page_open {
            return false;
        }
        self.ticks_since_figures = self.ticks_since_figures.saturating_add(1);
        self.ticks_since_figures >= INDEX_FIGURES_EVERY_TICKS && !self.figures_in_flight
    }

    pub fn figures_requested(&mut self) {
        self.figures_in_flight = true;
        self.ticks_since_figures = 0;
    }

    pub fn figures_arrived(&mut self, figures: Result<IndexFigures, String>) {
        self.index_figures = Some(figures);
        self.figures_in_flight = false;
        self.ticks_since_figures = 0;
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::path::PathBuf;

    use ncrs_pipeline::{Busy, StepSnapshot, Task, WorkerSnapshot};

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
    fn the_first_minute_is_awaited_only_while_files_are_open_and_no_minute_is_measured() {
        assert!(with_rate(&[], 5).awaiting_first_minute());
        assert!(with_rate(&[(0, 7)], 5).awaiting_first_minute());
        assert!(!with_rate(&[(1, 7)], 5).awaiting_first_minute());
        assert!(!with_rate(&[], 0).awaiting_first_minute());
        assert!(!FilingStatus::default().awaiting_first_minute());
    }

    fn with_errors(count: usize) -> FilingStatus {
        let mut snapshot = snapshot(RunnerState::Idle, 0, 0);
        snapshot.errors = (0..count)
            .map(|number| ErrorEntry {
                file: PathBuf::from(format!("/tmp/inbox/company-a-{number}.pdf")),
                task: Task::Step("probe"),
                message: "the step stopped unexpectedly".to_owned(),
                at: at(),
            })
            .collect();
        let mut status = FilingStatus::default();
        status.set_snapshot(snapshot, at());
        status
    }

    #[test]
    fn progress_counts_from_where_the_stretch_of_work_began() {
        let mut status = FilingStatus::default();
        status.set_snapshot(snapshot(RunnerState::Running, 100, 340), at());
        assert_eq!(
            status.progress(),
            Some(FilingProgress {
                done: 0,
                total: 340,
                paused: false
            })
        );

        status.set_snapshot(snapshot(RunnerState::Running, 112, 328), at());
        assert_eq!(
            status.progress(),
            Some(FilingProgress {
                done: 12,
                total: 340,
                paused: false
            })
        );

        status.set_snapshot(snapshot(RunnerState::Idle, 440, 0), at());
        assert_eq!(status.progress(), None);

        status.set_snapshot(snapshot(RunnerState::Running, 440, 5), at());
        assert_eq!(status.progress().map(|p| (p.done, p.total)), Some((0, 5)));
    }

    #[test]
    fn a_paused_pipeline_with_open_files_has_work_and_an_off_one_has_none() {
        let mut status = FilingStatus::default();
        status.set_snapshot(snapshot(RunnerState::Paused, 0, 4), at());
        assert_eq!(status.progress().map(|p| p.paused), Some(true));

        status.set_snapshot(snapshot(RunnerState::Off, 0, 4), at());
        assert_eq!(status.progress(), None);
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
        assert_eq!(status.progress(), None);
    }

    #[test]
    fn the_rate_averages_the_full_minutes_back_to_the_oldest_with_documents() {
        let status = with_rate(&[(0, 50), (1, 10), (2, 0), (3, 20)], 60);
        assert_eq!(status.documents_per_minute(), Some(10));
        assert_eq!(
            status.remaining_estimate(),
            Some(Duration::from_secs(6 * 60))
        );
    }

    #[test]
    fn there_is_no_rate_without_a_full_minute_with_documents() {
        assert_eq!(with_rate(&[], 5).documents_per_minute(), None);
        let only_running_minute = with_rate(&[(0, 9)], 5);
        assert_eq!(only_running_minute.documents_per_minute(), None);
        assert_eq!(only_running_minute.remaining_estimate(), None);
    }

    #[test]
    fn there_is_no_estimate_without_open_files() {
        assert_eq!(with_rate(&[(1, 10)], 0).remaining_estimate(), None);
    }

    #[test]
    fn the_error_selection_stays_inside_the_list() {
        let mut status = with_errors(3);
        assert_eq!(status.selected_error(), None);

        status.move_error_selection(-1);
        assert_eq!(status.selected_error(), Some(0));
        status.move_error_selection(1);
        status.move_error_selection(1);
        status.move_error_selection(1);
        assert_eq!(status.selected_error(), Some(2));
        assert_eq!(
            status.selected_error_file(),
            Some(Path::new("/tmp/inbox/company-a-0.pdf"))
        );

        let mut empty = with_errors(0);
        empty.move_error_selection(1);
        assert_eq!(empty.selected_error(), None);
        assert_eq!(empty.selected_error_file(), None);
    }

    #[test]
    fn figures_are_due_every_fifth_tick_of_an_open_page_and_never_in_parallel() {
        let mut status = FilingStatus::default();
        assert!(!status.tick_wants_figures());

        status.open_page();
        for _ in 1..INDEX_FIGURES_EVERY_TICKS {
            assert!(!status.tick_wants_figures());
        }
        assert!(status.tick_wants_figures());
        status.figures_requested();
        assert!(status.figures_in_flight());
        for _ in 0..2 * INDEX_FIGURES_EVERY_TICKS {
            assert!(!status.tick_wants_figures());
        }

        status.figures_arrived(Err("index not open".to_owned()));
        assert!(!status.figures_in_flight());
        for _ in 1..INDEX_FIGURES_EVERY_TICKS {
            assert!(!status.tick_wants_figures());
        }
        assert!(status.tick_wants_figures());
    }
}
