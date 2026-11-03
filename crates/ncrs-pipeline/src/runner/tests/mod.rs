#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::HashSet;
use std::fs;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::UNIX_EPOCH;

use ncrs_index::{DocumentStatus, Index};
use ncrs_intake::{
    Extracted, LazyOcrText, LocationMode, PageImage, TextChain, TextError, TextSource,
};
use tempfile::TempDir;

use super::*;
use crate::step::IndexAccess;
use crate::steps::{intake_steps, OwnDetails};

const HANG_GUARD: Duration = Duration::from_secs(30);
const INVOICE: &[u8] = b"Invoice 4711 from Company A";
const REMINDER: &[u8] = b"Reminder 12 from Agency B";
const CONTRACT: &[u8] = b"Contract 9 from Company A";

fn cores(count: usize) -> NonZeroUsize {
    NonZeroUsize::new(count).unwrap()
}

fn workers(count: usize) -> Load {
    Load::Workers(cores(count))
}

fn epoch() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

#[derive(Debug)]
struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        epoch()
    }
}

/// Moves on with every reading, so files look stable on the second look.
#[derive(Debug)]
struct SteppingClock {
    now: Mutex<SystemTime>,
}

impl Clock for SteppingClock {
    fn now(&self) -> SystemTime {
        let mut now = self.now.lock().unwrap();
        *now += Duration::from_secs(3);
        *now
    }
}

#[derive(Debug, Default)]
struct GateState {
    everything: bool,
    paths: HashSet<PathBuf>,
}

#[derive(Debug, Default)]
struct Gate {
    state: Mutex<GateState>,
    opened: Condvar,
}

impl Gate {
    fn release(&self, path: &Path) {
        self.state.lock().unwrap().paths.insert(path.to_path_buf());
        self.opened.notify_all();
    }

    fn release_everything(&self) {
        self.state.lock().unwrap().everything = true;
        self.opened.notify_all();
    }

    fn wait_until_released(&self, path: &Path) {
        let deadline = Instant::now() + HANG_GUARD;
        let mut state = self.state.lock().unwrap();
        while !state.everything && !state.paths.contains(path) {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "{} was never released", path.display());
            state = self.opened.wait_timeout(state, left).unwrap().0;
        }
    }
}

type Decide = fn(&Path) -> Result<StepOutcome, String>;

fn continue_always(_path: &Path) -> Result<StepOutcome, String> {
    Ok(StepOutcome::Continue)
}

fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|name| name.to_str()).unwrap()
}

/// Reports every start and can hold the item until the test lets it go.
#[derive(Debug)]
struct ProbeStep {
    name: &'static str,
    started: Mutex<Sender<PathBuf>>,
    gate: Option<Arc<Gate>>,
    decide: Decide,
}

impl Step for ProbeStep {
    fn name(&self) -> &'static str {
        self.name
    }

    fn run(&self, item: &mut Item, _now: SystemTime) -> Result<StepOutcome, String> {
        let _ = self
            .started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(item.path.clone());
        if let Some(gate) = &self.gate {
            gate.wait_until_released(&item.path);
        }
        (self.decide)(&item.path)
    }
}

fn probe(
    name: &'static str,
    gate: Option<&Arc<Gate>>,
    decide: Decide,
) -> (Box<dyn Step>, Receiver<PathBuf>) {
    let (sender, receiver) = mpsc::channel();
    let step = ProbeStep {
        name,
        started: Mutex::new(sender),
        gate: gate.map(Arc::clone),
        decide,
    };
    (Box::new(step), receiver)
}

fn next_started(receiver: &Receiver<PathBuf>) -> PathBuf {
    receiver.recv_timeout(HANG_GUARD).unwrap()
}

