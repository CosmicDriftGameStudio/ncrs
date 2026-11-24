//! Reading a folder for the chat: what is in it, which order it follows and
//! which files are duplicates that can go. Writes nothing.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::SystemTime;

use ncrs_index::{ContentHash, Document, DocumentFields, FileStamp, Kinds, SenderAliases};
use ncrs_intake::{
    date_in_file_stem, detect_fields, duplicate_copies, hash_path, is_eml_file, is_image_file,
    is_mail_body, learn_archive, location_structure, ArchiveDocument, LocationStructure, OwnNames,
    PlanFile, TextError,
};
use ncrs_pipeline::Load;
use ncrs_priority::{set_current_thread_priority, ThreadPriority};

use super::{ChatAccess, TrashItem};

/// Most files the chat reads in one folder.
pub const FILE_LIMIT: usize = 20_000;

#[derive(Debug, Clone, PartialEq)]
pub struct FolderReading {
    pub root: PathBuf,
    /// Sorted by relative path.
    pub files: Vec<PlanFile>,
    /// Images no text could be read from.
    pub photos: usize,
    /// Other files no text could be read from.
    pub without_text: usize,
    pub structure: LocationStructure,
    /// Files whose content lies in the same folder tree in a better place.
    pub duplicates: Vec<TrashItem>,
    /// The first file (by path) of each content.
    pub hashes: HashMap<ContentHash, PathBuf>,
    /// How far the AI got with the documents of the folder the index knows.
    pub ai: AiCoverage,
    /// The kinds the index knew when the folder was read, its own among them.
    pub kinds: Kinds,
}

/// Documents with a text, and how many of them the AI has understood.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AiCoverage {
    pub with_text: usize,
    pub understood: usize,
}

impl AiCoverage {
    pub fn is_complete(self) -> bool {
        self.understood >= self.with_text
    }
}

/// How far a running read is: the files found while walking, the number to
/// read once the walk is done, and the files read so far.
#[derive(Debug, Default)]
pub struct ReadProgress {
    pub found: AtomicUsize,
    pub total: AtomicUsize,
    pub done: AtomicUsize,
}

impl ReadProgress {
    pub fn found(&self) -> usize {
        self.found.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }

    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    NotADirectory,
    /// The folder is not there: deleted, or its drive is gone.
    Gone,
    TooMany {
        limit: usize,
    },
    Io(String),
}

struct Walked {
    relative: PathBuf,
    stamp: FileStamp,
}

fn read_error(folder: &Path, err: &io::Error) -> ReadError {
    if err.kind() == io::ErrorKind::NotFound {
        ReadError::Gone
    } else {
        ReadError::Io(format!("{}: {err}", folder.display()))
    }
}

/// A folder or entry that vanishes below the root while it is walked is
/// skipped; only the root being gone ends the read.
fn walk_error(folder: &Path, relative: &Path, err: &io::Error) -> Result<(), ReadError> {
    if err.kind() == io::ErrorKind::NotFound && !relative.as_os_str().is_empty() {
        Ok(())
    } else {
        Err(read_error(folder, err))
    }
}

fn walk(
    base: &Path,
    relative: &Path,
    found: &mut Vec<Walked>,
    progress: &ReadProgress,
) -> Result<(), ReadError> {
    let folder = base.join(relative);
    let entries = match fs::read_dir(&folder) {
        Ok(entries) => entries,
        Err(err) => return walk_error(&folder, relative, &err),
    };
    let mut dir_entries = Vec::new();
    for entry in entries {
        match entry {
            Ok(dir_entry) => dir_entries.push(dir_entry),
            Err(err) => walk_error(&folder, relative, &err)?,
        }
    }
    let has_eml = dir_entries
        .iter()
        .any(|dir_entry| is_eml_file(&dir_entry.file_name()));
    for dir_entry in dir_entries {
        let name = dir_entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(file_type) = dir_entry.file_type() else {
            continue;
        };
        let path = relative.join(&name);
        if file_type.is_dir() {
            walk(base, &path, found, progress)?;
        } else if !file_type.is_symlink() && !is_eml_file(&name) && !is_mail_body(&name, has_eml) {
            // A file that cannot be stat'ed cannot be read or filed either.
            let Some(stamp) = dir_entry.metadata().ok().and_then(|metadata| {
                Some(FileStamp {
                    size: metadata.len(),
                    mtime: metadata.modified().ok()?,
                })
            }) else {
                continue;
            };
            found.push(Walked {
                relative: path,
                stamp,
            });
            progress.found.store(found.len(), Ordering::Relaxed);
            if found.len() > FILE_LIMIT {
                return Err(ReadError::TooMany { limit: FILE_LIMIT });
            }
        }
    }
    Ok(())
}

