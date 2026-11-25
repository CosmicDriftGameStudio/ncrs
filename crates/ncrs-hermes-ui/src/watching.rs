//! What the watched sources are filed from next, and what became of the last
//! run. Pure bookkeeping: the job itself is started by `update::watching`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ncrs_filing::sorting::{PlaceOutcome, WatchedFiles, WatchedReport, WatchedSkip};
use ncrs_intake::SortSource;
use ncrs_pipeline::{ChangeOutcome, Snapshot};

/// The tick is once a second: a source that could not be filed from, or a run
/// that crashed, is tried again after this many. A drive can be away for days.
const RETRY_AFTER_TICKS: u32 = 300;

/// What one watched source came to in a run.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceRun {
    pub source: PathBuf,
    pub result: Result<WatchedReport, String>,
}

/// A watched source and the spellings of its root a path of the pipeline can
/// start with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedRoots {
    pub source: PathBuf,
    pub roots: Vec<PathBuf>,
}

/// What the user is told about a finished run.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Settled {
    /// The sources that had files filed, and how many.
    pub filed: Vec<(PathBuf, usize)>,
    /// A problem is told once, not at every retry.
    pub errors: Vec<String>,
    /// The run changed what the counts say.
    pub counts_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Problem {
    /// The source or a target cannot be reached right now: no message, the
    /// next retry finds out.
    Waiting,
    Failed(String),
}

#[derive(Debug, Default)]
pub struct Watching {
    /// `changed_sequence` of the pipeline snapshot the changes are taken from.
    seen: Option<u64>,
    /// Every watched source is looked at in full next.
    everything: bool,
    all: BTreeSet<PathBuf>,
    changed: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    running: bool,
    stalled: BTreeMap<PathBuf, u32>,
    everything_in: Option<u32>,
    problems: BTreeMap<PathBuf, Problem>,
    /// Switched to watching and not run since: what they hold is backlog.
    switched_on: BTreeSet<PathBuf>,
    /// Looking up how a folder is spelled on disk takes a moment or, on a dead
    /// mount, long: kept until the sources change.
    roots: Option<Vec<WatchedRoots>>,
}

impl Watching {
    /// A new pipeline numbers its changes from one again, and what a rule, a
    /// source or a target says may have changed: look at everything.
    pub fn reset(&mut self) {
        *self = Self {
            running: self.running,
            everything: true,
            switched_on: std::mem::take(&mut self.switched_on),
            ..Self::default()
        };
    }

    /// The rules, the sources or the targets changed, or a question was
    /// answered: the next run looks at every watched source in full.
    pub fn mark_all(&mut self) {
        self.everything = true;
        self.roots = None;
    }

    /// Remembers the sources `after` watches and `before` did not; returns the
    /// ones that are no longer watched.
    pub fn switch(&mut self, before: &[SortSource], after: &[SortSource]) -> Vec<PathBuf> {
        let watched = |sources: &[SortSource]| -> BTreeSet<PathBuf> {
            sources
                .iter()
                .filter(|source| source.watch)
                .map(|source| source.path.clone())
                .collect()
        };
        let (before, after) = (watched(before), watched(after));
        self.switched_on.extend(after.difference(&before).cloned());
        before.difference(&after).cloned().collect()
    }

