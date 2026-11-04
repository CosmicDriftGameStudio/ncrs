//! The runs of the chat: filing, trashing duplicates, and undoing the last one.

use std::fs;
use std::path::{Path, PathBuf};

use ncrs_fs::transfer::Counting;
use ncrs_index::{BatchId, BatchKind, ContentHash, Index, JournalState, Transfer};
use ncrs_intake::stays_below;

use super::{ChatAccess, TrashItem, TrashReport, UndoReport};
use crate::execute::{self, ExecutionItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunItem {
    /// Absolute, below the source root.
    pub from: PathBuf,
    /// Below the target root.
    pub to: PathBuf,
    pub hash: ContentHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    pub name: String,
    /// `Move` or `Copy`.
    pub transfer: Transfer,
    pub source_root: PathBuf,
    pub target_root: PathBuf,
    pub items: Vec<RunItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunReport {
    pub done: usize,
    pub skipped: usize,
}

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

fn refusal(run: &RunPlan) -> Option<String> {
    if run.transfer == Transfer::Trash {
        return Some("a filing run moves or copies".to_owned());
    }
    let source = real(&run.source_root);
    let target = real(&run.target_root);
    if source.starts_with(&target) || target.starts_with(&source) {
        return Some(format!(
            "{} and {} lie inside each other",
            run.source_root.display(),
            run.target_root.display()
        ));
    }
    for item in &run.items {
        let below_source = item
            .from
            .strip_prefix(&run.source_root)
            .is_ok_and(stays_below);
        if !below_source {
            return Some(format!(
                "{} is not below {}",
                item.from.display(),
                run.source_root.display()
            ));
        }
        if !stays_below(&item.to) {
            return Some(format!("{} leaves the target", item.to.display()));
        }
    }
    None
}

pub fn file(access: &ChatAccess, run: &RunPlan) -> Result<RunReport, String> {
    if let Some(reason) = refusal(run) {
        return Err(reason);
    }
    access.with_index(|index| {
        let items: Vec<ExecutionItem> = run
            .items
            .iter()
            .map(|item| {
                let document_id = index
                    .document_by_hash(&item.hash)
                    .map_err(|err| err.to_string())?
                    .filter(|document| document.path == item.from)
                    .map(|document| document.id);
                Ok(ExecutionItem {
                    from: item.from.clone(),
                    to: run.target_root.join(&item.to),
                    expected_hash: item.hash,
                    document_id,
                    duplicate: false,
                })
            })
            .collect::<Result<_, String>>()?;
        let report = execute::execute_run(
            index,
            BatchKind::Hermes,
            &run.name,
            run.transfer,
            execute::RunPlaces {
                source: &run.source_root,
                target: &run.target_root,
            },
            &items,
            &mut Counting,
        )
        .map_err(|err| err.to_string())?;
        Ok(RunReport {
            done: report.moved(),
            skipped: report.skipped(),
        })
    })
}

pub fn trash_duplicates(
    access: &ChatAccess,
    name: &str,
    root: &Path,
    items: &[TrashItem],
) -> Result<TrashReport, String> {
    if !access.trash_can_restore() {
        return Err("the trash cannot give files back".to_owned());
    }
    access.with_index(|index| {
        execute::trash_run(index, name, root, items, &*access.trash).map_err(|err| err.to_string())
    })
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

pub fn last_run(access: &ChatAccess) -> Result<Option<RunSummary>, String> {
    access.with_index(|index| {
        let Some(batch) = index
            .last_undoable_batch(BatchKind::Hermes)
            .map_err(|err| err.to_string())?
        else {
            return Ok(None);
        };
        Ok(recent_run(index, batch)?.map(|run| run.summary))
    })
}

/// The runs that can still be put back, newest first.
pub fn recent_runs(access: &ChatAccess, limit: usize) -> Result<Vec<RecentRun>, String> {
    access.with_index(|index| {
        let mut runs = Vec::new();
        for batch in index
            .undoable_batches(BatchKind::Hermes, limit)
            .map_err(|err| err.to_string())?
        {
            runs.extend(recent_run(index, batch)?);
        }
        Ok(runs)
    })
}

fn recent_run(index: &Index, batch: BatchId) -> Result<Option<RecentRun>, String> {
    let Some(info) = index.batch_info(batch).map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    let done: Vec<_> = index
        .journal_items(batch)
        .map_err(|err| err.to_string())?
        .into_iter()
        .filter(|item| item.state == JournalState::Moved)
        .collect();
    let target_folder = info.target_root.as_ref().map(|root| root.given.clone());
    let examples = target_folder
        .as_deref()
        .map(|target| example_folders(target, &done))
        .unwrap_or_default();
    let undoable = match (&info.source_root, &target_folder) {
        (Some(source), Some(target)) => {
            let plan = RunPlan {
                name: String::new(),
                transfer: info.transfer,
                source_root: source.given.clone(),
                target_root: target.clone(),
                items: done
                    .iter()
                    .map(|item| RunItem {
                        from: item.from.clone(),
                        to: item
                            .to
                            .strip_prefix(target)
                            .unwrap_or(&item.to)
                            .to_path_buf(),
                        hash: item.hash,
                    })
                    .collect(),
            };
            refusal(&plan).is_none()
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
        },
        target_folder,
        examples,
        undoable,
    }))
}

fn example_folders(target: &Path, done: &[ncrs_index::JournalItem]) -> Vec<PathBuf> {
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
    access.with_index(|index| {
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
        execute::undo_with_trash(index, batch, &roots, Some(&*access.trash))
            .map_err(|err| err.to_string())
    })
}
