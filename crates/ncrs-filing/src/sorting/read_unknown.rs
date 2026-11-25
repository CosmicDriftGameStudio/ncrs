//! What the index knows about a file, and reading a file it does not know yet
//! the way the pipeline does, so it is read once.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ncrs_index::{
    ContentHash, Document, DocumentId, DocumentStatus, Field, FieldOrigin, FieldOrigins, FileStamp,
    Index, ReviewReason,
};
use ncrs_intake::{
    detect_fields, finish_take_in, hash_path, open_below, prepare_take_in, text_is_readable,
    ChainReading, Location, Outcome, OwnNames, Prepared, Skip, StabilityTracker, TextError,
    FIELDS_READ_VERSION, STABLE_AFTER,
};

use super::access::{text, SortingAccess};

/// Whether a plan reads the files the index does not know yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadUnknown {
    Yes,
    No,
}

/// Why a file was not read; it is never taken for a file without text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadProblem {
    /// The index does not know the file, or not its text.
    Unknown,
    StillWriting,
    /// No file the text can be read of.
    Unsupported,
    LibraryMissing,
    Failed(String),
}

pub struct KnownFile {
    pub document: Document,
    pub hash: ContentHash,
    pub text: String,
    pub origins: FieldOrigins,
    /// The other places of the same content that are still there.
    pub known_at: Vec<PathBuf>,
    /// The user decided, or took a run back: nothing is decided for it again.
    pub aside: bool,
    /// The file was copied from here; what stays here is done with.
    pub copied_from_here: bool,
}

pub struct NotRead {
    pub hash: Option<ContentHash>,
    pub document: Option<DocumentId>,
    pub problem: ReadProblem,
}

pub enum Found {
    /// The user excluded the file from filing, or it is gone.
    Nothing,
    NotRead(NotRead),
    Known(Box<KnownFile>),
}

impl Found {
    fn problem(problem: ReadProblem) -> Self {
        Self::NotRead(NotRead {
            hash: None,
            document: None,
            problem,
        })
    }
}

pub fn find(
    access: &SortingAccess,
    location: &Location,
    path: &Path,
    stamp: FileStamp,
    read: ReadUnknown,
) -> Result<Found, String> {
    if !text_is_readable(path) {
        let excluded = access.with_index(|index| index.is_excluded(path).map_err(text))?;
        return Ok(if excluded {
            Found::Nothing
        } else {
            Found::problem(ReadProblem::Unsupported)
        });
    }
    let by_stamp = access.with_index(|index| {
        if index.is_excluded(path).map_err(text)? {
            return Ok(None);
        }
        by_stamp(index, path, stamp).map(Some)
    })?;
    let Some(document_by_stamp) = by_stamp else {
        return Ok(Found::Nothing);
    };
    let known = match document_by_stamp {
        Some(document) => Some(document),
        None => {
            let hash = match hash_path(path) {
                Ok(hash) => hash,
                Err(_) if !path.exists() => return Ok(Found::Nothing),
                Err(err) => return Ok(Found::problem(ReadProblem::Failed(err.to_string()))),
            };
            access.with_index(|index| index.document_by_hash(&hash).map_err(text))?
        }
    };
    let document = match (known, read) {
        (Some(document), _) => document,
        (None, ReadUnknown::No) => return Ok(Found::problem(ReadProblem::Unknown)),
        (None, ReadUnknown::Yes) => match take_in(access, location, path, stamp)? {
            TakenIn::Document(document) => *document,
            TakenIn::Excluded => return Ok(Found::Nothing),
            TakenIn::Problem(problem) => return Ok(Found::problem(problem)),
        },
    };
    if let Some(problem) = read_missing_text(access, location, path, stamp, document.id, read)? {
        return Ok(Found::NotRead(NotRead {
            hash: Some(document.hash),
            document: Some(document.id),
            problem,
        }));
    }
    load(access, document.id, path, stamp)
}

fn by_stamp(index: &Index, path: &Path, stamp: FileStamp) -> Result<Option<Document>, String> {
    if let Some(document) = index.document_by_path(path).map_err(text)? {
        return Ok((document.stamp == Some(stamp)).then_some(document));
    }
    match index.document_by_duplicate_path(path).map_err(text)? {
        Some((id, Some(known))) if known == stamp => index.document(id).map_err(text),
        _ => Ok(None),
    }
}

enum TakenIn {
    Document(Box<Document>),
    Excluded,
    Problem(ReadProblem),
}

