use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::Utc;
use ncrs_index::{DocumentId, Index};
use walkdir::WalkDir;

use crate::classify::{classify, is_hidden_name, Skip};
use crate::error::IntakeError;
use crate::location::Location;
use crate::stability::StabilityTracker;
use crate::take_in::{path_exists, promote_first_copy, take_in_counted, Outcome};

/// Only this many failing paths are listed; `error_total` counts all of them.
pub const MAX_REPORTED_ERRORS: usize = 100;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScanReport {
    pub new: usize,
    pub moved: usize,
    pub copies: usize,
    pub already_filed: usize,
    pub unchanged: usize,
    /// Of `unchanged`, the files recognised by path, size and mtime alone.
    pub unchanged_without_hash: usize,
    pub content_changed: usize,
    pub skipped: BTreeMap<Skip, usize>,
    pub not_stable: Vec<PathBuf>,
    /// Recorded copies whose file is gone, removed from the index.
    pub removed_copies: usize,
    /// Documents whose file is gone and whose recorded copy took their place.
    pub promoted_copies: usize,
    /// Documents whose file is gone and that have no copy left.
    pub missing: usize,
    pub error_paths: Vec<PathBuf>,
    pub error_total: usize,
}

impl ScanReport {
    fn count(&mut self, outcome: Outcome, path: &Path, hashed: bool) {
        match outcome {
            Outcome::New(_) => self.new += 1,
            Outcome::Moved(_) => self.moved += 1,
            Outcome::Copy(_) => self.copies += 1,
            Outcome::AlreadyFiled(_) => self.already_filed += 1,
            Outcome::Unchanged(_) => {
                self.unchanged += 1;
                if !hashed {
                    self.unchanged_without_hash += 1;
                }
            }
            Outcome::ContentChanged(_) => self.content_changed += 1,
            Outcome::Skipped(skip) => self.count_skip(skip),
            Outcome::NotStableYet => self.not_stable.push(path.to_path_buf()),
        }
    }

    fn count_skip(&mut self, skip: Skip) {
        *self.skipped.entry(skip).or_default() += 1;
    }

    fn count_error(&mut self, path: &Path) {
        self.error_total += 1;
        if self.error_paths.len() < MAX_REPORTED_ERRORS {
            self.error_paths.push(path.to_path_buf());
        }
    }
}

/// What the walk of a location came across.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found<'path> {
    File(&'path Path),
    /// A hidden file or folder below the root; a hidden folder is not entered.
    Hidden,
    /// An entry that could not be read, with the reason the walk gave.
    Unreadable {
        path: &'path Path,
        message: &'path str,
    },
}

/// Walks the location once, without following links, in file name order. Only
/// files are reported, never folders. `ControlFlow::Break` stops the walk.
pub fn walk_location(
    location: &Location,
    mut visit: impl FnMut(Found<'_>) -> ControlFlow<()>,
) -> Result<(), IntakeError> {
    fs::metadata(&location.path).map_err(IntakeError::io(&location.path))?;
    let mut walker = WalkDir::new(&location.path)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter();
    while let Some(step) = walker.next() {
        let entry = match step {
            Ok(found) => found,
            Err(err) => {
                let path = err.path().unwrap_or(&location.path);
                let message = err.to_string();
                if visit(Found::Unreadable {
                    path,
                    message: &message,
                })
                .is_break()
                {
                    return Ok(());
                }
                continue;
            }
        };
        let reported = if entry.depth() > 0 && is_hidden_name(entry.file_name()) {
            if entry.file_type().is_dir() {
                walker.skip_current_dir();
            }
            Found::Hidden
        } else if entry.file_type().is_dir() {
            continue;
        } else {
            Found::File(entry.path())
        };
        if visit(reported).is_break() {
            return Ok(());
        }
    }
    Ok(())
}

/// Walks the location once and takes in every file. A file or directory that
/// cannot be read is counted in the report and does not stop the scan.
pub fn scan_location(
    index: &mut Index,
    location: &Location,
    tracker: &mut StabilityTracker,
    now: SystemTime,
) -> Result<ScanReport, IntakeError> {
    let mut report = ScanReport::default();
    walk_location(location, |found| {
        match found {
            Found::File(path) => match take_in_counted(index, location, path, tracker, now) {
                Ok(taken) => report.count(taken.outcome, path, taken.hashed),
                Err(_failed) => report.count_error(path),
            },
            Found::Hidden => report.count_skip(Skip::Hidden),
            Found::Unreadable { path, .. } => report.count_error(path),
        }
        ControlFlow::Continue(())
    })?;
    let settled = settle_location(index, location)?;
    report.removed_copies = settled.removed_copies;
    report.promoted_copies = settled.promoted_copies;
    report.missing = settled.missing;
    report.error_total += settled.error_total;
    for path in settled.error_paths {
        if report.error_paths.len() < MAX_REPORTED_ERRORS {
            report.error_paths.push(path);
        }
    }
    Ok(report)
}

/// Removes recorded copies whose file is gone and settles documents whose file
/// is gone. Fills only the `removed_copies`, `promoted_copies`, `missing` and
/// error fields of the report.
///
/// The root is checked first: with an unplugged drive every file looks gone,
/// and the whole archive would be marked missing.
pub fn settle_location(index: &Index, location: &Location) -> Result<ScanReport, IntakeError> {
    fs::metadata(&location.path).map_err(IntakeError::io(&location.path))?;
    let mut report = ScanReport::default();
    remove_vanished_copies(index, location, &mut report)?;
    settle_vanished_documents(index, location, &mut report)?;
    Ok(report)
}

fn remove_vanished_copies(
    index: &Index,
    location: &Location,
    report: &mut ScanReport,
) -> Result<(), IntakeError> {
    for copy in index.duplicate_paths_under(&location.path)? {
        settle_vanished_copy(index, &copy, report)?;
    }
    Ok(())
}

fn settle_vanished_copy(
    index: &Index,
    copy: &Path,
    report: &mut ScanReport,
) -> Result<(), IntakeError> {
    match fs::symlink_metadata(copy) {
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            if index.remove_duplicate_path(copy)? {
                report.removed_copies += 1;
            }
        }
        Err(_unreadable) => report.count_error(copy),
    }
    Ok(())
}

