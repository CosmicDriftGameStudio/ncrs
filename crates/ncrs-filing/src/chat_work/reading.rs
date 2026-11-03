//! Reading a folder for the chat: what is in it, which order it follows and
//! which files are duplicates that can go. Writes nothing.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Local};
use ncrs_index::{
    ContentHash, DocumentFields, Finding, Kinds, NoActionReason, PlanOutcome, SenderAliases,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    NotADirectory,
    TooMany { limit: usize },
    Io(String),
}

fn walk(base: &Path, relative: &Path, found: &mut Vec<PathBuf>) -> Result<(), ReadError> {
    let folder = base.join(relative);
    let entries = fs::read_dir(&folder)
        .map_err(|err| ReadError::Io(format!("{}: {err}", folder.display())))?;
    for entry in entries {
        let dir_entry =
            entry.map_err(|err| ReadError::Io(format!("{}: {err}", folder.display())))?;
        let name = dir_entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(file_type) = dir_entry.file_type() else {
            continue;
        };
        let path = relative.join(&name);
        if file_type.is_dir() {
            walk(base, &path, found)?;
        } else if !file_type.is_symlink() {
            found.push(path);
            if found.len() > FILE_LIMIT {
                return Err(ReadError::TooMany { limit: FILE_LIMIT });
            }
        }
    }
    Ok(())
}

fn modified_date(path: &Path) -> Option<chrono::NaiveDate> {
    let modified: SystemTime = fs::metadata(path).ok()?.modified().ok()?;
    Some(DateTime::<Local>::from(modified).date_naive())
}

struct Known {
    fields: DocumentFields,
    text: Option<String>,
}

pub fn read_folder(access: &ChatAccess, folder: &Path) -> Result<FolderReading, ReadError> {
    match fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(ReadError::NotADirectory),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Err(ReadError::NotADirectory),
        Err(err) => return Err(ReadError::Io(format!("{}: {err}", folder.display()))),
    }
    let mut relatives = Vec::new();
    walk(folder, Path::new(""), &mut relatives)?;
    relatives.sort();

    // A file that cannot be hashed cannot be read or filed either, so it is not part of the picture.
    let hashed: Vec<(PathBuf, ContentHash)> = relatives
        .into_iter()
        .filter_map(|relative| {
            let hash = hash_path(&folder.join(&relative)).ok()?;
            Some((relative, hash))
        })
        .collect();

    let (known, kinds) = look_up(access, &hashed).map_err(ReadError::Io)?;
    let names = &access.own.names;
    let own = OwnNames {
        names,
        ibans: &access.own.ibans,
        tax_ids: &access.own.tax_ids,
    };
    let aliases = SenderAliases::default();

    let mut files = Vec::with_capacity(hashed.len());
    let mut photos = 0;
    let mut without_text = 0;
    for (position, ((relative, hash), stored)) in (0..).zip(hashed.into_iter().zip(known)) {
        let absolute = folder.join(&relative);
        let (fields, text) = match stored {
            Some(record) => (record.fields, record.text),
            None => {
                let extracted = access.text_chain.extract(&absolute).ok().flatten();
                let detected = extracted.as_deref().map(|body| {
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
                        .or_else(|| modified_date(&absolute)),
                    sender: detected.as_ref().and_then(|fields| fields.sender.clone()),
                    kind: detected.and_then(|fields| fields.kind),
                    ..DocumentFields::default()
                };
                (fields, extracted)
            }
        };
        if text.is_none() {
            if is_image_file(&relative) {
                photos += 1;
            } else {
                without_text += 1;
            }
        }
        files.push(PlanFile {
            relative_path: relative,
            hash,
            fields,
            text,
            recorded_at: position,
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
    };
    reading.patterns = adoptable_patterns(&kinds, &reading);
    Ok(reading)
}

fn look_up(
    access: &ChatAccess,
    hashed: &[(PathBuf, ContentHash)],
) -> Result<(Vec<Option<Known>>, Kinds), String> {
    access.with_index(|index| {
        let kinds = Kinds::new(
            std::sync::Arc::clone(index.kinds()),
            access.own.kind_language,
        );
        let mut known = Vec::with_capacity(hashed.len());
        for (_, hash) in hashed {
            let by_hash = index
                .document_by_hash(hash)
                .map_err(|err| err.to_string())?;
            known.push(match by_hash {
                Some(document) => Some(Known {
                    text: index
                        .document_text(document.id)
                        .map_err(|err| err.to_string())?,
                    fields: document.fields,
                }),
                None => None,
            });
        }
        Ok((known, kinds))
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