fn modified_date(mtime: SystemTime) -> Option<chrono::NaiveDate> {
    let since_epoch = mtime.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    let seconds = i64::try_from(since_epoch.as_secs()).ok()?;
    let utc = chrono::DateTime::from_timestamp(seconds, 0)?;
    Some(utc.with_timezone(&chrono::Local).date_naive())
}

fn date_in_file_name(relative: &Path) -> Option<chrono::NaiveDate> {
    date_in_file_stem(relative.file_stem()?.to_str()?)
}

/// What the index knows of a file: its hash, fields and full text.
struct Known {
    hash: ContentHash,
    fields: DocumentFields,
    text: Option<String>,
}

/// `work` on every item, spread over the threads the load allows, each at the
/// load's priority; the results keep the order.
fn in_parallel<T: Sync, R: Send>(items: &[T], load: Load, work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let cores = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let threads = usize::try_from(load.ai_threads(cores).get())
        .unwrap_or(1)
        .min(8);
    in_threads(items, threads, load.thread_priority(), work)
}

fn in_threads<T: Sync, R: Send>(
    items: &[T],
    threads: usize,
    priority: ThreadPriority,
    work: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    let chunk = items.len().div_ceil(threads).max(1);
    let work_ref = &work;
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    set_current_thread_priority(priority);
                    part.iter().map(work_ref).collect::<Vec<R>>()
                })
            })
            .collect();
        handles
            .into_iter()
            // A lost chunk would shift every later zip of results and inputs.
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
            })
            .collect()
    })
}

/// The files `read_folder` walks, relative to the folder and in its order;
/// files that cannot be hashed are still among them.
pub fn list_files(folder: &Path) -> Result<Vec<PathBuf>, ReadError> {
    let walked = walk_sorted(folder, &ReadProgress::default())?;
    Ok(walked.into_iter().map(|file| file.relative).collect())
}

fn walk_sorted(folder: &Path, progress: &ReadProgress) -> Result<Vec<Walked>, ReadError> {
    match fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(ReadError::NotADirectory),
        Err(err) => return Err(read_error(folder, &err)),
    }
    let mut walked = Vec::new();
    walk(folder, Path::new(""), &mut walked, progress)?;
    walked.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(walked)
}

fn is_stopped(stop: Option<&AtomicBool>) -> bool {
    stop.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// A set `stop` skips the hashing and reading of the files not yet started, so
/// the reading then lacks them and must not be used after stopping.
pub fn read_folder(
    access: &ChatAccess,
    folder: &Path,
    progress: &ReadProgress,
    stop: Option<&AtomicBool>,
) -> Result<FolderReading, ReadError> {
    read_folder_only(
        access,
        folder,
        progress,
        stop,
        None,
        UnknownFiles::Extract,
    )
}

/// What becomes of a file the index does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownFiles {
    /// Its text is read, which for a scan means OCR.
    Extract,
    /// It stays out of the picture, as in [`known_structure`]: a folder that
    /// only gives context must not cost the minutes of reading its files.
    Skip,
}