    pub fn is_switched_on(&self, source: &Path) -> bool {
        self.switched_on.contains(source)
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Takes the files the pipeline finished since the last look into the
    /// sources they lie in. The first look at a pipeline, and one that lost
    /// track of changes, ask for everything.
    pub fn collect(&mut self, snapshot: &Snapshot, look_up: impl FnOnce() -> Vec<WatchedRoots>) {
        let Some(seen) = self.seen else {
            self.everything = true;
            self.seen = Some(snapshot.changed_sequence);
            return;
        };
        if seen == snapshot.changed_sequence {
            return;
        }
        let watched = self.roots.get_or_insert_with(look_up);
        match snapshot.changes_since(seen) {
            None => self.everything = true,
            Some(changes) => {
                let done = changes.filter(|change| {
                    matches!(
                        change.outcome,
                        ChangeOutcome::Finished | ChangeOutcome::FilledByAi
                    )
                });
                for change in done {
                    for source in watched.iter().filter(|source| {
                        source
                            .roots
                            .iter()
                            .any(|root| change.path.starts_with(root))
                    }) {
                        self.changed
                            .entry(source.source.clone())
                            .or_default()
                            .insert(change.path.clone());
                    }
                }
            }
        }
        self.seen = Some(snapshot.changed_sequence);
    }

    /// What the next run files from, and the run is under way from then on:
    /// no second one starts before `finish`. A source `is_busy` for stays due.
    pub fn take_due(
        &mut self,
        sources: &[SortSource],
        is_busy: impl Fn(&SortSource) -> bool,
    ) -> Vec<(SortSource, WatchedFiles)> {
        self.age_retries();
        if self.running {
            return Vec::new();
        }
        let everything = std::mem::take(&mut self.everything);
        let mut due = Vec::new();
        for source in sources.iter().filter(|source| source.watch) {
            let wants_all = everything || self.all.contains(&source.path);
            if !wants_all && !self.changed.contains_key(&source.path) {
                continue;
            }
            if is_busy(source) {
                if wants_all {
                    self.all.insert(source.path.clone());
                }
                continue;
            }
            self.all.remove(&source.path);
            let changed = self.changed.remove(&source.path).unwrap_or_default();
            let files = if wants_all {
                WatchedFiles::All
            } else {
                WatchedFiles::Changed(changed.into_iter().collect())
            };
            due.push((source.clone(), files));
        }
        self.running = !due.is_empty();
        due
    }

    fn age_retries(&mut self) {
        let mut ripe = Vec::new();
        for (source, ticks) in &mut self.stalled {
            *ticks += 1;
            if *ticks >= RETRY_AFTER_TICKS {
                ripe.push(source.clone());
            }
        }
        for source in ripe {
            self.stalled.remove(&source);
            self.all.insert(source);
            self.roots = None;
        }
        if let Some(ticks) = &mut self.everything_in {
            *ticks += 1;
            if *ticks >= RETRY_AFTER_TICKS {
                self.everything_in = None;
                self.everything = true;
            }
        }
    }

    /// The run is over. What it could not file stays for a retry; the
    /// result says what the user is told.
    pub fn finish(&mut self, result: &Result<Vec<SourceRun>, String>) -> Settled {
        self.running = false;
        let mut settled = Settled::default();
        let runs = match result {
            Ok(runs) => runs,
            Err(detail) => {
                self.everything_in = Some(0);
                settled.errors.push(detail.clone());
                return settled;
            }
        };
        for run in runs {
            let source = &run.source;
            if run.result.is_ok() {
                self.switched_on.remove(source);
            }
            match &run.result {
                Ok(WatchedReport::Done { run: report, .. }) => {
                    settled.counts_changed = true;
                    let filed = report.done();
                    if filed > 0 {
                        settled.filed.push((source.clone(), filed));
                    }
                    let still_waiting = report
                        .entries
                        .iter()
                        .flat_map(|entry| &entry.places)
                        .any(|place| matches!(place.outcome, PlaceOutcome::Waiting(_)));
                    if still_waiting {
                        self.stall(source, Problem::Waiting);
                    } else {
                        self.problems.remove(source);
                    }
                }
                Ok(WatchedReport::BacklogRecorded { .. }) => {
                    self.problems.remove(source);
                }
                Ok(WatchedReport::Skipped(WatchedSkip::Busy)) => {
                    self.all.insert(source.clone());
                }
                Ok(WatchedReport::Skipped(_)) => {
                    self.problems.remove(source);
                }
                Ok(WatchedReport::SourceWaiting(_) | WatchedReport::Waiting(_)) => {
                    self.stall(source, Problem::Waiting);
                }
                Err(detail) => {
                    if self.stall(source, Problem::Failed(detail.clone())) {
                        settled.errors.push(detail.clone());
                    }
                }
            }
        }
        settled
    }

    /// Retried later; returns whether this is a different state from the one
    /// the source was in before.
    fn stall(&mut self, source: &Path, problem: Problem) -> bool {
        self.stalled.insert(source.to_path_buf(), 0);
        let before = self.problems.insert(source.to_path_buf(), problem.clone());
        before.as_ref() != Some(&problem)
    }
}

#[cfg(test)]
mod tests {
    use ncrs_filing::sorting::{SortReport, SourceWait};
    use ncrs_intake::{LocationTransfer, OnNoText, OnSeveralTargets};
    use ncrs_pipeline::{ChangedPath, RunnerState};

    use super::*;

    fn source(path: &str) -> SortSource {
        SortSource {
            path: PathBuf::from(path),
            include: String::new(),
            exclude: String::new(),
            targets: Vec::new(),
            watch: true,
            transfer: LocationTransfer::Move,
            no_text: OnNoText::default(),
            several: OnSeveralTargets::default(),
            own_volume: None,
        }
    }

    fn watched(path: &str) -> WatchedRoots {
        WatchedRoots {
            source: PathBuf::from(path),
            roots: vec![PathBuf::from(path)],
        }
    }