fn put(location: &Location, name: &str, bytes: &[u8]) -> PathBuf {
    let path = location.path.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn location(dir: &TempDir, mode: LocationMode) -> Location {
    Location::new(dir.path(), mode, false).unwrap()
}

fn settings(load: Load) -> Settings {
    Settings {
        retry_after: Duration::ZERO,
        rescan_every: None,
        ..Settings::new(load, cores(8))
    }
}

fn busy_count(report: &Snapshot) -> usize {
    report
        .workers
        .iter()
        .filter(|worker| worker.busy.is_some())
        .count()
}

fn nothing_busy(report: &Snapshot) -> bool {
    busy_count(report) == 0
}

fn is_idle(snapshot: &Snapshot) -> bool {
    snapshot.state == RunnerState::Idle
}

/// A location with files `file-1.pdf` .. `file-N.pdf`.
fn numbered_files(count: usize) -> (TempDir, Location) {
    let dir = TempDir::new().unwrap();
    let location = location(&dir, LocationMode::Archive);
    for number in 1..=count {
        put(&location, &format!("file-{number}.pdf"), b"x");
    }
    (dir, location)
}

fn start(
    load: Load,
    steps: Vec<Box<dyn Step>>,
    location: &Location,
    clock: Arc<dyn Clock>,
) -> Runner {
    Runner::start(settings(load), steps, vec![location.clone()], clock)
}

#[test]
fn a_changed_load_adds_workers_and_lets_the_surplus_finish_its_file() {
    let (_dir, location) = numbered_files(4);
    let gate = Arc::new(Gate::default());
    let (step, started) = probe("gated", Some(&gate), continue_always);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));

    let first = next_started(&started);
    runner.wait_for("one busy worker", |snapshot| busy_count(snapshot) == 1);
    runner.set_load(workers(3));
    let report = runner.wait_for("three busy workers", |snapshot| busy_count(snapshot) == 3);
    assert_eq!(report.worker_target, 3);
    let running = [first, next_started(&started), next_started(&started)];
    started.try_recv().unwrap_err();

    runner.set_load(workers(1));
    assert_eq!(runner.snapshot().worker_target, 1);
    for path in &running {
        gate.release(path);
    }
    let report_after = runner.wait_for("surplus workers gone", |snapshot| {
        snapshot.workers.len() == 1 && snapshot.steps[0].done == 3
    });
    assert_eq!(report_after.steps[0].failed, 0);

    let fourth = next_started(&started);
    assert!(!running.contains(&fourth));
    assert!(busy_count(&runner.snapshot()) <= 1);
    gate.release(&fourth);
    let report_last = runner.wait_for("all four done", is_idle);
    assert_eq!(report_last.steps[0].done, 4);
}

#[test]
fn pause_lets_running_files_finish_and_starts_no_new_ones() {
    let (_dir, location) = numbered_files(4);
    let gate = Arc::new(Gate::default());
    let (step, started) = probe("gated", Some(&gate), continue_always);
    let runner = start(workers(2), vec![step], &location, Arc::new(FixedClock));

    let running = [next_started(&started), next_started(&started)];
    runner.wait_for("two busy workers", |snapshot| busy_count(snapshot) == 2);
    runner.pause();
    for path in &running {
        gate.release(path);
    }
    let report = runner.wait_for("the running files done", |snapshot| {
        nothing_busy(snapshot) && snapshot.steps[0].done == 2
    });
    assert_eq!(report.state, RunnerState::Paused);
    assert_eq!(report.steps[0].waiting, 2);
    started.try_recv().unwrap_err();

    runner.resume();
    gate.release_everything();
    let report_after = runner.wait_for("the rest done", is_idle);
    assert_eq!(report_after.steps[0].done, 4);
}

#[test]
fn a_paused_runner_still_takes_a_started_file_through_its_remaining_steps() {
    let (_dir, location) = numbered_files(2);
    let gate = Arc::new(Gate::default());
    let (first, first_started) = probe("first", Some(&gate), continue_always);
    let (second, second_started) = probe("second", None, continue_always);
    let runner = start(
        workers(1),
        vec![first, second],
        &location,
        Arc::new(FixedClock),
    );

    let file = next_started(&first_started);
    runner.pause();
    gate.release(&file);
    assert_eq!(next_started(&second_started), file);
    let report = runner.wait_for("the file through both steps", |snapshot| {
        nothing_busy(snapshot) && snapshot.steps[1].done == 1
    });
    assert_eq!(report.state, RunnerState::Paused);
    first_started.try_recv().unwrap_err();

    runner.resume();
    gate.release_everything();
    let report_after = runner.wait_for("both files done", is_idle);
    assert_eq!(report_after.steps[1].done, 2);
}

