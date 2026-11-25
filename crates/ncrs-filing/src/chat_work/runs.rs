//! The runs that can be put back, and putting them back.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use ncrs_index::{BatchId, BatchKind, Index, JournalItem, JournalState, Transfer};
use ncrs_intake::stays_below;

use super::ChatAccess;
use crate::execute::{self, UndoReport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub batch: BatchId,
    pub name: String,
    pub transfer: Transfer,
    /// The files that were carried out.
    pub items: usize,
    /// Unix seconds.
    pub finished_at: i64,
    pub source_folder: Option<PathBuf>,
    /// Hermes filed these by itself; `name` is then the folder they went into.
    pub automatic: bool,
}

/// A run of the list in the settings: the summary, where its files went, and
/// whether the run may be put back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentRun {
    pub summary: RunSummary,
    pub target_folder: Option<PathBuf>,
    /// Folders below the target that received files, as paths below it.
    pub examples: Vec<PathBuf>,
    /// The run passes the same checks a filing run does.
    pub undoable: bool,
}

/// How many folders a run names as examples.
const EXAMPLE_FOLDERS: usize = 2;

/// The path with links resolved as far as it exists, so two spellings of the
/// same folder compare equal even when the folder is not made yet.
fn real(path: &Path) -> PathBuf {
    path.ancestors()
        .find_map(|ancestor| {
            let resolved = fs::canonicalize(ancestor).ok()?;
            let rest = path.strip_prefix(ancestor).ok()?;
            Some(resolved.join(rest))
        })
        .unwrap_or_else(|| path.to_path_buf())
}

/// A run of its own keeps its source and target apart; only a run that moved
/// files within its target has one folder for both.
fn places_lie_apart(source: &Path, target: &Path, done: &[JournalItem]) -> bool {
    let real_source = real(source);
    let real_target = real(target);
    let only_inside = done.iter().all(|item| item.from.starts_with(target));
    (only_inside && real_source == real_target)
        || !(real_source.starts_with(&real_target) || real_target.starts_with(&real_source))
}

fn items_stay_below(source: &Path, target: &Path, done: &[JournalItem]) -> bool {
    done.iter().all(|item| {
        let root = if item.from.starts_with(target) {
            target
        } else {
            source
        };
        item.from.strip_prefix(root).is_ok_and(stays_below)
            && stays_below(item.to.strip_prefix(target).unwrap_or(&item.to))
    })
}

/// The folders below `root` a run emptied by moving files out of them; `root`
/// and every folder that still holds anything stay.
pub fn remove_emptied_folders<'a>(root: &Path, moved_from: impl Iterator<Item = &'a Path>) {
    let mut folders: Vec<&Path> = moved_from
        .filter_map(Path::parent)
        .flat_map(|parent| parent.ancestors().take_while(|folder| *folder != root))
        .filter(|folder| folder.starts_with(root))
        .collect();
    folders.sort_by_key(|folder| std::cmp::Reverse(folder.components().count()));
    for folder in folders {
        // Fails on a folder that holds anything, hidden files included.
        let _ = fs::remove_dir(folder);
    }
}

fn common_parent<'a>(paths: impl Iterator<Item = &'a Path>) -> Option<PathBuf> {
    let mut common: Option<PathBuf> = None;
    for parent in paths.filter_map(Path::parent) {
        common = Some(match common {
            None => parent.to_path_buf(),
            Some(current) => current
                .ancestors()
                .find(|ancestor| parent.starts_with(ancestor))
                .map(Path::to_path_buf)
                .unwrap_or_default(),
        });
    }
    common
}

/// The run a batch belongs to, by its first batch; itself when it is a run of
/// its own.
fn run_of(index: &Index, batch: BatchId) -> Result<BatchId, String> {
    Ok(index
        .batch_info(batch)
        .map_err(|err| err.to_string())?
        .and_then(|info| info.run)
        .unwrap_or(batch))
}

/// The newest runs that can be put back, newest first: the runs Hermes
/// carried out on request and the files it filed by itself.
fn undoable(index: &Index, limit: usize) -> Result<Vec<BatchId>, String> {
    let mut batches = Vec::new();
    for kind in [BatchKind::Hermes, BatchKind::Filing] {
        batches.extend(
            index
                .undoable_batches(kind, usize::MAX)
                .map_err(|err| err.to_string())?,
        );
    }
    batches.sort_by_key(|batch| std::cmp::Reverse(batch.number()));
    let mut runs = Vec::new();
    for batch in batches {
        if runs.len() == limit {
            break;
        }
        let run = run_of(index, batch)?;
        if !runs.contains(&run) {
            runs.push(run);
        }
    }
    Ok(runs)
}

pub fn last_run(access: &ChatAccess) -> Result<Option<RunSummary>, String> {
    access.with_index(|index| {
        let Some(batch) = undoable(index, 1)?.into_iter().next() else {
            return Ok(None);
        };
        Ok(recent_run(index, batch)?.map(|run| run.summary))
    })
}

/// The runs that can still be put back, newest first.
pub fn runs_of_index(index: &Index, limit: usize) -> Result<Vec<RecentRun>, String> {
    let mut runs = Vec::new();
    for batch in undoable(index, limit)? {
        runs.extend(recent_run(index, batch)?);
    }
    Ok(runs)
}