    fn snapshot(changes: &[(&str, ChangeOutcome)]) -> Snapshot {
        let mut snapshot = Snapshot::off();
        snapshot.state = RunnerState::Idle;
        snapshot.changed_paths = changes
            .iter()
            .enumerate()
            .map(|(index, (path, outcome))| ChangedPath {
                sequence: index as u64 + 1,
                path: PathBuf::from(path),
                outcome: *outcome,
            })
            .collect();
        snapshot.changed_sequence = changes.len() as u64;
        snapshot
    }

    fn never_busy(_: &SortSource) -> bool {
        false
    }

    /// A first look that has been taken, so only changes are due after it.
    fn watching_after_the_start(sources: &[SortSource]) -> Watching {
        let mut watching = Watching::default();
        watching.collect(&snapshot(&[]), || vec![watched("/in")]);
        assert_eq!(watching.take_due(sources, never_busy).len(), 1);
        watching.finish(&Ok(Vec::new()));
        watching
    }

    fn paths(due: &[(SortSource, WatchedFiles)]) -> Vec<WatchedFiles> {
        due.iter().map(|(_, files)| files.clone()).collect()
    }

    fn changed(files: &[&str]) -> WatchedFiles {
        WatchedFiles::Changed(files.iter().map(PathBuf::from).collect())
    }

    #[test]
    fn the_first_look_at_a_pipeline_asks_for_everything() {
        let sources = [source("/in")];
        let mut watching = Watching::default();

        watching.collect(&snapshot(&[]), || vec![watched("/in")]);

        assert_eq!(
            paths(&watching.take_due(&sources, never_busy)),
            [WatchedFiles::All]
        );
    }

    #[test]
    fn a_source_switched_on_again_is_remembered_until_a_run_worked() {
        let on = [source("/in")];
        let off = [SortSource {
            watch: false,
            ..source("/in")
        }];
        let mut watching = Watching::default();

        assert!(watching.switch(&on, &on).is_empty());
        assert!(!watching.is_switched_on(Path::new("/in")));
        assert_eq!(watching.switch(&on, &off), [PathBuf::from("/in")]);
        assert_eq!(watching.switch(&on, &[]), [PathBuf::from("/in")]);
        assert!(watching.switch(&off, &on).is_empty());
        watching.reset();

        let ended = |result| {
            vec![SourceRun {
                source: PathBuf::from("/in"),
                result,
            }]
        };
        watching.finish(&Ok(ended(Err("broken".to_owned()))));
        assert!(watching.is_switched_on(Path::new("/in")));
        watching.finish(&Ok(ended(Ok(WatchedReport::BacklogRecorded { files: 1 }))));
        assert!(!watching.is_switched_on(Path::new("/in")));
    }

    #[test]
    fn only_files_the_pipeline_finished_in_a_watched_source_are_due() {
        let sources = [source("/in")];
        let mut watching = watching_after_the_start(&sources);

        watching.collect(
            &snapshot(&[
                ("/in/read.pdf", ChangeOutcome::Finished),
                ("/in/half.pdf", ChangeOutcome::Advanced),
                ("/in/broken.pdf", ChangeOutcome::Failed),
                ("/in/model.pdf", ChangeOutcome::FilledByAi),
                ("/elsewhere/a.pdf", ChangeOutcome::Finished),
            ]),
            || vec![watched("/in")],
        );

        assert_eq!(
            paths(&watching.take_due(&sources, never_busy)),
            [changed(&["/in/model.pdf", "/in/read.pdf"])]
        );
    }

    #[test]
    fn files_that_come_during_a_run_go_into_the_next_run_and_no_second_run_starts() {
        let sources = [source("/in")];
        let mut watching = watching_after_the_start(&sources);
        watching.collect(&snapshot(&[("/in/a.pdf", ChangeOutcome::Finished)]), || {
            vec![watched("/in")]
        });
        assert_eq!(
            paths(&watching.take_due(&sources, never_busy)),
            [changed(&["/in/a.pdf"])]
        );

        watching.collect(
            &snapshot(&[
                ("/in/a.pdf", ChangeOutcome::Finished),
                ("/in/b.pdf", ChangeOutcome::Finished),
            ]),
            || vec![watched("/in")],
        );
        assert!(watching.is_running());
        assert!(watching.take_due(&sources, never_busy).is_empty());

        watching.finish(&Ok(Vec::new()));
        assert_eq!(
            paths(&watching.take_due(&sources, never_busy)),
            [changed(&["/in/b.pdf"])]
        );
    }