fn fail_the_second_file(path: &Path) -> Result<StepOutcome, String> {
    match file_name(path) {
        "file-2.pdf" => Err("not a readable document".to_owned()),
        _ => Ok(StepOutcome::Continue),
    }
}

#[test]
fn a_failing_file_is_counted_and_listed_and_the_others_carry_on() {
    let (_dir, location) = numbered_files(3);
    let (step, _started) = probe("probe", None, fail_the_second_file);
    let runner = start(workers(2), vec![step], &location, Arc::new(FixedClock));

    let report = runner.wait_for("all three handled", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].done + snapshot.steps[0].failed == 3
    });

    let counts = &report.steps[0];
    assert_eq!((counts.done, counts.failed, counts.unchanged), (2, 1, 0));
    assert_eq!(report.errors.len(), 1);
    let error = &report.errors[0];
    assert_eq!(error.file, location.path.join("file-2.pdf"));
    assert_eq!(error.task, Task::Step("probe"));
    assert_eq!(error.message, "not a readable document");
    assert_eq!(error.at, epoch());
    assert_eq!(report.documents_per_minute[0], 2);
    assert_eq!(report.documents_per_minute.len(), RATE_MINUTES);
    assert_eq!(report.left_pipeline, 3);
    assert_eq!(report.open_files(), 0);
}

/// FSEvents also reports files written shortly before the watch started,
/// so a file the scan already took can be reported again unchanged.
#[test]
fn a_file_reported_again_unchanged_is_not_taken_again() {
    let (_dir, location) = numbered_files(3);
    let (step, _started) = probe("probe", None, fail_the_second_file);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));
    runner.wait_for("all three handled", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].done + snapshot.steps[0].failed == 3
    });

    for name in ["file-1.pdf", "file-2.pdf"] {
        runner
            .shared
            .on_watch_event(WatchEvent::File(location.path.join(name)));
    }
    let report = runner.wait_for("the repeated reports handled", is_idle);

    let counts = &report.steps[0];
    assert_eq!((counts.done, counts.failed, counts.unchanged), (2, 1, 0));
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.left_pipeline, 3);
}

#[test]
fn a_failed_file_that_changes_is_tried_again() {
    let (_dir, location) = numbered_files(3);
    let (step, _started) = probe("probe", None, fail_the_second_file);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));
    runner.wait_for("all three handled", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].done + snapshot.steps[0].failed == 3
    });

    // Moved in whole, so no report can see the file half written.
    let staging = TempDir::new().unwrap();
    let changed = staging.path().join("file-2.pdf");
    fs::write(&changed, b"a longer body").unwrap();
    let second = location.path.join("file-2.pdf");
    fs::rename(&changed, &second).unwrap();
    runner.shared.on_watch_event(WatchEvent::File(second));
    let report = runner.wait_for("the changed file tried again", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].failed == 2
    });

    assert_eq!(report.steps[0].done, 2);
    assert_eq!(report.errors.len(), 2);
}

#[test]
fn a_file_that_is_gone_is_forgotten_by_the_repeat_check() {
    let (_dir, location) = numbered_files(2);
    let (step, _started) = probe("probe", None, continue_always);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));
    runner.wait_for("both handled", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].done == 2
    });
    let gone = location.path.join("file-1.pdf");
    assert!(runner
        .shared
        .update(|state| state.taken.contains_key(&gone)));

    fs::remove_file(&gone).unwrap();
    runner.shared.on_watch_event(WatchEvent::File(gone.clone()));

    assert!(!runner
        .shared
        .update(|state| state.taken.contains_key(&gone)));
}

#[cfg(unix)]
#[test]
fn a_file_replaced_by_one_of_the_same_size_and_mtime_looks_changed() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("letter.pdf");
    let mtime = epoch() + Duration::from_secs(1_000_000);
    let stamp = |path: &Path, body: &[u8]| {
        fs::write(path, body).unwrap();
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    };
    stamp(&target, b"first");
    let before = Fingerprint::of(&target).unwrap();
    let replacement = dir.path().join("staged.pdf");
    stamp(&replacement, b"other");
    fs::rename(&replacement, &target).unwrap();

    let after = Fingerprint::of(&target).unwrap();

    assert_ne!(before, after);
}

