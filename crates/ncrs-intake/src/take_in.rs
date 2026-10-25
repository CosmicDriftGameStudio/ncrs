use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use ncrs_index::{
    ContentHash, Document, DocumentId, DocumentSource, DocumentStatus, FileStamp, Index,
    NewDocument,
};
use sha2::{Digest as _, Sha256};

use crate::classify::{classify, relative_to_root, Skip};
use crate::error::IntakeError;
use crate::location::{Location, LocationMode};
use crate::stability::StabilityTracker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    New(DocumentId),
    Moved(DocumentId),
    /// Same content as the document, recorded as another place for it.
    Copy(DocumentId),
    /// An inbox file whose content is already filed. It stays where it is and
    /// nothing is recorded.
    AlreadyFiled(DocumentId),
    Unchanged(DocumentId),
    ContentChanged(DocumentId),
    Skipped(Skip),
    NotStableYet,
}

/// Takes one file in. The file is only read; the index is the only thing
/// written.
pub fn take_in(
    index: &mut Index,
    location: &Location,
    path: &Path,
    tracker: &mut StabilityTracker,
    now: SystemTime,
) -> Result<Outcome, IntakeError> {
    take_in_counted(index, location, path, tracker, now).map(|taken| taken.outcome)
}

pub struct Taken {
    pub outcome: Outcome,
    pub hashed: bool,
}

pub fn take_in_counted(
    index: &mut Index,
    location: &Location,
    path: &Path,
    tracker: &mut StabilityTracker,
    now: SystemTime,
) -> Result<Taken, IntakeError> {
    let without_hash = |outcome| Taken {
        outcome,
        hashed: false,
    };
    if let Err(skip) = classify(location, path) {
        return Ok(without_hash(Outcome::Skipped(skip)));
    }
    if has_symlink_ancestor(location, path)? {
        return Ok(without_hash(Outcome::Skipped(Skip::Symlink)));
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) => {
            tracker.forget(path);
            return Err(IntakeError::io(path)(err));
        }
    };
    if metadata.file_type().is_symlink() {
        return Ok(without_hash(Outcome::Skipped(Skip::Symlink)));
    }
    if !metadata.is_file() {
        return Ok(without_hash(Outcome::Skipped(Skip::Unsupported)));
    }
    let stamp = FileStamp {
        size: metadata.len(),
        mtime: metadata.modified().map_err(IntakeError::io(path))?,
    };
    if let Some(id) = unchanged_by_stamp(index, path, stamp)? {
        return Ok(without_hash(Outcome::Unchanged(id)));
    }
    if !tracker.observe(path, stamp.size, stamp.mtime, now) {
        return Ok(without_hash(Outcome::NotStableYet));
    }
    let Some(hash) = hash_file(path, stamp)? else {
        return Ok(without_hash(Outcome::NotStableYet));
    };
    let outcome = file_with_hash(index, location, path, stamp, hash, now)?;
    Ok(Taken {
        outcome,
        hashed: true,
    })
}

/// A path whose size and mtime are the stored ones needs no hashing. A recorded
/// copy only counts while its document's own file is still there: otherwise
/// the copy has to become the document, which takes the hash.
fn unchanged_by_stamp(
    index: &Index,
    path: &Path,
    stamp: FileStamp,
) -> Result<Option<DocumentId>, IntakeError> {
    if let Some(document) = index.document_by_path(path)? {
        return Ok((document.stamp == Some(stamp)).then_some(document.id));
    }
    let Some((id, Some(recorded))) = index.document_by_duplicate_path(path)? else {
        return Ok(None);
    };
    if recorded != stamp {
        return Ok(None);
    }
    let Some(document) = index.document(id)? else {
        return Ok(None);
    };
    Ok(path_exists(&document.path)?.then_some(id))
}

/// Makes the first recorded copy of the document, in path order, the
/// document's file. Only a copy that is unchanged since it was recorded
/// qualifies: a changed one may no longer have the document's bytes.
pub fn promote_first_copy(index: &Index, document: &Document) -> Result<bool, IntakeError> {
    for copy in index.duplicate_paths(document.id)? {
        let Some((_, Some(recorded))) = index.document_by_duplicate_path(&copy)? else {
            continue;
        };
        let metadata = match fs::symlink_metadata(&copy) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(IntakeError::io(&copy)(err)),
        };
        let current = FileStamp {
            size: metadata.len(),
            mtime: metadata.modified().map_err(IntakeError::io(&copy))?,
        };
        if !metadata.is_file() || current != recorded {
            continue;
        }
        index.remove_duplicate_path(&copy)?;
        index.set_document_path(document.id, &copy, current)?;
        return Ok(true);
    }
    Ok(false)
}

pub fn path_exists(path: &Path) -> Result<bool, IntakeError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(IntakeError::io(path)(err)),
    }
}

/// A directory between the root and the file that is a link could lead out of
/// the location, however the file itself looks. The file's own name is checked
/// by the caller.
fn has_symlink_ancestor(location: &Location, path: &Path) -> Result<bool, IntakeError> {
    let Some(relative) = relative_to_root(location, path) else {
        return Ok(false);
    };
    let mut current = location.path.clone();
    let mut parts = relative.components().peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            break;
        }
        current.push(part);
        let metadata = fs::symlink_metadata(&current).map_err(IntakeError::io(&current))?;
        if metadata.file_type().is_symlink() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Opens without following a link and without blocking on a FIFO, since the
/// path may have been swapped since it was looked at.
fn open_regular(path: &Path) -> Result<File, IntakeError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(
        &mut options,
        libc::O_NOFOLLOW | libc::O_NONBLOCK,
    );
    options.open(path).map_err(IntakeError::io(path))
}