    #[test]
    fn a_source_the_filing_tab_works_on_is_skipped_and_stays_due() {
        let sources = [source("/in")];
        let mut watching = watching_after_the_start(&sources);
        watching.collect(&snapshot(&[("/in/a.pdf", ChangeOutcome::Finished)]), || {
            vec![watched("/in")]
        });

        let busy = |source: &SortSource| source.path == Path::new("/in");
        assert!(watching.take_due(&sources, busy).is_empty());
        assert!(!watching.is_running());

        assert_eq!(
            paths(&watching.take_due(&sources, never_busy)),
            [changed(&["/in/a.pdf"])]
        );
    }

    #[test]
    fn everything_marked_stays_due_for_a_busy_source_and_only_for_it() {
        let sources = [source("/in"), source("/other")];
        let mut watching = watching_after_the_start(&sources[..1]);

        watching.mark_all();
        let busy = |source: &SortSource| source.path == Path::new("/in");
        let due = watching.take_due(&sources, busy);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0.path, PathBuf::from("/other"));
        watching.finish(&Ok(Vec::new()));

        let due = watching.take_due(&sources, never_busy);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0.path, PathBuf::from("/in"));
        assert_eq!(due[0].1, WatchedFiles::All);
    }

    #[test]
    fn a_rule_source_or_target_change_asks_for_everything_again() {
        let sources = [source("/in")];
        let mut watching = watching_after_the_start(&sources);
        assert!(watching.take_due(&sources, never_busy).is_empty());

        watching.mark_all();

        assert_eq!(
            paths(&watching.take_due(&sources, never_busy)),
            [WatchedFiles::All]
        );
    }

    #[test]
    fn a_pipeline_that_lost_track_of_changes_asks_for_everything() {
        let sources = [source("/in")];
        let mut watching = watching_after_the_start(&sources);
        let mut lost = snapshot(&[("/in/a.pdf", ChangeOutcome::Finished)]);
        lost.changed_sequence_floor = 1;
        watching.seen = Some(0);

        watching.collect(&lost, || vec![watched("/in")]);

        assert_eq!(
            paths(&watching.take_due(&sources, never_busy)),
            [WatchedFiles::All]
        );
    }

    #[test]
    fn a_source_that_cannot_be_reached_is_tried_again_later_and_told_nothing() {
        let sources = [source("/in")];
        let mut watching = watching_after_the_start(&sources);
        watching.mark_all();
        assert_eq!(watching.take_due(&sources, never_busy).len(), 1);

        let settled = watching.finish(&Ok(vec![SourceRun {
            source: PathBuf::from("/in"),
            result: Ok(WatchedReport::SourceWaiting(SourceWait::NotMounted)),
        }]));

        assert_eq!(settled, Settled::default());
        for _ in 0..RETRY_AFTER_TICKS - 1 {
            assert!(watching.take_due(&sources, never_busy).is_empty());
        }
        assert_eq!(
            paths(&watching.take_due(&sources, never_busy)),
            [WatchedFiles::All]
        );
    }

    #[test]
    fn a_failure_is_told_once_however_often_it_comes_back() {
        let sources = [source("/in")];
        let mut watching = watching_after_the_start(&sources);
        let failed = || {
            Ok(vec![SourceRun {
                source: PathBuf::from("/in"),
                result: Err("index is locked".to_owned()),
            }])
        };

        watching.mark_all();
        watching.take_due(&sources, never_busy);
        let first = watching.finish(&failed());
        watching.mark_all();
        watching.take_due(&sources, never_busy);
        let second = watching.finish(&failed());

        assert_eq!(first.errors, ["index is locked"]);
        assert!(second.errors.is_empty());
    }

    #[test]
    fn a_done_run_counts_what_it_filed_and_says_the_counts_changed() {
        let sources = [source("/in")];
        let mut watching = watching_after_the_start(&sources);
        watching.mark_all();
        watching.take_due(&sources, never_busy);

        let settled = watching.finish(&Ok(vec![SourceRun {
            source: PathBuf::from("/in"),
            result: Ok(WatchedReport::Done {
                run: SortReport::default(),
                asked: 2,
            }),
        }]));

        assert!(settled.counts_changed);
        assert!(settled.filed.is_empty());
        assert!(!watching.is_running());
    }

    #[test]
    fn a_crashed_run_is_told_and_everything_is_looked_at_again_later() {
        let sources = [source("/in")];
        let mut watching = watching_after_the_start(&sources);
        watching.mark_all();
        watching.take_due(&sources, never_busy);

        let settled = watching.finish(&Err("panicked".to_owned()));

        assert_eq!(settled.errors, ["panicked"]);
        for _ in 0..RETRY_AFTER_TICKS - 1 {
            assert!(watching.take_due(&sources, never_busy).is_empty());
        }
        assert_eq!(watching.take_due(&sources, never_busy).len(), 1);
    }
}
