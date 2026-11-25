//! Filing from a watched source without being asked: only what one rule
//! decides, and only files that came after the source was watched.

use std::path::{Path, PathBuf};

use ncrs_fs::transfer::Progress;
use ncrs_index::{BatchKind, DocumentStatus, FileStamp, Index, ReviewReason, TargetId};
use ncrs_intake::{OnNoText, OnSeveralTargets, SortSource};

use super::access::{text, SortingAccess};
use super::plan_run::{
    files_of, plan_watched, source_root, PlanError, PlanFiles, PlanRequest, PlanRow, RowPlace, RowState,
};
use super::read_unknown::ReadUnknown;
use super::ready::{source_is_mounted, WaitReason};
use super::run::{run_as, RunEntry, RunError, RunRequest, SortReport};
use crate::execute::reconcile;
use crate::reading_support::{walk_sorted, ReadError, ReadProgress};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchedFiles {
    Changed(Vec<PathBuf>),
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchedReport {
    Skipped(WatchedSkip),
    /// The first look at the source: what lies there stays there.
    BacklogRecorded {
        files: usize,
    },
    SourceWaiting(SourceWait),
    Waiting(Vec<(TargetId, WaitReason)>),
    Done {
        run: SortReport,
        asked: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceWait {
    Folder(ReadError),
    /// The volume the source is on is not there.
    NotMounted,
}

impl SourceWait {
    fn from_error(source: &SortSource, err: ReadError) -> Self {
        match err {
            ReadError::Gone if source.own_volume == Some(true) => Self::NotMounted,
            err => Self::Folder(err),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchedSkip {
    NotWatched,
    Paused,
    /// The user is looking at a plan of this source.
    Busy,
    InsideTarget,
}

/// `name` is what the history calls the run.
pub fn run_watched<P: Progress>(
    access: &SortingAccess,
    source: &SortSource,
    files: &WatchedFiles,
    busy_source: Option<&Path>,
    name: &str,
    progress: &mut P,
) -> Result<WatchedReport, String> {
    let is_mounted = |root: &Path| source_is_mounted(source, root);
    watch_with(access, source, files, busy_source, name, progress, is_mounted)
}

pub(super) fn watch_with<P: Progress>(
    access: &SortingAccess,
    source: &SortSource,
    files: &WatchedFiles,
    busy_source: Option<&Path>,
    name: &str,
    progress: &mut P,
    is_mounted: impl Fn(&Path) -> bool,
) -> Result<WatchedReport, String> {
    let waiting = |err| WatchedReport::SourceWaiting(SourceWait::from_error(source, err));
    if !source.watch {
        return Ok(WatchedReport::Skipped(WatchedSkip::NotWatched));
    }
    if is_paused(access, source) {
        return Ok(WatchedReport::Skipped(WatchedSkip::Paused));
    }
    let root = match source_root(source) {
        Ok(root) => root,
        Err(err) => return Ok(waiting(err)),
    };
    let is_busy = busy_source.is_some_and(|busy| {
        busy == source.path || ncrs_intake::canonical_path(busy).is_ok_and(|busy| busy == root)
    });
    if is_busy {
        return Ok(WatchedReport::Skipped(WatchedSkip::Busy));
    }
    if access.is_in_target(&root) {
        return Ok(WatchedReport::Skipped(WatchedSkip::InsideTarget));
    }
    if !is_mounted(&root) {
        return Ok(WatchedReport::SourceWaiting(SourceWait::NotMounted));
    }
    let _work = access.index_work.begin();
    access.with_index(|index| reconcile(index).map_err(text))?;

    if !access.with_index(|index| index.has_source_backlog(&source.path).map_err(text))? {
        return match backlog_of(&root) {
            Err(err) => Ok(waiting(err)),
            // Unplugged while it was walked, the list is cut short.
            Ok(_) if !is_mounted(&root) => {
                Ok(WatchedReport::SourceWaiting(SourceWait::NotMounted))
            }
            Ok(backlog) => record_backlog(access, source, &backlog),
        };
    }
    // Before the walk: while a drive is away this is asked again and again.
    let targets_waiting = access.waiting(&source.targets);
    if !targets_waiting.is_empty() {
        return Ok(WatchedReport::Waiting(targets_waiting));
    }
    let new = match new_files(access, source, &root, files)? {
        Ok(new) => new,
        Err(err) => return Ok(waiting(err)),
    };
    let request = PlanRequest {
        source: source.clone(),
        files: PlanFiles::Listed(new),
        read: ReadUnknown::No,
    };
    let planned = match plan_watched(access, &request) {
        Ok(planned) => planned,
        Err(PlanError::Source(err)) => return Ok(waiting(err)),
        Err(err) => return Err(err.to_string()),
    };
    if !planned.waiting.is_empty() {
        return Ok(WatchedReport::Waiting(planned.waiting));
    }
    let mut entries = Vec::new();
    let mut asked = 0;
    for row in &planned.rows {
        match access.with_index(|index| step(index, source, row))? {
            Step::File(entry) => entries.push(entry),
            Step::Asked => asked += 1,
            Step::Leave => {}
        }
    }
    let request = RunRequest {
        name: name.to_owned(),
        source: source.clone(),
        entries,
        transfer: source.transfer,
    };
    match run_as(access, &request, BatchKind::Filing, progress) {
        Ok(run) => Ok(WatchedReport::Done { run, asked }),
        Err(RunError::Waiting(targets)) => Ok(WatchedReport::Waiting(targets)),
        Err(RunError::Source(err)) => Ok(waiting(err)),
        Err(err) => Err(err.to_string()),
    }
}

fn is_paused(access: &SortingAccess, source: &SortSource) -> bool {
    access
        .sources
        .iter()
        .any(|entry| entry.source.path == source.path && entry.paused.is_some())
}

/// All files, whatever the filter says: a filter changed later must not make
/// old files new. The same goes for targets below the source.
fn backlog_of(root: &Path) -> Result<Vec<(PathBuf, FileStamp)>, ReadError> {
    let walked = walk_sorted(root, &[], &ReadProgress::default())?;
    Ok(walked
        .into_iter()
        .map(|file| (root.join(&file.relative), file.stamp))
        .collect())
}

fn record_backlog(
    access: &SortingAccess,
    source: &SortSource,
    backlog: &[(PathBuf, FileStamp)],
) -> Result<WatchedReport, String> {
    access.with_index_mut(|index| {
        index
            .set_source_backlog(&source.path, backlog)
            .map_err(text)
    })?;
    Ok(WatchedReport::BacklogRecorded {
        files: backlog.len(),
    })
}

fn new_files(
    access: &SortingAccess,
    source: &SortSource,
    root: &Path,
    files: &WatchedFiles,
) -> Result<Result<Vec<PathBuf>, ReadError>, String> {
    let wanted = match files {
        WatchedFiles::All => PlanFiles::All,
        WatchedFiles::Changed(paths) => PlanFiles::Listed(paths.clone()),
    };
    let found = match files_of(access, root, &wanted, &ReadProgress::default()) {
        Ok(found) => found,
        Err(err) => return Ok(Err(err)),
    };
    let mut new = Vec::new();
    for file in found {
        let path = root.join(&file.relative);
        let is_backlog = access.with_index(|index| {
            index
                .is_backlog(&source.path, &path, file.stamp)
                .map_err(text)
        })?;
        if !is_backlog {
            new.push(path);
        }
    }
    Ok(Ok(new))
}

enum Step {
    File(RunEntry),
    Asked,
    Leave,
}

fn step(index: &Index, source: &SortSource, row: &PlanRow) -> Result<Step, String> {
    let (Some(id), Some(hash)) = (row.document, row.hash) else {
        return Ok(Step::Leave);
    };
    let Some(document) = index.document(id).map_err(text)? else {
        return Ok(Step::Leave);
    };
    // A copy of a document that lies elsewhere is the user's, and a filed
    // document is done with.
    if document.path != row.path || document.status == DocumentStatus::Filed {
        return Ok(Step::Leave);
    }
    let question = index.open_review_of(id).map_err(text)?;
    let file = |places: &[RowPlace]| {
        Step::File(RunEntry {
            path: row.path.clone(),
            hash,
            document: Some(id),
            places: places
                .iter()
                .map(|place| (place.target.clone(), place.relative.clone()))
                .collect(),
        })
    };
    let ask = |reason: ReviewReason| -> Result<Step, String> {
        index.add_review_shared(id, reason).map_err(text)?;
        Ok(Step::Asked)
    };
    match &row.state {
        RowState::Place(place) => Ok(file(std::slice::from_ref(place))),
        RowState::Several(_) if question.is_some() => Ok(Step::Leave),
        RowState::Several(places) => match source.several {
            OnSeveralTargets::CopyToAll => Ok(file(places)),
            OnSeveralTargets::Ask => ask(ReviewReason::SeveralTargets),
        },
        RowState::NoText if question.is_none() && source.no_text == OnNoText::Ask => {
            ask(ReviewReason::NoText)
        }
        _ => Ok(Step::Leave),
    }
}
