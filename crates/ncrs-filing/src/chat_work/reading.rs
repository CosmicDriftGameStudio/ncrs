//! Reading a folder for the chat: what is in it, which order it follows and
//! which files are duplicates that can go. Writes nothing.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use ncrs_index::{ContentHash, Document, DocumentFields, Kinds, SenderAliases};
use ncrs_intake::{
    date_in_file_stem, detect_fields, duplicate_copies, hash_path, is_image_file, learn_archive,
    location_structure, ArchiveDocument, LocationStructure, OwnNames, PlanFile, TextError,
};

use super::{ChatAccess, TrashItem};
use crate::reading_support::{
    in_parallel, is_stopped, read_error, walk, walk_sorted, ReadError, ReadProgress, Walked,
};

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

/// The files `read_folder` walks, relative to the folder and in its order;
/// files that cannot be hashed are still among them.
pub fn list_files(folder: &Path) -> Result<Vec<PathBuf>, ReadError> {
    let walked = walk_sorted(folder, &[], &ReadProgress::default())?;
    Ok(walked.into_iter().map(|file| file.relative).collect())
}

/// A set `stop` skips the hashing and reading of the files not yet started, so
/// the reading then lacks them and must not be used after stopping.
pub fn read_folder(
    access: &ChatAccess,
    folder: &Path,
    progress: &ReadProgress,
    stop: Option<&AtomicBool>,
) -> Result<FolderReading, ReadError> {
    read_folder_only(access, folder, progress, stop, None, UnknownFiles::Extract)
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
    let mut walked = walk_sorted(folder, &[], progress)?;
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
    walk(
        folder,
        Path::new(""),
        &[],
        &mut walked,
        &ReadProgress::default(),
    )?;
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