#[derive(Debug)]
struct UnsettledAfterScan;

impl Step for UnsettledAfterScan {
    fn name(&self) -> &'static str {
        "settle"
    }

    fn run(&self, _item: &mut Item, _now: SystemTime) -> Result<StepOutcome, String> {
        Ok(StepOutcome::Continue)
    }

    fn after_scan(&self, location: &Location) -> Result<Vec<PathBuf>, String> {
        Ok(vec![location.path.join("broken.pdf")])
    }
}

#[test]
fn paths_a_step_could_not_settle_after_a_scan_are_listed_as_errors() {
    let (_dir, location) = numbered_files(1);
    let runner = start(
        workers(1),
        vec![Box::new(UnsettledAfterScan)],
        &location,
        Arc::new(FixedClock),
    );

    let report = runner.wait_for("the scan settled", |snapshot| {
        is_idle(snapshot) && !snapshot.errors.is_empty()
    });

    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].file, location.path.join("broken.pdf"));
}

fn leave_the_second_file_unchanged(path: &Path) -> Result<StepOutcome, String> {
    match file_name(path) {
        "file-2.pdf" => Ok(StepOutcome::Unchanged),
        _ => Ok(StepOutcome::Finished),
    }
}

#[test]
fn an_unchanged_file_is_neither_done_nor_a_document_in_the_rate() {
    let (_dir, location) = numbered_files(2);
    let (step, _started) = probe("probe", None, leave_the_second_file_unchanged);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));

    let report = runner.wait_for("both files handled", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].done + snapshot.steps[0].unchanged == 2
    });

    let counts = &report.steps[0];
    assert_eq!((counts.done, counts.failed, counts.unchanged), (1, 0, 1));
    assert_eq!(report.documents_per_minute[0], 1);
    assert!(report.errors.is_empty());
    assert_eq!(report.left_pipeline, 2);
    assert_eq!(report.open_files(), 0);
}

#[test]
fn open_files_count_waiting_and_step_work_but_not_scans() {
    let busy = |task| WorkerSnapshot {
        id: 0,
        busy: Some(Busy {
            file: PathBuf::from("/tmp/inbox/company-a-invoice.pdf"),
            task,
            started_at: epoch(),
        }),
    };
    let step = |waiting| StepSnapshot {
        name: "probe",
        waiting,
        done: 0,
        failed: 0,
        unchanged: 0,
    };
    let mut snapshot = Snapshot::off();
    snapshot.steps = vec![step(3), step(2)];
    snapshot.workers = vec![
        busy(Task::Scan),
        busy(Task::Watch),
        busy(Task::Workers),
        busy(Task::Step("probe")),
        WorkerSnapshot { id: 4, busy: None },
    ];
    assert_eq!(snapshot.open_files(), 6);
}

fn panic_on_the_second_file(path: &Path) -> Result<StepOutcome, String> {
    if file_name(path) == "file-2.pdf" {
        explode();
    }
    Ok(StepOutcome::Continue)
}

fn explode() {
    panic!("broken step");
}

#[test]
fn a_panicking_step_is_a_failed_file_and_the_worker_carries_on() {
    let (_dir, location) = numbered_files(3);
    let (step, _started) = probe("probe", None, panic_on_the_second_file);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));

    let report = runner.wait_for("all three handled", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].done + snapshot.steps[0].failed == 3
    });

    assert_eq!(report.steps[0].done, 2);
    assert_eq!(report.steps[0].failed, 1);
    assert_eq!(report.errors[0].message, PANIC_MESSAGE);
    assert_eq!(report.errors[0].file, location.path.join("file-2.pdf"));
    assert_eq!(report.workers.len(), 1);
}

#[derive(Debug)]
struct SharedIndex(Arc<Mutex<Index>>);