fn hash_open(file: &mut File, path: &Path) -> Result<ContentHash, IntakeError> {
    let mut hasher = Sha256::new();
    io::copy(file, &mut hasher).map_err(IntakeError::io(path))?;
    Ok(ContentHash(hasher.finalize().into()))
}

/// `None` when what was opened is not the file that was stamped: it is still
/// changing.
fn hash_file(path: &Path, stamp: FileStamp) -> Result<Option<ContentHash>, IntakeError> {
    let mut file = open_regular(path)?;
    let opened = file.metadata().map_err(IntakeError::io(path))?;
    let unchanged = opened.is_file()
        && opened.len() == stamp.size
        && opened.modified().map_err(IntakeError::io(path))? == stamp.mtime;
    if !unchanged {
        return Ok(None);
    }
    hash_open(&mut file, path).map(Some)
}

/// The hash a document is known by, of the regular file at `path`, without a
/// stamp to compare with. A link, a FIFO or a folder is an error.
pub fn hash_path(path: &Path) -> Result<ContentHash, IntakeError> {
    let mut file = open_regular(path)?;
    let opened = file.metadata().map_err(IntakeError::io(path))?;
    if !opened.is_file() {
        return Err(IntakeError::io(path)(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        )));
    }
    hash_open(&mut file, path)
}

/// `DateTime::from(SystemTime)` panics on values chrono cannot hold, and a
/// file's mtime is whatever the file system says.
fn timestamp(time: SystemTime) -> Option<DateTime<Utc>> {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => {
            DateTime::from_timestamp(i64::try_from(after.as_secs()).ok()?, after.subsec_nanos())
        }
        Err(earlier) => {
            let before = earlier.duration();
            let seconds = i64::try_from(before.as_secs()).ok()?;
            match before.subsec_nanos() {
                0 => DateTime::from_timestamp(-seconds, 0),
                nanos => DateTime::from_timestamp(-seconds - 1, 1_000_000_000 - nanos),
            }
        }
    }
}

fn file_with_hash(
    index: &mut Index,
    location: &Location,
    path: &Path,
    stamp: FileStamp,
    hash: ContentHash,
    now: SystemTime,
) -> Result<Outcome, IntakeError> {
    if let Some(document) = index.document_by_path(path)? {
        if document.hash == hash {
            index.set_document_path(document.id, path, stamp)?;
            return Ok(Outcome::Unchanged(document.id));
        }
        // Content follows the hash: the document never points at bytes with
        // another hash. Its identity moves to a copy that still has the old
        // bytes; failing that, it is edited in place if the new content is
        // unknown, and otherwise it is missing because its content is gone.
        if !promote_first_copy(index, &document)? {
            if index.document_by_hash(&hash)?.is_none() {
                index.replace_document_content(document.id, &hash, stamp)?;
                return Ok(Outcome::ContentChanged(document.id));
            }
            index.mark_document_missing(document.id)?;
        }
    }
    match index.document_by_hash(&hash)? {
        Some(known) => file_with_known_hash(index, location, path, stamp, &known),
        None => insert_new(index, location, path, stamp, hash, now),
    }
}

fn file_with_known_hash(
    index: &Index,
    location: &Location,
    path: &Path,
    stamp: FileStamp,
    known: &Document,
) -> Result<Outcome, IntakeError> {
    let recorded_copy = index.document_by_duplicate_path(path)?;
    if known.missing_since.is_some() || has_moved_here(&known.path, path)? {
        if recorded_copy.is_some() {
            index.remove_duplicate_path(path)?;
        }
        index.set_document_path(known.id, path, stamp)?;
        if location.mode == LocationMode::Archive && known.status != DocumentStatus::Filed {
            index.set_document_status(known.id, DocumentStatus::Filed)?;
        }
        return Ok(Outcome::Moved(known.id));
    }
    if matches!(recorded_copy, Some((id, _)) if id == known.id) {
        index.record_duplicate_path(known.id, path, stamp)?;
        return Ok(Outcome::Unchanged(known.id));
    }
    // The status decides, not the source: a document filed from the inbox
    // keeps source Inbox.
    if location.mode == LocationMode::Inbox && known.status == DocumentStatus::Filed {
        return Ok(Outcome::AlreadyFiled(known.id));
    }
    index.record_duplicate_path(known.id, path, stamp)?;
    Ok(Outcome::Copy(known.id))
}

/// The document's stored file is gone, or is this very file under another
/// spelling: a case-only rename on case-insensitive APFS keeps the old name
/// existing, so `exists()` alone would call the renamed file a copy.
fn has_moved_here(stored: &Path, path: &Path) -> Result<bool, IntakeError> {
    if !path_exists(stored)? {
        return Ok(true);
    }
    same_file::is_same_file(stored, path).map_err(IntakeError::io(path))
}

fn insert_new(
    index: &mut Index,
    location: &Location,
    path: &Path,
    stamp: FileStamp,
    hash: ContentHash,
    now: SystemTime,
) -> Result<Outcome, IntakeError> {
    // A file that was a recorded copy and now has content of its own.
    index.remove_duplicate_path(path)?;
    let (source, preferred) = match location.mode {
        LocationMode::Inbox => (DocumentSource::Inbox, timestamp(now)),
        LocationMode::Archive => (DocumentSource::Archive, timestamp(stamp.mtime)),
    };
    let received_at = preferred
        .or_else(|| timestamp(now))
        .unwrap_or_else(Utc::now);
    let id = index.insert_document(&NewDocument {
        hash,
        path: path.to_path_buf(),
        source,
        received_at,
        stamp: Some(stamp),
    })?;
    if location.mode == LocationMode::Archive {
        index.set_document_status(id, DocumentStatus::Filed)?;
    }
    Ok(Outcome::New(id))
}