/// [`read_folder`] for the listed files (paths below `folder`) alone: the
/// others are neither looked up in the index nor hashed.
pub fn read_folder_only(
    access: &ChatAccess,
    folder: &Path,
    progress: &ReadProgress,
    stop: Option<&AtomicBool>,
    only: Option<&BTreeSet<&PathBuf>>,
    unknown_files: UnknownFiles,
) -> Result<FolderReading, ReadError> {
    let mut walked = walk_sorted(folder, progress)?;
    if let Some(listed) = only {
        walked.retain(|file| listed.contains(&file.relative));
    }
    progress.total.store(walked.len(), Ordering::Relaxed);

    // Whatever the index has under the folder with the same size and mtime is
    // taken as it is, as the intake does: the file is not opened.
    let (mut known, kinds) = unchanged_in_index(access, folder, &walked).map_err(ReadError::Io)?;
    progress
        .done
        .store(known.iter().flatten().count(), Ordering::Relaxed);

    // A file that cannot be hashed cannot be read or filed either, so it is not part of the picture.
    let changed: Vec<usize> = (0..walked.len())
        .filter(|position| known.get(*position).is_some_and(Option::is_none))
        .collect();
    let changed_hashes = in_parallel(&changed, access.load, |position| {
        if is_stopped(stop) {
            return None;
        }
        let hash = walked
            .get(*position)
            .and_then(|file| hash_path(&folder.join(&file.relative)).ok());
        if hash.is_none() {
            progress.done.fetch_add(1, Ordering::Relaxed);
        }
        hash
    });
    let hashed: Vec<(usize, ContentHash)> = changed
        .into_iter()
        .zip(changed_hashes)
        .filter_map(|(position, hash)| Some((position, hash?)))
        .collect();
    let by_hash = known_by_hash(access, &hashed).map_err(ReadError::Io)?;
    let mut unknown = Vec::new();
    for ((position, hash), stored) in hashed.into_iter().zip(by_hash) {
        match stored {
            Some(record) => {
                progress.done.fetch_add(1, Ordering::Relaxed);
                if let Some(slot) = known.get_mut(position) {
                    *slot = Some(record);
                }
            }
            None if unknown_files == UnknownFiles::Skip => {
                progress.done.fetch_add(1, Ordering::Relaxed);
            }
            None => unknown.push((position, hash)),
        }
    }

    let names = &access.own.names;
    let own = OwnNames {
        names,
        ibans: &access.own.ibans,
        tax_ids: &access.own.tax_ids,
    };
    let aliases = SenderAliases::default();
    let extracted = in_parallel(&unknown, access.load, |(position, _)| {
        if is_stopped(stop) {
            return None;
        }
        let read = walked
            .get(*position)
            .map(|file| access.text_chain.extract(&folder.join(&file.relative)));
        progress.done.fetch_add(1, Ordering::Relaxed);
        read
    });
    for ((position, hash), read) in unknown.into_iter().zip(extracted) {
        // A file that could not be opened, or whose reader is missing, says
        // nothing about its text: it stays out of the picture like one that
        // cannot be hashed, instead of being tagged as unreadable.
        let text = match read {
            None
            | Some(Err(
                TextError::LibraryMissing { .. }
                | TextError::Io { .. }
                | TextError::EngineUnavailable { .. },
            )) => continue,
            Some(Ok(text)) => text,
            Some(Err(_)) => None,
        };
        let detected = text.as_deref().map(|body| {
            detect_fields(
                body,
                &DocumentFields::default(),
                &aliases,
                &own,
                &kinds.catalog,
            )
        });
        let fields = DocumentFields {
            date: detected.as_ref().and_then(|fields| fields.date),
            sender: detected.as_ref().and_then(|fields| fields.sender.clone()),
            kind: detected.and_then(|fields| fields.kind),
            ..DocumentFields::default()
        };
        if let Some(slot) = known.get_mut(position) {
            *slot = Some(Known { hash, fields, text });
        }
    }

    let mut files = Vec::with_capacity(walked.len());
    let mut photos = 0;
    let mut without_text = 0;
    for (file, known_record) in walked.into_iter().zip(known) {
        let Some(mut record) = known_record else {
            continue;
        };
        record.fields.date = record
            .fields
            .date
            .or_else(|| date_in_file_name(&file.relative))
            .or_else(|| modified_date(file.stamp.mtime));
        if record.text.is_none() {
            if is_image_file(&file.relative) {
                photos += 1;
            } else {
                without_text += 1;
            }
        }
        files.push(PlanFile {
            relative_path: file.relative,
            hash: record.hash,
            fields: record.fields,
            text: record.text,
            recorded_at: i64::try_from(files.len()).unwrap_or(i64::MAX),
        });
    }

    let structure = location_structure(&files, &kinds.catalog);
    let duplicates = duplicates_of(folder, &files, &structure, &kinds);
    let mut hashes = HashMap::new();
    for file in &files {
        hashes
            .entry(file.hash)
            .or_insert_with(|| file.relative_path.clone());
    }
    let reading = FolderReading {
        root: folder.to_path_buf(),
        files,
        photos,
        without_text,
        structure,
        duplicates,
        hashes,
        ai: ai_coverage(access, folder).map_err(ReadError::Io)?,
        kinds: kinds.clone(),
    };
    Ok(reading)
}

/// The order of a folder from what the index already knows of its files.
/// Files the index does not know are left out: nothing is hashed, opened or
/// read for text, so this is cheap enough for a settings page.
pub fn known_structure(access: &ChatAccess, folder: &Path) -> Result<LocationStructure, ReadError> {
    match fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(ReadError::NotADirectory),
        Err(err) => return Err(read_error(folder, &err)),
    }
    let mut walked = Vec::new();
    walk(folder, Path::new(""), &mut walked, &ReadProgress::default())?;
    let (known, kinds) = unchanged_in_index(access, folder, &walked).map_err(ReadError::Io)?;
    let files: Vec<PlanFile> = walked
        .into_iter()
        .zip(known)
        .filter_map(|(file, known_record)| {
            let record = known_record?;
            Some(PlanFile {
                relative_path: file.relative,
                hash: record.hash,
                fields: record.fields,
                text: record.text,
                recorded_at: 0,
            })
        })
        .collect();
    Ok(location_structure(&files, &kinds.catalog))
}

