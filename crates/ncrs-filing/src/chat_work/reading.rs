//! Reading a folder for the chat: what is in it, which order it follows and
//! which files are duplicates that can go. Writes nothing.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use chrono::{DateTime, Local};
use ncrs_index::{
    ContentHash, Document, DocumentFields, FileStamp, Finding, Kinds, NoActionReason, PlanOutcome,
    SenderAliases,
};
use ncrs_intake::{
    detect_fields, hash_path, is_image_file, learn_archive, location_structure,
    plan_reorganisation, ArchiveDocument, LocationStructure, OwnNames, PlanFile,
    ReorganisationInput,
};

use super::pattern::{adoptable_patterns, PatternChoice};
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
    /// The patterns below the areas that moving to them would change files.
    pub patterns: Vec<PatternChoice>,
    /// How far the AI got with the documents of the folder the index knows.
    pub ai: AiCoverage,
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

fn walk(
    base: &Path,
    relative: &Path,
    found: &mut Vec<Walked>,
    progress: &ReadProgress,
) -> Result<(), ReadError> {
    let folder = base.join(relative);
    let entries = fs::read_dir(&folder).map_err(|err| read_error(&folder, &err))?;
    for entry in entries {
        let dir_entry = entry.map_err(|err| read_error(&folder, &err))?;
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
        } else if !file_type.is_symlink() {
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

fn modified_date(mtime: SystemTime) -> chrono::NaiveDate {
    DateTime::<Local>::from(mtime).date_naive()
}

/// What the index knows of a file: its hash, fields and full text.
struct Known {
    hash: ContentHash,
    fields: DocumentFields,
    text: Option<String>,
}

/// `work` on every item, spread over a few threads; the results keep the order.
fn in_parallel<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(8);
    let chunk = items.len().div_ceil(threads).max(1);
    let work = &work;
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|part| scope.spawn(move || part.iter().map(work).collect::<Vec<R>>()))
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

pub fn read_folder(
    access: &ChatAccess,
    folder: &Path,
    progress: &ReadProgress,
) -> Result<FolderReading, ReadError> {
    match fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(ReadError::NotADirectory),
        Err(err) => return Err(read_error(folder, &err)),
    }
    let mut walked = Vec::new();
    walk(folder, Path::new(""), &mut walked, progress)?;
    walked.sort_by(|left, right| left.relative.cmp(&right.relative));
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
    let hashes = in_parallel(&changed, |position| {
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
        .zip(hashes)
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
    let extracted = in_parallel(&unknown, |(position, _)| {
        let text = walked.get(*position).and_then(|file| {
            access
                .text_chain
                .extract(&folder.join(&file.relative))
                .ok()
                .flatten()
        });
        progress.done.fetch_add(1, Ordering::Relaxed);
        text
    });
    for ((position, hash), text) in unknown.into_iter().zip(extracted) {
        let Some(file) = walked.get(position) else {
            continue;
        };
        let detected = text.as_deref().map(|body| {
            detect_fields(
                body,
                &DocumentFields::default(),
                &aliases,
                &[],
                &own,
                &kinds.catalog,
            )
        });
        let fields = DocumentFields {
            date: detected
                .as_ref()
                .and_then(|fields| fields.date)
                .or_else(|| Some(modified_date(file.stamp.mtime))),
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
    for (file, record) in walked.into_iter().zip(known) {
        let Some(record) = record else {
            continue;
        };
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
    let mut reading = FolderReading {
        root: folder.to_path_buf(),
        files,
        photos,
        without_text,
        structure,
        duplicates,
        hashes,
        patterns: Vec::new(),
        ai: ai_coverage(access, folder),
    };
    reading.patterns = adoptable_patterns(&kinds, &reading);
    Ok(reading)
}

fn ai_coverage(access: &ChatAccess, folder: &Path) -> AiCoverage {
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
        .unwrap_or_default()
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
    let existing: BTreeSet<PathBuf> = files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect();
    plan_reorganisation(&ReorganisationInput {
        files,
        profile: &profile,
        unordered: structure,
        rules: &[],
        sender_aliases: &SenderAliases::default(),
        existing: &existing,
        kinds,
    })
    .into_iter()
    .filter(|item| {
        matches!(
            item.outcome,
            PlanOutcome::NoAction(NoActionReason::DuplicateCopy)
        )
    })
    .filter_map(|item| {
        let keeper = item.findings.iter().find_map(|finding| match finding {
            Finding::Duplicate { keeper } => Some(keeper),
            _ => None,
        })?;
        Some(TrashItem {
            path: root.join(&item.source),
            expected_hash: item.hash,
            keeper: root.join(keeper),
        })
    })
    .collect()
}
