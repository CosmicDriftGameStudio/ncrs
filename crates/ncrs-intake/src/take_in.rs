use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use ncrs_index::{
    ContentHash, Document, DocumentId, DocumentSource, DocumentStatus, FileStamp, Index,
    NewDocument,
};
use sha2::{Digest as _, Sha256};

use crate::classify::{classify, Skip};
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

#[derive(Debug)]
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
    match prepare_take_in(index, location, path, tracker, now)? {
        Prepared::Done(taken) => Ok(taken),
        Prepared::Hash(pending) => match pending.hash(path)? {
            Some(hash) => finish_take_in(index, location, path, pending, hash, now),
            None => Ok(Taken {
                outcome: Outcome::NotStableYet,
                hashed: false,
            }),
        },
    }
}

/// A file that passed everything up to its hash. Hashing reads the whole file
/// and needs no index, so a caller that shares the index can let go of it
/// between [`prepare_take_in`] and [`finish_take_in`].
#[derive(Debug, Clone)]
pub struct PendingHash {
    stamp: FileStamp,
    root: PathBuf,
}

impl PendingHash {
    /// `None` when the file changed since it was stamped.
    pub fn hash(&self, path: &Path) -> Result<Option<ContentHash>, IntakeError> {
        hash_file(&self.root, path, self.stamp)
    }
}

#[derive(Debug)]
pub enum Prepared {
    Done(Taken),
    Hash(PendingHash),
}

/// The part of taking a file in that comes before its hash.
pub fn prepare_take_in(
    index: &Index,
    location: &Location,
    path: &Path,
    tracker: &mut StabilityTracker,
    now: SystemTime,
) -> Result<Prepared, IntakeError> {
    let without_hash = |outcome| {
        Prepared::Done(Taken {
            outcome,
            hashed: false,
        })
    };
    if let Err(skip) = classify(location, path) {
        return Ok(without_hash(Outcome::Skipped(skip)));
    }
    if index.is_excluded(path)? {
        return Ok(without_hash(Outcome::Skipped(Skip::Excluded)));
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
    Ok(Prepared::Hash(PendingHash {
        stamp,
        root: location.path.clone(),
    }))
}

/// The part of taking a file in that comes after its hash.
pub fn finish_take_in(
    index: &mut Index,
    location: &Location,
    path: &Path,
    pending: PendingHash,
    hash: ContentHash,
    now: SystemTime,
) -> Result<Taken, IntakeError> {
    let stamp = pending.stamp;
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
        index.promote_copy_to_document_path(document.id, &copy, current)?;
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
    has_symlink_ancestor_under(&location.path, path)
}

/// `has_symlink_ancestor` for a root and a path that may not exist yet: a
/// folder that is not there cannot be a link, and nothing below it exists.
pub fn has_symlink_ancestor_under(root: &Path, path: &Path) -> Result<bool, IntakeError> {
    let Ok(relative) = path.strip_prefix(root) else {
        return Ok(false);
    };
    if !relative
        .components()
        .all(|part| matches!(part, std::path::Component::Normal(_)))
    {
        return Ok(false);
    }
    let mut current = root.to_path_buf();
    let mut parts = relative.components().peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            break;
        }
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Ok(true),
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(IntakeError::io(&current)(err)),
        }
    }
    Ok(false)
}

/// Opens without following a link and without blocking on a FIFO, since the
/// path may have been swapped since it was looked at.
fn open_regular(path: &Path) -> Result<File, IntakeError> {
    open_without_following(path).map_err(IntakeError::io(path))
}

/// [`open_regular`] with the plain I/O error, for a caller that maps it itself.
pub fn open_without_following(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(
        &mut options,
        libc::O_NOFOLLOW | libc::O_NONBLOCK,
    );
    options.open(path)
}

/// [`open_without_following`] for a file below `root`, where no folder on the
/// way may be a link either: `O_NOFOLLOW` guards the last part only, and a
/// folder swapped for a link after [`has_symlink_ancestor_under`] looked would
/// lead the open out of the location. Each part is opened relative to the one
/// before it, so a swap cannot redirect the walk.
#[cfg(unix)]
fn open_below(root: &Path, path: &Path) -> io::Result<File> {
    use nix::fcntl::{openat, OFlag};
    use nix::sys::stat::Mode;
    use std::path::Component;

    let Ok(relative) = path.strip_prefix(root) else {
        return open_without_following(path);
    };
    let parts: Vec<&std::ffi::OsStr> = relative
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    if parts.is_empty() || parts.len() != relative.components().count() {
        return open_without_following(path);
    }
    let mut current = std::os::fd::OwnedFd::from(File::open(root)?);
    let folder_flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    for (position, name) in parts.iter().enumerate() {
        let last = position + 1 == parts.len();
        let flags = if last {
            OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC
        } else {
            folder_flags
        };
        current = openat(&current, *name, flags, Mode::empty()).map_err(io::Error::from)?;
    }
    Ok(File::from(current))
}

#[cfg(not(unix))]
fn open_below(_root: &Path, path: &Path) -> io::Result<File> {
    open_without_following(path)
}

fn hash_open(file: &mut File, path: &Path) -> Result<ContentHash, IntakeError> {
    let mut hasher = Sha256::new();
    let mut chunk: [u8; 64 * 1024] = [0; 64 * 1024];
    loop {
        match io::Read::read(file, &mut chunk) {
            Ok(0) => break,
            Ok(read) => hasher.update(chunk.get(..read).unwrap_or_default()),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(IntakeError::io(path)(err)),
        }
    }
    Ok(ContentHash(hasher.finalize().into()))
}

/// `None` when what was opened is not the file that was stamped: it is still
/// changing.
fn hash_file(
    root: &Path,
    path: &Path,
    stamp: FileStamp,
) -> Result<Option<ContentHash>, IntakeError> {
    let mut file = open_below(root, path).map_err(IntakeError::io(path))?;
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
            index.promote_copy_to_document_path(known.id, path, stamp)?;
        } else {
            index.set_document_path(known.id, path, stamp)?;
        }
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn the_content_hash_is_the_sha256_of_the_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc.txt");
        fs::write(&path, b"abc").unwrap();
        let hex: String = hash_path(&path)
            .unwrap()
            .0
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_swapped_for_a_link_after_the_check_is_not_followed_when_hashing() {
        let location = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let folder = location.path().join("scans");
        fs::create_dir(&folder).unwrap();
        let path = folder.join("a.txt");
        fs::write(&path, b"inside").unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let stamp = FileStamp {
            size: metadata.len(),
            mtime: metadata.modified().unwrap(),
        };
        let pending = PendingHash {
            stamp,
            root: location.path().to_path_buf(),
        };
        assert!(pending.hash(&path).unwrap().is_some());

        // Same name, size and mtime behind the link: only the walk can tell.
        let secret = outside.path().join("a.txt");
        fs::write(&secret, b"secret").unwrap();
        File::options()
            .write(true)
            .open(&secret)
            .unwrap()
            .set_modified(stamp.mtime)
            .unwrap();
        fs::remove_dir_all(&folder).unwrap();
        std::os::unix::fs::symlink(outside.path(), &folder).unwrap();

        pending.hash(&path).unwrap_err();
    }
}