fn ai_coverage(access: &ChatAccess, folder: &Path) -> Result<AiCoverage, String> {
    access
        .with_index(|index| {
            index
                .ai_coverage_beneath(folder)
                .map_err(|err| err.to_string())
        })
        .map(|(with_text, understood)| AiCoverage {
            with_text,
            understood,
        })
}

/// One query for the documents under the folder and one for their texts; a
/// walked file whose path is among them with an equal stamp is known.
fn unchanged_in_index(
    access: &ChatAccess,
    folder: &Path,
    walked: &[Walked],
) -> Result<(Vec<Option<Known>>, Kinds), String> {
    access.with_index(|index| {
        let kinds = Kinds::new(
            std::sync::Arc::clone(index.kinds()),
            access.own.kind_language,
        );
        let mut documents: HashMap<PathBuf, Document> = index
            .documents_beneath(folder)
            .map_err(|err| err.to_string())?
            .into_iter()
            .map(|document| (document.path.clone(), document))
            .collect();
        let mut texts = index
            .document_texts_beneath(folder)
            .map_err(|err| err.to_string())?;
        let known = walked
            .iter()
            .map(|file| {
                let absolute = folder.join(&file.relative);
                let unchanged = documents
                    .get(&absolute)
                    .is_some_and(|document| document.stamp == Some(file.stamp));
                if !unchanged {
                    return None;
                }
                let document = documents.remove(&absolute)?;
                Some(Known {
                    hash: document.hash,
                    fields: document.fields,
                    text: texts.remove(&absolute),
                })
            })
            .collect();
        Ok((known, kinds))
    })
}

/// The documents with the hashes of files that changed or are new.
fn known_by_hash(
    access: &ChatAccess,
    hashed: &[(usize, ContentHash)],
) -> Result<Vec<Option<Known>>, String> {
    access.with_index(|index| {
        let mut known = Vec::with_capacity(hashed.len());
        for (_, hash) in hashed {
            let by_hash = index
                .document_by_hash(hash)
                .map_err(|err| err.to_string())?;
            known.push(match by_hash {
                Some(document) => Some(Known {
                    hash: *hash,
                    text: index
                        .document_text(document.id)
                        .map_err(|err| err.to_string())?,
                    fields: document.fields,
                }),
                None => None,
            });
        }
        Ok(known)
    })
}

/// The rule of the reorganisation: a copy that lies in an unordered folder
/// while the same content has a place in the order can go.
fn duplicates_of(
    root: &Path,
    files: &[PlanFile],
    structure: &LocationStructure,
    kinds: &Kinds,
) -> Vec<TrashItem> {
    let documents: Vec<ArchiveDocument> = files
        .iter()
        .map(|file| ArchiveDocument {
            relative_path: file.relative_path.clone(),
            date: file.fields.date,
        })
        .collect();
    let profile = learn_archive(&documents);
    duplicate_copies(files, &profile, structure, kinds)
        .into_iter()
        .map(|copy| TrashItem {
            path: root.join(copy.source),
            expected_hash: copy.hash,
            keeper: root.join(copy.keeper),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vanished_subfolder_is_skipped_but_a_vanished_root_ends_the_read() {
        let gone = io::Error::from(io::ErrorKind::NotFound);
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        let folder = Path::new("/archive/sub");

        assert_eq!(walk_error(folder, Path::new("sub"), &gone), Ok(()));
        assert_eq!(
            walk_error(Path::new("/archive"), Path::new(""), &gone),
            Err(ReadError::Gone)
        );
        assert!(matches!(
            walk_error(folder, Path::new("sub"), &denied),
            Err(ReadError::Io(_))
        ));
    }

    #[test]
    fn in_threads_keeps_the_order_for_any_thread_count() {
        let items: Vec<usize> = (0..50).collect();
        for threads in [1, 2, 7] {
            let doubled = in_threads(&items, threads, ThreadPriority::Utility, |item| {
                item.saturating_mul(2)
            });
            let expected: Vec<usize> = items.iter().map(|item| item.saturating_mul(2)).collect();
            assert_eq!(doubled, expected, "{threads} threads");
        }
    }
}