impl IndexAccess for SharedIndex {
    fn with_index(&self, work: &mut dyn FnMut(&mut Index)) -> bool {
        work(&mut self.0.lock().unwrap());
        true
    }
}

struct IndexFixture {
    _dir: TempDir,
    index: Arc<Mutex<Index>>,
}

impl IndexFixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        Self {
            _dir: dir,
            index: Arc::new(Mutex::new(index)),
        }
    }

    fn steps(&self) -> Vec<Box<dyn Step>> {
        self.steps_reading(TextChain::new())
    }

    fn steps_reading(&self, chain: TextChain) -> Vec<Box<dyn Step>> {
        intake_steps(
            Arc::new(SharedIndex(Arc::clone(&self.index))),
            Arc::new(chain),
            None,
            OwnDetails::default(),
        )
    }
}

fn stepping_clock() -> Arc<dyn Clock> {
    Arc::new(SteppingClock {
        now: Mutex::new(epoch()),
    })
}

#[test]
fn the_change_log_numbers_entries_and_tells_a_slow_consumer_it_missed_some() {
    let mut log = ChangeLog::default();
    for number in 0..MAX_CHANGED_PATHS + 3 {
        log.push(
            Path::new(&format!("/in/{number}.pdf")),
            ChangeOutcome::Finished,
        );
    }
    log.push(Path::new("/in/514.pdf"), ChangeOutcome::Finished);
    let mut snapshot = Snapshot::off();
    snapshot.changed_paths = log.entries.iter().cloned().collect();
    snapshot.changed_sequence = log.latest;
    snapshot.changed_sequence_floor = log.floor;

    assert_eq!(snapshot.changed_paths.len(), MAX_CHANGED_PATHS);
    assert_eq!(snapshot.changed_sequence, (MAX_CHANGED_PATHS + 3) as u64);
    assert_eq!(snapshot.changed_sequence_floor, 3);
    assert!(snapshot.changes_since(2).is_none());
    let recent: Vec<_> = snapshot
        .changes_since(snapshot.changed_sequence - 2)
        .unwrap()
        .collect();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[1].path, Path::new("/in/514.pdf"));
    assert_eq!(
        snapshot.changes_since(3).unwrap().count(),
        MAX_CHANGED_PATHS
    );
    assert!(snapshot
        .changes_since(snapshot.changed_sequence + 1)
        .is_none());
}

#[test]
fn files_that_changed_are_listed_and_files_found_unchanged_are_not() {
    let (_dir, location) = numbered_files(2);
    let (step, _started) = probe("probe", None, leave_the_second_file_unchanged);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));

    let report = runner.wait_for("both files handled", |snapshot| {
        is_idle(snapshot) && snapshot.left_pipeline == 2
    });

    let paths: Vec<_> = report
        .changed_paths
        .iter()
        .map(|change| change.path.clone())
        .collect();
    assert_eq!(paths, vec![location.path.join("file-1.pdf")]);
    assert_eq!(report.changed_sequence, 1);
    assert_eq!(report.changed_sequence_floor, 0);
}

#[test]
fn a_failed_file_is_listed_as_changed() {
    let (_dir, location) = numbered_files(3);
    let (step, _started) = probe("probe", None, fail_the_second_file);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));

    let report = runner.wait_for("all handled", |snapshot| {
        is_idle(snapshot) && snapshot.left_pipeline == 3
    });

    assert!(report
        .changed_paths
        .iter()
        .any(|change| change.path == location.path.join("file-2.pdf")));
}

fn leave_every_file_unchanged(_path: &Path) -> Result<StepOutcome, String> {
    Ok(StepOutcome::Unchanged)
}

#[test]
fn a_file_processed_again_is_listed_even_when_nothing_changed() {
    let (_dir, location) = numbered_files(1);
    let (step, _started) = probe("probe", None, leave_every_file_unchanged);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));
    let path = location.path.join("file-1.pdf");
    let first = runner.wait_for("the file seen", |snapshot| {
        is_idle(snapshot) && snapshot.left_pipeline == 1
    });
    assert!(first.changed_paths.is_empty(), "a rescan is not announced");

    assert!(runner.process_again(&path));

    let report = runner.wait_for("the file handled again", |snapshot| {
        is_idle(snapshot) && snapshot.left_pipeline == 2
    });
    let outcomes: Vec<_> = report
        .changed_paths
        .iter()
        .map(|change| (change.path.clone(), change.outcome))
        .collect();
    assert_eq!(outcomes, vec![(path, ChangeOutcome::Unchanged)]);
}

