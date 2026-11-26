//! Carrying the plan out, off the UI thread, and what the user reads about it.

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use ncrs_filing::execute::{SkipReason, TrashItem};
use ncrs_filing::sorting::{
    answer, leave, run, EntryReport, PlaceOutcome, RunError, RunRequest, SortReport, SortingAccess,
    WaitReason,
};
use ncrs_fs::transfer::{Counting, Progress};
use ncrs_i18n::{Language, Msg};
use ncrs_index::DocumentId;
use ncrs_intake::{canonical_path, LocationTransfer, SortSource};

use crate::state::Change;
use crate::texts::{say, wait_reason};

/// How far a run is, shared with the thread that carries it out.
#[derive(Debug, Default)]
pub struct RunProgress {
    done: AtomicUsize,
    total: AtomicUsize,
    stop: AtomicBool,
}

impl RunProgress {
    pub fn new(total: usize) -> Self {
        Self {
            total: AtomicUsize::new(total),
            ..Self::default()
        }
    }

    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }

    /// The file being carried out is finished, the ones after it are left.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

struct Shared(Arc<RunProgress>);

impl Progress for Shared {
    fn advanced(&mut self, done: usize, total: usize) -> io::Result<()> {
        self.0.done.store(done, Ordering::Relaxed);
        self.0.total.store(total, Ordering::Relaxed);
        if self.0.is_stopped() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        Ok(())
    }
}

/// What a run or an answer did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Carried {
    pub transfer: LocationTransfer,
    pub report: SortReport,
    pub trashed: usize,
    /// Why the duplicates are still there.
    pub trash_problem: Option<String>,
}

/// The duplicates of the source, with the name the history gives their run.
#[derive(Debug, Clone)]
pub struct Duplicates {
    pub name: String,
    pub items: Vec<TrashItem>,
}

pub fn carry_out(
    access: &SortingAccess,
    request: &RunRequest,
    duplicates: &Duplicates,
    progress: Arc<RunProgress>,
    lang: Language,
) -> Result<Carried, String> {
    let report = run(access, request, &mut Shared(Arc::clone(&progress)))
        .map_err(|err| run_error_text(lang, access, &err))?;
    let mut carried = Carried {
        transfer: request.transfer,
        report,
        ..Carried::default()
    };
    if !duplicates.items.is_empty() && !progress.is_stopped() {
        let root =
            canonical_path(&request.source.path).unwrap_or_else(|_| request.source.path.clone());
        match access.trash_duplicates(&duplicates.name, &root, &duplicates.items) {
            Ok(trash_report) => carried.trashed = trash_report.trashed,
            Err(reason) => carried.trash_problem = Some(reason),
        }
    }
    Ok(carried)
}

pub fn carry_answer(
    access: &SortingAccess,
    source: &SortSource,
    document: DocumentId,
    change: Change,
    name: &str,
    lang: Language,
) -> Result<Carried, String> {
    let report = match change {
        Change::Stay => {
            leave(access, document)?;
            SortReport::default()
        }
        Change::Places(places) => answer(access, source, document, places, name, &mut Counting)
            .map_err(|err| run_error_text(lang, access, &err))?,
    };
    Ok(Carried {
        transfer: source.transfer,
        report,
        ..Carried::default()
    })
}

fn run_error_text(lang: Language, access: &SortingAccess, err: &RunError) -> String {
    match err {
        RunError::Source(_) => lang.text(Msg::FilingViewErrorSource).to_owned(),
        RunError::Index(reason) => reason.clone(),
        RunError::Waiting(waiting) => {
            let targets = access.configured_targets();
            let lines: Vec<String> = waiting
                .iter()
                .map(|(id, reason)| {
                    let number = targets
                        .iter()
                        .position(|target| target.id == *id)
                        .map_or_else(|| id.to_string(), |index| format!("T{}", index + 1));
                    waiting_line(lang, &number, *reason)
                })
                .collect();
            lines.join(" ")
        }
    }
}

pub fn waiting_line(lang: Language, target: &str, reason: WaitReason) -> String {
    say(
        lang,
        Msg::FilingViewTargetWaiting,
        &[
            ("target", target),
            ("reason", lang.text(wait_reason(reason))),
        ],
    )
}

/// One line for every place a file did not reach.
pub fn problems(lang: Language, report: &SortReport) -> Vec<String> {
    report
        .entries
        .iter()
        .flat_map(|entry| {
            entry
                .places
                .iter()
                .filter_map(|place| problem(lang, entry, &place.outcome))
        })
        .collect()
}

fn problem(lang: Language, entry: &EntryReport, outcome: &PlaceOutcome) -> Option<String> {
    let file = file_name(&entry.path);
    let (msg, reason) = match outcome {
        // What the user stopped is told by the figure of what is left.
        PlaceOutcome::Done | PlaceOutcome::Skipped(SkipReason::Cancelled) => return None,
        PlaceOutcome::Waiting(reason) => (
            Msg::FilingViewProblemWaiting,
            lang.text(wait_reason(*reason)).to_owned(),
        ),
        PlaceOutcome::BadPlace
        | PlaceOutcome::Skipped(SkipReason::OutsideLocation | SkipReason::LeavesLocation) => {
            (Msg::FilingViewProblemBadPlace, String::new())
        }
        PlaceOutcome::Skipped(SkipReason::SourceChanged | SkipReason::SourceMissing) => {
            (Msg::FilingViewProblemChanged, String::new())
        }
        PlaceOutcome::Skipped(SkipReason::TargetExists) => {
            (Msg::FilingViewProblemNameTaken, String::new())
        }
        PlaceOutcome::Skipped(SkipReason::Interrupted) => {
            (Msg::FilingViewProblemStopped, String::new())
        }
        PlaceOutcome::Skipped(SkipReason::Failed(reason)) => {
            (Msg::FilingViewProblemFailed, reason.clone())
        }
    };
    Some(say(lang, msg, &[("file", &file), ("reason", &reason)]))
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