fn settle_vanished_documents(
    index: &Index,
    location: &Location,
    report: &mut ScanReport,
) -> Result<(), IntakeError> {
    let documents = index.documents_under(&location.path)?;
    if looks_unmounted(location, &documents) {
        return Err(unmounted_error(location));
    }
    for (id, path) in documents {
        settle_vanished_document(index, location, id, &path, report)?;
    }
    let cutoff = Utc::now().timestamp() - MISSING_DOCUMENTS_KEPT_DAYS * 24 * 60 * 60;
    index.purge_missing_beneath(&location.path, cutoff)?;
    Ok(())
}

fn settle_vanished_document(
    index: &Index,
    location: &Location,
    id: DocumentId,
    path: &Path,
    report: &mut ScanReport,
) -> Result<(), IntakeError> {
    match path_exists(path) {
        // Mail texts read before they were left out of the index.
        Ok(true) if classify(location, path) == Err(Skip::MailBody) => {
            index.delete_document(id)?;
        }
        Ok(true) => {}
        Ok(false) => {
            let Some(document) = index.document(id)? else {
                return Ok(());
            };
            match promote_first_copy(index, &document) {
                Ok(true) => report.promoted_copies += 1,
                Ok(false) => {
                    index.mark_document_missing(id)?;
                    report.missing += 1;
                }
                Err(_unreadable) => report.count_error(path),
            }
        }
        Err(_unreadable) => report.count_error(path),
    }
    Ok(())
}

/// A drive that is not mounted can leave its folder behind empty: every
/// document would be marked missing and purged after the retention time.
fn looks_unmounted(location: &Location, documents: &[(DocumentId, PathBuf)]) -> bool {
    !documents.is_empty()
        && fs::read_dir(&location.path).is_ok_and(|mut entries| entries.next().is_none())
        && documents
            .iter()
            .all(|(_, path)| matches!(path_exists(path), Ok(false)))
}

fn unmounted_error(location: &Location) -> IntakeError {
    IntakeError::Io {
        path: location.path.clone(),
        source: io::Error::other(
            "the folder is empty but the index holds documents of it; it may be a drive that is not mounted",
        ),
    }
}

/// Settles what the index holds at `gone`, a file or a folder the watcher saw
/// deleted or moved out: copies and documents whose file is gone, like the
/// scan does for the whole location. A path that exists again, or that the
/// index no longer knows (the app's own filing repointed it), changes nothing.
/// Fills only the `removed_copies`, `promoted_copies`, `missing` and error
/// fields of the report.
pub fn settle_gone_path(
    index: &Index,
    location: &Location,
    gone: &Path,
) -> Result<ScanReport, IntakeError> {
    fs::metadata(&location.path).map_err(IntakeError::io(&location.path))?;
    let mut report = ScanReport::default();
    // The app's own move is under way: the file left before the index was
    // repointed, and the repoint follows.
    if index.has_pending_source_under(gone)? {
        return Ok(report);
    }
    if fs::read_dir(&location.path).is_ok_and(|mut entries| entries.next().is_none())
        && looks_unmounted(location, &index.documents_under(&location.path)?)
    {
        return Err(unmounted_error(location));
    }
    // The subtree queries list what lies below a folder, not the path itself.
    settle_vanished_copy(index, gone, &mut report)?;
    for copy in index.duplicate_paths_under(gone)? {
        settle_vanished_copy(index, &copy, &mut report)?;
    }
    if let Some(document) = index.document_by_path(gone)? {
        if document.missing_since.is_none() {
            settle_vanished_document(index, location, document.id, gone, &mut report)?;
        }
    }
    for (id, path) in index.documents_under(gone)? {
        settle_vanished_document(index, location, id, &path, &mut report)?;
    }
    Ok(report)
}

/// How long the text and fields of a document whose file is gone stay in the
/// index, in case the file comes back or was only moved.
const MISSING_DOCUMENTS_KEPT_DAYS: i64 = 30;
