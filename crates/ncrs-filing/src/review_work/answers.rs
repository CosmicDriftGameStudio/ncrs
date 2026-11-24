//! The answers to a review whose place is taken by a file: delete the
//! duplicate, replace the old file, or keep both. Nothing is deleted for good:
//! files go to the trash through a journaled run, and every move is the same
//! journaled move as filing by hand.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use ncrs_fs::Trash;
use ncrs_index::Kinds;
use ncrs_intake::{hash_path, render_pattern_in};

use super::{AcceptSummary, Naming, NotMoved, ReviewAccess, ReviewItem, PATTERN_DOES_NOT_FIT};
use crate::execute::{
    trash_can_restore, trash_replaced, trash_run, FilingBatches, ReplacedItem, TrashItem,
};
use crate::filing::lies_within;
use crate::filing_work::target_of;

pub const TRASH_CANNOT_RESTORE: &str = "the trash cannot give files back";
pub const IN_COPIED_FOLDER: &str = "the file lies in a folder whose files are copied, not changed";
pub const NOT_IN_A_LOCATION: &str = "the file lies in none of the folders Hermes works in";
pub const NOT_A_DUPLICATE: &str = "the file in the target holds other content";
pub const TARGET_GONE: &str = "the file in the target is gone";
pub const NOT_TRASHED: &str = "the file was not moved to the trash";
pub const NO_FREE_NAME: &str = "no free name was found";
const DUPLICATES_BATCH: &str = "Remove duplicates";
const REPLACED_BATCH: &str = "Replace files";
/// The highest number tried for the name of the second file.
const MOST_COPIES: u32 = 999;