#[test]
fn a_file_processed_again_without_a_matching_rule_ends_in_review_and_is_announced() {
    let inbox_dir = TempDir::new().unwrap();
    let inbox = location(&inbox_dir, LocationMode::Inbox);
    let path = put(&inbox, "invoice.pdf", INVOICE);
    let fixture = IndexFixture::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let runner = reviewed_once(&inbox, &fixture, &reads);
    let sequence_before = runner.snapshot().changed_sequence;

    assert!(runner.process_again(&path));

    let report = runner.wait_for("the file passed again", |snapshot| {
        is_idle(snapshot) && snapshot.steps[3].done == 2
    });
    assert!(nothing_busy(&report));
    assert_eq!(
        first_document_status(&fixture, &inbox),
        DocumentStatus::InReview
    );
    assert_eq!(
        fixture.index.lock().unwrap().open_reviews().unwrap().len(),
        1
    );
    let last = report.changed_paths.last().unwrap();
    assert_eq!(last.path, path);
    assert_eq!(last.outcome, ChangeOutcome::Finished);
    assert!(last.sequence > sequence_before);
}

#[test]
fn a_step_in_the_middle_and_the_end_of_one_file_are_both_listed() {
    let mut log = ChangeLog::default();
    let path = Path::new("/in/a.pdf");
    log.push(path, ChangeOutcome::Advanced);
    log.push(path, ChangeOutcome::Advanced);
    log.push(path, ChangeOutcome::Finished);
    let outcomes: Vec<_> = log.entries.iter().map(|change| change.outcome).collect();
    assert_eq!(
        outcomes,
        vec![ChangeOutcome::Advanced, ChangeOutcome::Finished]
    );
}

#[test]
fn stopping_ends_the_workers_and_can_be_repeated() {
    let (_dir, location) = numbered_files(1);
    let (step, _started) = probe("probe", None, continue_always);
    let mut runner = start(workers(2), vec![step], &location, Arc::new(FixedClock));
    runner.wait_for("the file handled", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].done == 1
    });

    runner.stop();

    let report = runner.snapshot();
    assert_eq!(report.state, RunnerState::Off);
    assert!(report.workers.is_empty());
    runner.stop();
    drop(runner);
    assert_eq!(Snapshot::off().state, RunnerState::Off);
}

#[test]
fn dropping_a_runner_right_after_the_start_does_not_hang() {
    let (_dir, location) = numbered_files(3);
    let (step, _started) = probe("probe", None, continue_always);
    drop(start(
        workers(2),
        vec![step],
        &location,
        Arc::new(FixedClock),
    ));
}

#[path = "runner_archive_tests.rs"]
mod archive_fields;

mod review_tests;
use review_tests::{archive_scan, first_document_status, reviewed_once, FixedText};
mod rule_tests;

fn handled_everything(runner: &Runner, count: usize) -> Snapshot {
    runner.wait_for("every file handled", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].done == count
    })
}

#[test]
fn a_report_while_the_file_waits_updates_the_state_a_repeat_is_matched_against() {
    let (_dir, location) = numbered_files(1);
    let (step, _started) = probe("probe", None, continue_always);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));
    handled_everything(&runner, 1);
    runner.pause();
    let path = put(&location, "growing.pdf", b"half");
    let half = Fingerprint::of(&path);
    let queued_first = runner
        .shared
        .update(|state| state.queue_first_step(&location, &path, None, half));
    fs::write(&path, b"the whole file").unwrap();
    let whole = Fingerprint::of(&path);
    let queued_again = runner
        .shared
        .update(|state| state.queue_first_step(&location, &path, None, whole));

    let waiting = runner.shared.update(|state| {
        state
            .first_step
            .iter()
            .find(|queued| queued.item.path == path)
            .map(|queued| queued.fingerprint)
    });

    assert!(queued_first);
    assert!(!queued_again);
    assert_ne!(half, whole);
    assert_eq!(waiting, Some(whole));
}

