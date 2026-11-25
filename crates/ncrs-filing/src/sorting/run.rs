//! Carries out a plan. Everything is checked again file by file, and the index
//! is free between two files.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use ncrs_fs::archive::is_device_name;
use ncrs_fs::transfer::Progress;
use ncrs_index::{BatchId, BatchKind, ContentHash, DocumentId, Index, TargetId, Transfer};
use ncrs_intake::{hash_path, is_plain_literal, stays_below, LocationTransfer, SortSource};

use super::access::{text, SortingAccess};
use super::plan_run::source_root;
use super::read_unknown::stamp_at;
use super::ready::{TargetState, WaitReason};
use crate::chat_work::remove_emptied_folders;
use crate::execute::{
    is_respelling, open_run_batch, run_one, ExecutionItem, ItemOutcome, RunPlaces, SkipReason,
};
use crate::reading_support::ReadError;
use crate::rules_work::{order_moves, ArchiveMove};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    /// What the history calls the run.
    pub name: String,
    pub source: SortSource,
    pub entries: Vec<RunEntry>,
    pub transfer: LocationTransfer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunEntry {
    pub path: PathBuf,
    /// What the file must still hold.
    pub hash: ContentHash,
    pub document: Option<DocumentId>,
    /// Below the folder of each target, with the file name. More than one
    /// place copies: the file itself then stays where it is.
    pub places: Vec<(TargetId, PathBuf)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    Source(ReadError),
    /// Nothing was done.
    Waiting(Vec<(TargetId, WaitReason)>),
    Index(String),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(err) => write!(f, "the source cannot be read: {err:?}"),
            Self::Waiting(targets) => {
                let names: Vec<String> = targets
                    .iter()
                    .map(|(id, reason)| format!("{id} ({reason:?})"))
                    .collect();
                write!(f, "not ready: {}", names.join(", "))
            }
            Self::Index(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for RunError {}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SortReport {
    /// Any batch of the run, for `SortingAccess::undo`; `None` when nothing
    /// was begun.
    pub run: Option<BatchId>,
    pub entries: Vec<EntryReport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryReport {
    pub path: PathBuf,
    /// One per place, in the order asked for.
    pub places: Vec<PlaceReport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceReport {
    pub target: TargetId,
    pub relative: PathBuf,
    pub outcome: PlaceOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceOutcome {
    Done,
    Skipped(SkipReason),
    Waiting(WaitReason),
    /// Not a target of the source, or a path that leaves it or is no name on
    /// every system.
    BadPlace,
}

impl EntryReport {
    pub fn is_done(&self) -> bool {
        !self.places.is_empty()
            && self
                .places
                .iter()
                .all(|place| place.outcome == PlaceOutcome::Done)
    }
}

impl SortReport {
    /// The files that reached every place they were to go to.
    pub fn done(&self) -> usize {
        self.entries.iter().filter(|entry| entry.is_done()).count()
    }
}

pub fn run<P: Progress>(
    access: &SortingAccess,
    request: &RunRequest,
    progress: &mut P,
) -> Result<SortReport, RunError> {
    run_as(access, request, BatchKind::Hermes, progress)
}

/// `run`, shown in the history as `kind`.
pub(super) fn run_as<P: Progress>(
    access: &SortingAccess,
    request: &RunRequest,
    kind: BatchKind,
    progress: &mut P,
) -> Result<SortReport, RunError> {
    let _work = access.index_work.begin();
    let root = source_root(&request.source).map_err(RunError::Source)?;
    let waiting = access.waiting(&request.source.targets);
    if !waiting.is_empty() {
        return Err(RunError::Waiting(waiting));
    }
    let mut job = Job {
        access,
        request,
        kind,
        root: &root,
        nested: access.targets_below(&root),
        batches: Vec::new(),
    };
    let result = job.all(progress);
    let closed = job.close();
    let report = result.map_err(RunError::Index)?;
    closed.map_err(RunError::Index)?;
    Ok(report)
}

struct Batch {
    id: BatchId,
    target: TargetId,
    transfer: Transfer,
}

struct Job<'a> {
    access: &'a SortingAccess,
    request: &'a RunRequest,
    kind: BatchKind,
    root: &'a Path,
    /// Targets below the source: no file comes out of one.
    nested: Vec<PathBuf>,
    /// Opened when the first file needs one; the first is the id of the run.
    batches: Vec<Batch>,
}

impl Job<'_> {
    fn all<P: Progress>(&mut self, progress: &mut P) -> Result<SortReport, String> {
        let order = self.order();
        let total = order.len();
        let mut reports: Vec<(usize, EntryReport)> = Vec::with_capacity(total);
        let mut moved_from = Vec::new();
        let mut stopped = false;
        for (done, at) in order.into_iter().enumerate() {
            let Some(entry) = self.request.entries.get(at) else {
                continue;
            };
            let report = if stopped {
                refused(entry, &PlaceOutcome::Skipped(SkipReason::Cancelled))
            } else {
                self.entry(entry)?
            };
            if report.is_done() && self.transfer_of(entry) == Transfer::Move {
                moved_from.push(entry.path.clone());
            }
            reports.push((at, report));
            stopped = stopped || progress.advanced(done + 1, total).is_err();
        }
        remove_emptied_folders(self.root, moved_from.iter().map(PathBuf::as_path));
        reports.sort_by_key(|(at, _)| *at);
        Ok(SortReport {
            run: self.batches.first().map(|batch| batch.id),
            entries: reports.into_iter().map(|(_, report)| report).collect(),
        })
    }

    /// Files that make room for another one go first. What no order can free
    /// runs last and is skipped for its taken place.
    fn order(&self) -> Vec<usize> {
        let mut at_path: HashMap<PathBuf, usize> = HashMap::new();
        let mut moves = Vec::new();
        for (at, entry) in self.request.entries.iter().enumerate() {
            if let Some(found) = self.archive_move(entry) {
                at_path.insert(found.root.join(&found.from), at);
                moves.push(found);
            }
        }
        let (ordered, _) = order_moves(moves);
        let mut order: Vec<usize> = Vec::with_capacity(self.request.entries.len());
        let mut placed = vec![false; self.request.entries.len()];
        let ordered_first = ordered
            .iter()
            .filter_map(|found| at_path.get(&found.root.join(&found.from)).copied());
        for at in ordered_first.chain(0..self.request.entries.len()) {
            if let Some(seen) = placed.get_mut(at) {
                if !std::mem::replace(seen, true) {
                    order.push(at);
                }
            }
        }
        order
    }

    fn archive_move(&self, entry: &RunEntry) -> Option<ArchiveMove> {
        let [(id, relative)] = entry.places.as_slice() else {
            return None;
        };
        let folder = self.access.target(id)?.path;
        if self.transfer_of(entry) != Transfer::Move || !self.root.starts_with(&folder) {
            return None;
        }
        Some(ArchiveMove {
            from: entry.path.strip_prefix(&folder).ok()?.to_path_buf(),
            root: folder,
            to: relative.clone(),
            hash: entry.hash,
        })
    }

    fn transfer_of(&self, entry: &RunEntry) -> Transfer {
        if entry.places.len() > 1 || self.request.transfer == LocationTransfer::Copy {
            Transfer::Copy
        } else {
            Transfer::Move
        }
    }

    fn entry(&mut self, entry: &RunEntry) -> Result<EntryReport, String> {
        let below = entry.path.strip_prefix(self.root).is_ok_and(stays_below);
        let nested = self
            .nested
            .iter()
            .any(|folder| entry.path.starts_with(folder));
        if !below || nested {
            let outside = PlaceOutcome::Skipped(SkipReason::OutsideLocation);
            return Ok(refused(entry, &outside));
        }
        // Hashed before anything is opened, so a changed file begins no run;
        // the move itself compares the content again.
        let unusable = match hash_path(&entry.path) {
            Ok(hash) if hash == entry.hash => None,
            Ok(_) => Some(SkipReason::SourceChanged),
            Err(_) if !entry.path.exists() => Some(SkipReason::SourceMissing),
            Err(err) => Some(SkipReason::Failed(err.to_string())),
        };
        if let Some(reason) = unusable {
            return Ok(refused(entry, &PlaceOutcome::Skipped(reason)));
        }
        let known = self.known(entry)?;
        let transfer = self.transfer_of(entry);
        let mut places = Vec::with_capacity(entry.places.len());
        for (target, relative) in &entry.places {
            places.push(PlaceReport {
                target: target.clone(),
                relative: relative.clone(),
                outcome: self.place(entry, target, relative, transfer, known.as_ref())?,
            });
        }
        let report = EntryReport {
            path: entry.path.clone(),
            places,
        };
        if let Some(own) = known.filter(|known| !known.duplicate && report.is_done()) {
            self.access
                .with_index(|index| close_question(index, own.document))?;
        }
        Ok(report)
    }

    /// The document of the file. One whose own file is gone lies here now.
    fn known(&self, entry: &RunEntry) -> Result<Option<Known>, String> {
        self.access.with_index(|index| {
            let given = match entry.document {
                Some(id) => index.document(id).map_err(text)?,
                None => None,
            };
            let document = match given {
                Some(found) if found.hash == entry.hash => Some(found),
                _ => index.document_by_hash(&entry.hash).map_err(text)?,
            };
            let Some(document) = document else {
                return Ok(None);
            };
            let mut duplicate = document.path != entry.path;
            if duplicate && !document.path.exists() {
                if let Some(stamp) = stamp_at(&entry.path) {
                    index
                        .set_document_path(document.id, &entry.path, stamp)
                        .map_err(text)?;
                    duplicate = false;
                }
            }
            Ok(Some(Known {
                document: document.id,
                duplicate,
            }))
        })
    }

    fn place(
        &mut self,
        entry: &RunEntry,
        id: &TargetId,
        relative: &Path,
        transfer: Transfer,
        known: Option<&Known>,
    ) -> Result<PlaceOutcome, String> {
        let is_allowed = self.request.source.targets.contains(id) && is_usable(relative);
        let (Some(target), true) = (self.access.target(id), is_allowed) else {
            return Ok(PlaceOutcome::BadPlace);
        };
        if let TargetState::Waiting(reason) = self.access.target_state(id) {
            return Ok(PlaceOutcome::Waiting(reason));
        }
        let to = target.path.join(relative);
        if std::fs::symlink_metadata(&to).is_ok() && !is_respelling(&entry.path, &to) {
            return Ok(PlaceOutcome::Skipped(SkipReason::TargetExists));
        }
        let places = RunPlaces {
            source: self.root,
            target: &target.path,
            inside_target: self.root.starts_with(&target.path),
        };
        let item = ExecutionItem {
            from: entry.path.clone(),
            to,
            expected_hash: entry.hash,
            document_id: known.map(|found| found.document),
            duplicate: known.is_some_and(|found| found.duplicate),
        };
        let open = self
            .batches
            .iter()
            .find(|batch| batch.target == *id && batch.transfer == transfer)
            .map(|batch| batch.id);
        let run = self.batches.first().map(|batch| batch.id);
        let (batch, outcome) = self.access.with_index(|index| {
            let batch = match open {
                Some(batch) => batch,
                None => {
                    let name = &self.request.name;
                    let batch =
                        open_run_batch(index, self.kind, name, transfer, &places).map_err(text)?;
                    index.join_run(batch, run.unwrap_or(batch)).map_err(text)?;
                    batch
                }
            };
            Ok((batch, run_one(index, batch, transfer, &places, &item)))
        })?;
        if open.is_none() {
            self.batches.push(Batch {
                id: batch,
                target: id.clone(),
                transfer,
            });
        }
        // An error leaves the journal entry `pending` for `reconcile`.
        match outcome.map_err(text)? {
            ItemOutcome::Moved => Ok(PlaceOutcome::Done),
            ItemOutcome::Skipped(reason) => Ok(PlaceOutcome::Skipped(reason)),
        }
    }

    fn close(&self) -> Result<(), String> {
        for batch in &self.batches {
            self.access
                .with_index(|index| index.finish_batch(batch.id).map_err(text))?;
        }
        Ok(())
    }
}

struct Known {
    document: DocumentId,
    /// The file is a copy; the document lies elsewhere.
    duplicate: bool,
}

fn refused(entry: &RunEntry, outcome: &PlaceOutcome) -> EntryReport {
    EntryReport {
        path: entry.path.clone(),
        places: entry
            .places
            .iter()
            .map(|(target, relative)| PlaceReport {
                target: target.clone(),
                relative: relative.clone(),
                outcome: outcome.clone(),
            })
            .collect(),
    }
}

pub(super) fn close_question(index: &Index, document: DocumentId) -> Result<(), String> {
    if let Some(question) = index.open_review_of(document).map_err(text)? {
        index.resolve_review(question.id).map_err(text)?;
    }
    Ok(())
}

/// The most bytes a file system takes in one name.
const NAME_LIMIT: usize = 255;

/// A place below the target with names every system can hold, as a place may
/// be typed by hand.
fn is_usable(relative: &Path) -> bool {
    stays_below(relative)
        && relative.components().all(|part| {
            part.as_os_str().to_str().is_some_and(|name| {
                is_plain_literal(name)
                    && name.len() <= NAME_LIMIT
                    && !name.ends_with([' ', '.'])
                    && !is_device_name(name)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_typed_by_hand_stays_below_the_target_and_is_a_name_everywhere() {
        assert!(is_usable(Path::new("Acme/2025/Invoice 1.pdf")));
        let long = format!("{}.pdf", "a".repeat(NAME_LIMIT));
        for bad in [
            "",
            "../x.pdf",
            "/x.pdf",
            "a/../x.pdf",
            "CON.pdf",
            "a./x.pdf",
            "a:b.pdf",
            "x.pdf ",
            &long,
        ] {
            assert!(!is_usable(Path::new(bad)), "{bad}");
        }
    }
}