fn take_in(
    access: &SortingAccess,
    location: &Location,
    path: &Path,
    stamp: FileStamp,
) -> Result<TakenIn, String> {
    let now = SystemTime::now();
    let written_just_now = now
        .duration_since(stamp.mtime)
        .is_ok_and(|age| age < STABLE_AFTER);
    if written_just_now {
        return Ok(TakenIn::Problem(ReadProblem::StillWriting));
    }
    // A plan sees a file once: what the pipeline learns by looking twice is
    // told from how long the file has not been written to.
    let mut tracker = StabilityTracker::new();
    let earlier = now.checked_sub(STABLE_AFTER).unwrap_or(now);
    tracker.observe(path, stamp.size, stamp.mtime, earlier);
    let prepared = access.with_index(|index| {
        prepare_take_in(index, location, path, &mut tracker, now).map_err(text)
    })?;
    let taken = match prepared {
        Prepared::Done(taken) => taken,
        Prepared::Hash(pending) => {
            let Some(hash) = pending.hash(path).map_err(text)? else {
                return Ok(TakenIn::Problem(ReadProblem::StillWriting));
            };
            access.with_index_mut(|index| {
                finish_take_in(index, location, path, pending, hash, now).map_err(text)
            })?
        }
    };
    let id = match taken.outcome {
        Outcome::New(id)
        | Outcome::Moved(id)
        | Outcome::Copy(id)
        | Outcome::AlreadyFiled(id)
        | Outcome::Unchanged(id)
        | Outcome::ContentChanged(id) => id,
        Outcome::Skipped(Skip::Excluded) => return Ok(TakenIn::Excluded),
        Outcome::Skipped(_) => return Ok(TakenIn::Problem(ReadProblem::Unsupported)),
        Outcome::NotStableYet => return Ok(TakenIn::Problem(ReadProblem::StillWriting)),
    };
    let document = access.with_index(|index| index.document(id).map_err(text))?;
    Ok(
        document.map_or(TakenIn::Problem(ReadProblem::Unknown), |found| {
            TakenIn::Document(Box::new(found))
        }),
    )
}

fn read_missing_text(
    access: &SortingAccess,
    location: &Location,
    path: &Path,
    stamp: FileStamp,
    id: DocumentId,
    read: ReadUnknown,
) -> Result<Option<ReadProblem>, String> {
    if access.with_index(|index| index.has_document_text_row(id).map_err(text))? {
        return Ok(None);
    }
    if read == ReadUnknown::No {
        return Ok(Some(ReadProblem::Unknown));
    }
    let found = match read_text(access, location, path) {
        Ok(found) => found,
        Err(problem) => return Ok(Some(problem)),
    };
    if stamp_at(path) != Some(stamp) {
        return Ok(Some(ReadProblem::StillWriting));
    }
    access.with_index_mut(|index| store(access, index, id, &found))?;
    Ok(None)
}

fn read_text(
    access: &SortingAccess,
    location: &Location,
    path: &Path,
) -> Result<String, ReadProblem> {
    let file = open_below(&location.path, path)
        .map_err(|err| ReadProblem::Failed(format!("{}: {err}", path.display())))?;
    match access.text_chain.read_open(path, &file) {
        Ok(ChainReading::Text(found)) => Ok(found),
        Ok(ChainReading::NoText { .. }) => Ok(String::new()),
        Err(TextError::LibraryMissing { .. }) => Err(ReadProblem::LibraryMissing),
        Err(err) => Err(ReadProblem::Failed(err.to_string())),
    }
}

fn store(
    access: &SortingAccess,
    index: &mut Index,
    id: DocumentId,
    found: &str,
) -> Result<(), String> {
    index.set_document_text(id, found).map_err(text)?;
    if found.trim().is_empty() {
        return Ok(());
    }
    let Some(document) = index.document(id).map_err(text)? else {
        return Ok(());
    };
    let aliases = index.confirmed_sender_aliases().map_err(text)?;
    let own = OwnNames {
        names: &access.own.names,
        ibans: &access.own.ibans,
        tax_ids: &access.own.tax_ids,
    };
    let filled = detect_fields(found, &document.fields, &aliases, &own, index.kinds())
        .fill(&document.fields);
    if filled != document.fields {
        index
            .set_document_fields(id, &filled, |field| match field {
                Field::Recipient => FieldOrigin::AddressBlock,
                Field::Kind => FieldOrigin::Heading,
                _ => FieldOrigin::Detected,
            })
            .map_err(text)?;
    }
    index
        .set_fields_read_version(id, FIELDS_READ_VERSION)
        .map_err(text)
}

fn load(
    access: &SortingAccess,
    id: DocumentId,
    path: &Path,
    stamp: FileStamp,
) -> Result<Found, String> {
    let known = access.with_index(|index| {
        let Some(document) = index.document(id).map_err(text)? else {
            return Ok(None);
        };
        let question = index.open_review_of(id).map_err(text)?;
        let aside = document.status == DocumentStatus::InReview
            && question.is_none_or(|entry| entry.reason == ReviewReason::Undone);
        let mut known_at = index.duplicate_paths(id).map_err(text)?;
        known_at.push(document.path.clone());
        let copied_from = index
            .filed_document_for_inbox_path(path, stamp)
            .map_err(text)?;
        Ok(Some(KnownFile {
            hash: document.hash,
            text: index.document_text(id).map_err(text)?.unwrap_or_default(),
            origins: index.field_origins(id).map_err(text)?,
            known_at,
            aside,
            copied_from_here: copied_from.is_some(),
            document,
        }))
    })?;
    Ok(known.map_or_else(
        || Found::problem(ReadProblem::Unknown),
        |mut file| {
            file.known_at
                .retain(|place| place != path && place.exists());
            Found::Known(Box::new(file))
        },
    ))
}

pub fn stamp_at(path: &Path) -> Option<FileStamp> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    Some(FileStamp {
        size: metadata.len(),
        mtime: metadata.modified().ok()?,
    })
}