static GROWN_WHILE_NOT_READY: AtomicUsize = AtomicUsize::new(0);

fn not_ready_once_while_the_file_grows(path: &Path) -> Result<StepOutcome, String> {
    if GROWN_WHILE_NOT_READY.fetch_add(1, Ordering::SeqCst) == 0 {
        fs::write(path, b"a body that grew while the step was not ready").unwrap();
        return Ok(StepOutcome::NotReady);
    }
    Ok(StepOutcome::Continue)
}

#[test]
fn a_file_retried_after_not_ready_is_matched_against_its_state_at_the_retry() {
    let (_dir, location) = numbered_files(1);
    let (step, _started) = probe("probe", None, not_ready_once_while_the_file_grows);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));
    handled_everything(&runner, 1);
    let path = location.path.join("file-1.pdf");

    let taken = runner
        .shared
        .update(|state| state.taken.get(&path).copied());

    assert_eq!(taken, Fingerprint::of(&path));
}

#[test]
fn a_scan_skips_the_files_already_taken_since_the_scan_started() {
    let (_dir, location) = numbered_files(3);
    let (step, _started) = probe("probe", None, continue_always);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));
    handled_everything(&runner, 3);

    runner.shared.run_scan(usize::MAX, &location);
    let report = runner.wait_for("the walk is over", is_idle);

    assert_eq!(report.steps[0].done, 3);
}

#[test]
fn a_new_scan_offers_every_file_again() {
    let (_dir, location) = numbered_files(3);
    let (step, _started) = probe("probe", None, continue_always);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));
    handled_everything(&runner, 3);

    runner.shared.update(|state| state.queue_scan(&location));
    runner.shared.work_ready.notify_all();
    let report = runner.wait_for("every file taken again", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].done == 6
    });

    assert_eq!(report.steps[0].done, 6);
}

#[test]
fn a_watch_report_before_the_watcher_is_stored_is_left_to_the_scan() {
    let (_dir, location) = numbered_files(1);
    let (step, _started) = probe("probe", None, continue_always);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));
    handled_everything(&runner, 1);
    let watcher = runner.shared.update(|state| state.watcher.take());
    let path = put(&location, "early.pdf", b"early");

    runner.shared.on_watch_event(WatchEvent::File(path.clone()));

    assert!(runner.shared.update(|state| state.first_step.is_empty()));
    runner.shared.update(|state| state.watcher = watcher);
    runner.shared.update(|state| state.queue_scan(&location));
    runner.shared.work_ready.notify_all();
    let report = runner.wait_for("the scan took the file once", |snapshot| {
        is_idle(snapshot) && snapshot.steps[0].done == 3
    });
    assert_eq!(report.steps[0].done, 3);
}

#[test]
fn the_rate_forgets_entries_dated_after_a_clock_that_jumped_back() {
    let (_dir, location) = numbered_files(1);
    let (step, _started) = probe("probe", None, continue_always);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));
    handled_everything(&runner, 1);

    let kept = runner.shared.update(|state| {
        state.rate.clear();
        state.record_document(epoch() + Duration::from_secs(10_000));
        state.record_document(epoch());
        state.rate.len()
    });

    assert_eq!(kept, 1);
}

#[cfg(unix)]
#[test]
fn an_unreadable_folder_is_reported_with_the_reason() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = TempDir::new().unwrap();
    let location = location(&dir, LocationMode::Archive);
    let locked = location.path.join("locked");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join("inside.pdf"), b"x").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let (step, _started) = probe("probe", None, continue_always);
    let runner = start(workers(1), vec![step], &location, Arc::new(FixedClock));

    let report = runner.wait_for("the scan reported the folder", |snapshot| {
        is_idle(snapshot) && !snapshot.errors.is_empty()
    });
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    let entry = &report.errors[0];
    assert_eq!(entry.file, locked);
    assert!(
        entry.message.to_lowercase().contains("denied"),
        "{}",
        entry.message
    );
}