/// What is known of one file for the user to compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileFacts {
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl FileFacts {
    fn of(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok().filter(fs::Metadata::is_file)?;
        Some(Self {
            size: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}

/// An incoming file next to the file that lies in its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetComparison {
    pub incoming: FileFacts,
    pub existing: FileFacts,
    /// Both files hold the same bytes.
    pub same_content: bool,
}

/// `None` when one of the files is not there. Equal sizes are hashed to tell
/// equal content from a mere equal length.
pub fn compare_files(incoming: &Path, existing: &Path) -> Option<TargetComparison> {
    let new = FileFacts::of(incoming)?;
    let old = FileFacts::of(existing)?;
    let same_content = new.size == old.size
        && matches!(
            (hash_path(incoming), hash_path(existing)),
            (Ok(first), Ok(second)) if first == second
        );
    Some(TargetComparison {
        incoming: new,
        existing: old,
        same_content,
    })
}

impl AcceptSummary {
    pub(super) fn fail(&mut self, detail: impl Into<NotMoved>) {
        self.failed += 1;
        self.first_failure.get_or_insert_with(|| detail.into());
    }
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// The name below the root for a second file next to the one at `relative`:
/// that name itself when it is free, else the first free " (2)", " (3)", ...
fn free_name(root: &Path, relative: &Path, source: &Path) -> Result<String, String> {
    let base = relative
        .to_str()
        .ok_or_else(|| PATTERN_DOES_NOT_FIT.to_owned())?;
    if !exists(&target_of(root, relative, source)) {
        return Ok(base.to_owned());
    }
    (2..=MOST_COPIES)
        .map(|number| format!("{base} ({number})"))
        .find(|name| !exists(&target_of(root, Path::new(name), source)))
        .ok_or_else(|| NO_FREE_NAME.to_owned())
}

impl ReviewAccess {
    /// Where files are put in the trash from, and that they can be got back.
    pub fn with_trash(self, trash: Arc<dyn Trash>) -> Self {
        Self {
            trash: Some(trash),
            ..self
        }
    }

    pub fn target_root(&self) -> &Path {
        &self.target_root
    }

    /// Each incoming file next to what lies in the place where it would go;
    /// `pairs` are the incoming path and the path below the archive the
    /// review suggests. Reads the files, so it blocks.
    pub fn compare_targets(&self, pairs: &[(PathBuf, PathBuf)]) -> Vec<Option<TargetComparison>> {
        pairs
            .iter()
            .map(|(incoming, relative)| {
                compare_files(incoming, &target_of(&self.target_root, relative, incoming))
            })
            .collect()
    }

    fn kinds(&self) -> Result<Kinds, String> {
        self.with_index(|index| {
            Ok::<_, String>(Kinds::new(Arc::clone(index.kinds()), self.kind_language))
        })
    }

    fn bin(&self) -> Result<&Arc<dyn Trash>, String> {
        self.trash
            .as_ref()
            .filter(|trash| trash_can_restore(&***trash))
            .ok_or_else(|| TRASH_CANNOT_RESTORE.to_owned())
    }

    /// The path below the archive the item's suggestion gives, and the file
    /// that would be there.
    fn suggested_place(
        &self,
        item: &ReviewItem,
        kinds: &Kinds,
    ) -> Result<(PathBuf, PathBuf), String> {
        let pattern = item
            .proposed_pattern
            .as_deref()
            .ok_or_else(|| PATTERN_DOES_NOT_FIT.to_owned())?;
        let relative = render_pattern_in(pattern, &item.fields, kinds)
            .ok_or_else(|| PATTERN_DOES_NOT_FIT.to_owned())?;
        let target = target_of(&self.target_root, &relative, &item.path);
        Ok((relative, target))
    }

    fn settle_removed(&self, item: &ReviewItem, summary: &mut AcceptSummary) {
        if exists(&item.path) {
            summary.fail(NOT_TRASHED.to_owned());
            return;
        }
        match self.with_index(|index| index.resolve_review(item.entry)) {
            Ok(()) => summary.trashed += 1,
            Err(detail) => summary.fail(detail),
        }
    }

    /// The incoming files go to the trash because the target already holds
    /// the same content. Each file is checked again, so a file that differs
    /// from the one in the target stays.
    pub fn delete_duplicates(&self, items: &[ReviewItem]) -> AcceptSummary {
        self.trash_duplicates(
            items,
            |item, kinds| {
                self.still_describes(item)?;
                let (_, target) = self.suggested_place(item, kinds)?;
                self.duplicate_to_trash(&item.path, target)
            },
            |item, summary| self.settle_removed(item, summary),
        )
    }

    /// Trashes each of `items` that `prepare` accepts, one journaled run per
    /// location, and lets `settle` close the question of every one that went.
    pub(super) fn trash_duplicates<T>(
        &self,
        items: &[T],
        prepare: impl Fn(&T, &Kinds) -> Result<(PathBuf, TrashItem), String>,
        settle: impl Fn(&T, &mut AcceptSummary),
    ) -> AcceptSummary {
        let _running = self.index_work.begin();
        let mut summary = AcceptSummary::default();
        let prepared = self.kinds().and_then(|kinds| Ok((kinds, self.bin()?)));
        let (kinds, bin) = match prepared {
            Ok(ready) => ready,
            Err(detail) => {
                items.iter().for_each(|_| summary.fail(detail.clone()));
                return summary;
            }
        };
        let mut runs: Vec<(PathBuf, Vec<TrashItem>, Vec<&T>)> = Vec::new();
        for item in items {
            match prepare(item, &kinds) {
                Ok((root, trash_item)) => match runs.iter_mut().find(|run| run.0 == root) {
                    Some(run) => {
                        run.1.push(trash_item);
                        run.2.push(item);
                    }
                    None => runs.push((root, vec![trash_item], vec![item])),
                },
                Err(detail) => summary.fail(detail),
            }
        }
        for (root, trash_items, members) in runs {
            let ran = self.with_index(|index| {
                trash_run(index, DUPLICATES_BATCH, &root, &trash_items, &**bin)
            });
            match ran {
                Ok(_) => members
                    .into_iter()
                    .for_each(|item| settle(item, &mut summary)),
                Err(detail) => members.iter().for_each(|_| summary.fail(detail.clone())),
            }
        }
        summary
    }

    /// The run and the trash entry for `path` if `keeper` holds the same
    /// content.
    pub(super) fn duplicate_to_trash(
        &self,
        path: &Path,
        keeper: PathBuf,
    ) -> Result<(PathBuf, TrashItem), String> {
        if self.copy_roots.iter().any(|copy| lies_within(path, copy)) {
            return Err(IN_COPIED_FOLDER.to_owned());
        }
        let root = self
            .location_roots
            .iter()
            .chain(std::iter::once(&self.target_root))
            .find(|root| lies_within(path, root))
            .ok_or_else(|| NOT_IN_A_LOCATION.to_owned())?
            .clone();
        if !exists(&keeper) {
            return Err(TARGET_GONE.to_owned());
        }
        let expected_hash = hash_path(path).map_err(|err| err.to_string())?;
        if hash_path(&keeper).map_err(|err| err.to_string())? != expected_hash {
            return Err(NOT_A_DUPLICATE.to_owned());
        }
        Ok((
            root,
            TrashItem {
                path: path.to_path_buf(),
                expected_hash,
                keeper,
            },
        ))
    }

    /// The old file in the place of each item goes to the trash, then the item
    /// is filed there.
    pub fn overwrite(&self, items: &[ReviewItem]) -> AcceptSummary {
        let _running = self.index_work.begin();
        let mut summary = AcceptSummary::default();
        let kinds = match self.kinds() {
            Ok(kinds) => kinds,
            Err(detail) => {
                items.iter().for_each(|_| summary.fail(detail.clone()));
                return summary;
            }
        };
        let mut ready: Vec<(&ReviewItem, PathBuf)> = Vec::new();
        let mut replaced: Vec<ReplacedItem> = Vec::new();
        for item in items {
            let placed = self
                .still_describes(item)
                .and_then(|()| self.suggested_place(item, &kinds))
                .and_then(|(_, target)| {
                    if exists(&target) {
                        let expected_hash = hash_path(&target).map_err(|err| err.to_string())?;
                        replaced.push(ReplacedItem {
                            path: target.clone(),
                            expected_hash,
                        });
                    }
                    Ok(target)
                });
            match placed {
                Ok(target) => ready.push((item, target)),
                Err(detail) => summary.fail(detail),
            }
        }
        if !replaced.is_empty() {
            let ran = self.bin().and_then(|bin| {
                self.with_index(|index| {
                    trash_replaced(index, REPLACED_BATCH, &self.target_root, &replaced, &**bin)
                })
            });
            if let Err(detail) = ran {
                ready.iter().for_each(|_| summary.fail(detail.clone()));
                return summary;
            }
        }
        let mut batches = FilingBatches::default();
        for (item, target) in ready {
            let outcome = match (&item.proposed_pattern, exists(&target)) {
                (_, true) => Err(NOT_TRASHED.to_owned().into()),
                (Some(pattern), false) => self.confirm_in(
                    &self.target_root,
                    &mut batches,
                    item.entry,
                    item.document_id,
                    &item.fields,
                    Naming::Pattern(pattern),
                ),
                (None, false) => Err(PATTERN_DOES_NOT_FIT.to_owned().into()),
            };
            summary.record(outcome);
        }
        summary
    }

    /// Files each item next to the file in its place, under the same name with
    /// the next free number.
    pub fn keep_both(&self, items: &[ReviewItem]) -> AcceptSummary {
        let _running = self.index_work.begin();
        let mut summary = AcceptSummary::default();
        let kinds = match self.kinds() {
            Ok(kinds) => kinds,
            Err(detail) => {
                items.iter().for_each(|_| summary.fail(detail.clone()));
                return summary;
            }
        };
        let mut batches = FilingBatches::default();
        for item in items {
            let outcome = self
                .still_describes(item)
                .and_then(|()| self.suggested_place(item, &kinds))
                .and_then(|(relative, _)| free_name(&self.target_root, &relative, &item.path))
                .map_err(NotMoved::from)
                .and_then(|name| {
                    self.confirm_in(
                        &self.target_root,
                        &mut batches,
                        item.entry,
                        item.document_id,
                        &item.fields,
                        Naming::Fixed(&name),
                    )
                });
            summary.record(outcome);
        }
        summary
    }
}