fn done_items(index: &Index, batch: BatchId) -> Result<Vec<JournalItem>, String> {
    Ok(index
        .journal_items(batch)
        .map_err(|err| err.to_string())?
        .into_iter()
        .filter(|item| matches!(item.state, JournalState::Moved | JournalState::UndoSkipped))
        .collect())
}

/// All batches of a run as one. The files are counted by where they came from:
/// one copied into two targets is one file.
fn recent_run(index: &Index, run: BatchId) -> Result<Option<RecentRun>, String> {
    let mut whole: Option<RecentRun> = None;
    let mut sources: BTreeSet<PathBuf> = BTreeSet::new();
    for batch in index.batches_of_run(run).map_err(|err| err.to_string())? {
        let done = done_items(index, batch)?;
        if done.is_empty() {
            continue;
        }
        let Some(part) = batch_as_run(index, batch)? else {
            continue;
        };
        sources.extend(done.into_iter().map(|item| item.from));
        match &mut whole {
            None => whole = Some(part),
            Some(sum) => {
                sum.summary.finished_at = sum.summary.finished_at.max(part.summary.finished_at);
                sum.undoable &= part.undoable;
                if sum.target_folder != part.target_folder {
                    sum.target_folder = None;
                }
                for folder in part.examples {
                    if sum.examples.len() < EXAMPLE_FOLDERS && !sum.examples.contains(&folder) {
                        sum.examples.push(folder);
                    }
                }
            }
        }
    }
    let Some(mut sum) = whole else {
        return batch_as_run(index, run);
    };
    sum.summary.batch = run;
    sum.summary.items = sources.len();
    sum.summary.source_folder = common_parent(sources.iter().map(PathBuf::as_path));
    Ok(Some(sum))
}

fn batch_as_run(index: &Index, batch: BatchId) -> Result<Option<RecentRun>, String> {
    let Some(info) = index.batch_info(batch).map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    let done = done_items(index, batch)?;
    let target_folder = info.target_root.as_ref().map(|root| root.given.clone());
    let examples = target_folder
        .as_deref()
        .map(|target| example_folders(target, &done))
        .unwrap_or_default();
    let undoable = match (&info.source_root, &target_folder) {
        (Some(source), Some(target)) => {
            // A sorting run may file into a target below its source.
            items_stay_below(&source.given, target, &done)
                && (info.run.is_some()
                    || (info.transfer != Transfer::Trash
                        && places_lie_apart(&source.given, target, &done)))
        }
        _ => info.transfer != Transfer::Trash,
    };
    Ok(Some(RecentRun {
        summary: RunSummary {
            batch,
            name: info.name.unwrap_or_default(),
            transfer: info.transfer,
            items: done.len(),
            finished_at: info.finished_at.unwrap_or(info.started_at),
            source_folder: common_parent(done.iter().map(|item| item.from.as_path())),
            automatic: info.kind == BatchKind::Filing,
        },
        target_folder,
        examples,
        undoable,
    }))
}

fn example_folders(target: &Path, done: &[JournalItem]) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = Vec::new();
    for folder in done
        .iter()
        .filter_map(|item| item.to.strip_prefix(target).ok()?.parent())
        .filter(|folder| !folder.as_os_str().is_empty())
    {
        if folders.len() == EXAMPLE_FOLDERS {
            break;
        }
        if !folders.iter().any(|known| known == folder) {
            folders.push(folder.to_path_buf());
        }
    }
    folders
}

pub fn undo(access: &ChatAccess, batch: BatchId) -> Result<UndoReport, String> {
    access.with_index(|index| undo_run(index, &*access.trash, batch))
}

/// Takes back all batches of the run, the last one first. A batch that cannot
/// be taken back at all leaves its files where they are; the others still go.
pub fn undo_run(
    index: &Index,
    trash: &dyn ncrs_fs::Trash,
    batch: BatchId,
) -> Result<UndoReport, String> {
    let run = run_of(index, batch)?;
    let mut batches = index.batches_of_run(run).map_err(|err| err.to_string())?;
    if batches.is_empty() {
        return undo_batch(index, trash, batch);
    }
    batches.reverse();
    let mut total = UndoReport::default();
    let mut refused = None;
    let mut taken_back = false;
    for part in batches {
        match undo_batch(index, trash, part) {
            Ok(report) => {
                total.undone += report.undone;
                total.left += report.left;
                taken_back = true;
            }
            Err(err) => {
                total.left += done_items(index, part)?.len();
                refused = Some(err);
            }
        }
    }
    match refused {
        Some(err) if !taken_back => Err(err),
        _ => Ok(total),
    }
}

fn undo_batch(
    index: &Index,
    trash: &dyn ncrs_fs::Trash,
    batch: BatchId,
) -> Result<UndoReport, String> {
    let items = index.journal_items(batch).map_err(|err| err.to_string())?;
    // The recorded source and target folders are added by the undo
    // itself; older runs fall back on the parent the targets share.
    let has_target = index
        .batch_info(batch)
        .map_err(|err| err.to_string())?
        .is_some_and(|info| info.target_root.is_some());
    let roots: Vec<PathBuf> = if has_target {
        Vec::new()
    } else {
        common_parent(items.iter().map(|item| item.to.as_path()))
            .into_iter()
            .collect()
    };
    execute::undo_with_trash(index, batch, &roots, Some(trash)).map_err(|err| err.to_string())
}
