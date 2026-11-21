//! Opening, creating, encrypting and replacing the index on disk.

use super::*;

#[cfg(test)]
pub(in crate::filing) fn put_index(slot: &IndexSlot, index: Index) {
    put_index_noting(slot, index, &SettleNote::default());
}

/// Every index goes through here before the watcher and the pipeline see it,
/// so a move that was interrupted is settled before anything looks at the files.
pub(in crate::filing) fn put_index_noting(slot: &IndexSlot, index: Index, note: &SettleNote) {
    *lock(slot) = Some(settled(index, note));
}

pub(in crate::filing) fn settled(index: Index, note: &SettleNote) -> Index {
    note_settling(&index, note);
    index
}

/// A failure leaves the pending items pending; the next open settles them.
fn note_settling(index: &Index, note: &SettleNote) {
    let problem = crate::execute::reconcile(index)
        .err()
        .map(|err| err.to_string());
    *note.lock().unwrap_or_else(PoisonError::into_inner) = problem;
}

/// Settles the index that was held back until the user accepted it.
pub(in crate::filing) fn settle_held(slot: &IndexSlot, note: &SettleNote) {
    if let Some(index) = lock(slot).as_ref() {
        note_settling(index, note);
    }
}

/// Never creates the file or its directory: the user has not chosen yet.
pub(in crate::filing) fn open_existing(
    db_path: &Path,
    store: &dyn KeyStore,
    slot: &IndexSlot,
    note: &SettleNote,
) -> OpenOutcome {
    open_into(&mut lock(slot), db_path, store, note)
}

/// For a job that keeps the slot locked from closing the index to opening it
/// again, so no reader sees it half way.
pub(in crate::filing) fn open_into(
    held: &mut Option<Index>,
    db_path: &Path,
    store: &dyn KeyStore,
    note: &SettleNote,
) -> OpenOutcome {
    match Index::open(db_path, store) {
        Err(IndexError::NotFound) => OpenOutcome::NoIndexYet,
        Err(IndexError::KeyMissing) => OpenOutcome::KeyMissing,
        Err(IndexError::WrongKey | IndexError::InvalidKeyLength { .. }) => OpenOutcome::KeyMismatch,
        Err(IndexError::SchemaTooNew { .. }) => OpenOutcome::FromNewerVersion,
        Err(other) => OpenOutcome::Failed(other.to_string()),
        Ok(index) => {
            let outcome = if index.is_encrypted() {
                OpenOutcome::Opened
            } else {
                // A plain index must not be blocked where no credential store
                // exists (Linux without Secret Service): only a stored key
                // makes an unencrypted file suspicious.
                match store.load() {
                    Ok(Some(_)) => OpenOutcome::PlainFileWithKeyEntry,
                    Ok(None) => OpenOutcome::Opened,
                    Err(_) => OpenOutcome::OpenedWithoutKeyStore,
                }
            };
            // The journal of a file nobody has accepted yet is not acted on:
            // it is settled when the user accepts it.
            *held = Some(if outcome == OpenOutcome::PlainFileWithKeyEntry {
                index
            } else {
                settled(index, note)
            });
            outcome
        }
    }
}

/// After a change that did not go through: the index is back in the state it
/// was in, so a key entry beside a plain file is no reason to ask again.
pub(in crate::filing) fn reopen_after_failure(
    held: &mut Option<Index>,
    db_path: &Path,
    store: &dyn KeyStore,
    note: &SettleNote,
) -> OpenOutcome {
    match open_into(held, db_path, store, note) {
        OpenOutcome::PlainFileWithKeyEntry => {
            if let Some(index) = held.as_ref() {
                note_settling(index, note);
            }
            OpenOutcome::Opened
        }
        other => other,
    }
}

/// Closes the index, encrypts the file and opens it again, all under the lock
/// of the slot. The caller has stopped the pipeline.
pub(in crate::filing) fn encrypt_index(
    db_path: &Path,
    store: &dyn KeyStore,
    slot: &IndexSlot,
    note: &SettleNote,
) -> EncryptReport {
    let mut held = lock(slot);
    drop(held.take());
    let encrypted = match mark_unconfirmed(db_path) {
        Ok(()) => Index::encrypt_plain(db_path, store).map_err(|err| err.to_string()),
        Err(err) => Err(err.to_string()),
    };
    match encrypted {
        Ok(recovery_key) => EncryptReport {
            // The copies from before a migration are plain files, and so is
            // an index set aside earlier when it was not encrypted; an
            // encrypted one stays, the recovery key brings it back.
            result: {
                ncrs_index::migration_backups(db_path)
                    .into_iter()
                    .chain(plain_set_aside_files(db_path))
                    .for_each(|copy| drop(remove_if_present(&copy)));
                Ok(ShownRecoveryKey::new(recovery_key))
            },
            opened: open_into(&mut held, db_path, store, note),
        },
        Err(reason) => EncryptReport {
            result: {
                drop(clear_unconfirmed(db_path));
                Err(reason)
            },
            opened: reopen_after_failure(&mut held, db_path, store, note),
        },
    }
}

pub(in crate::filing) const INDEX_NOT_REOPENED: &str = "the index could not be opened again";
pub(in crate::filing) const INDEX_FILE_NAME: &str = "index.db";

/// The files of an index, the one the data folder holds and what SQLite and
/// the encryption leave beside it; the database goes last. Nothing else is
/// ever removed, and a path that is not `index.db` directly in `data_dir` is
/// refused.
pub(in crate::filing) fn index_files(
    db_path: &Path,
    data_dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    let named_right = db_path
        .file_name()
        .is_some_and(|name| name == INDEX_FILE_NAME);
    let parent_folder = db_path
        .parent()
        .and_then(|parent| parent.canonicalize().ok());
    let data_folder = data_dir.canonicalize().ok();
    let folder = match (named_right, parent_folder, data_folder) {
        (true, Some(parent), Some(data)) if parent == data => parent,
        _ => return Err("the index is not where the data folder keeps it".to_owned()),
    };
    Ok([".encrypting", "-journal", "-wal", "-shm", ""]
        .iter()
        .map(|suffix| folder.join(format!("{INDEX_FILE_NAME}{suffix}")))
        .collect())
}

pub(in crate::filing) fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// What SQLite keeps beside the index file; these move with it when the index
/// is set aside. Anything else `index_files` names is a leftover and goes.
const SET_ASIDE_SUFFIXES: [&str; 4] = ["-journal", "-wal", "-shm", ""];

/// Moves the index files to the set-aside names, the previous set-aside copy
/// going first. Nothing else is deleted when a move fails, except leftovers
/// of an unfinished encryption and the older set-aside copy.
fn set_index_aside(db_path: &Path, data_dir: &Path) -> Result<(), String> {
    let suffix_of = |file: &Path| {
        file.file_name()?
            .to_str()?
            .strip_prefix(INDEX_FILE_NAME)
            .map(str::to_owned)
    };
    let (kept, leftovers): (Vec<_>, Vec<_>) = index_files(db_path, data_dir)?
        .into_iter()
        .partition(|file| {
            suffix_of(file).is_some_and(|suffix| SET_ASIDE_SUFFIXES.contains(&suffix.as_str()))
        });
    if db_path.exists() {
        for suffix in SET_ASIDE_SUFFIXES {
            let older =
                ncrs_index::set_aside_path(db_path, suffix).map_err(|err| err.to_string())?;
            remove_if_present(&older).map_err(|err| err.to_string())?;
        }
    }
    for file in leftovers {
        remove_if_present(&file).map_err(|err| err.to_string())?;
    }
    for file in kept.iter().filter(|file| file.exists()) {
        let suffix = suffix_of(file).unwrap_or_default();
        let target = ncrs_index::set_aside_path(db_path, &suffix).map_err(|err| err.to_string())?;
        std::fs::rename(file, target).map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// The set-aside files, but only when the set-aside index is not encrypted: an
/// encrypted one is kept, the recovery key brings it back.
fn plain_set_aside_files(db_path: &Path) -> Vec<PathBuf> {
    let Ok(main) = ncrs_index::set_aside_path(db_path, "") else {
        return Vec::new();
    };
    if !Index::file_is_plain(&main).unwrap_or(false) {
        return Vec::new();
    }
    SET_ASIDE_SUFFIXES
        .iter()
        .filter_map(|suffix| ncrs_index::set_aside_path(db_path, suffix).ok())
        .collect()
}

/// A plain index is readable by anyone and has no recovery key, so it goes
/// with its migration copies and any older set-aside copy.
fn remove_index(db_path: &Path, data_dir: &Path) -> Result<(), String> {
    let mut files = index_files(db_path, data_dir)?;
    files.extend(ncrs_index::migration_backups(db_path));
    files.extend(plain_set_aside_files(db_path));
    files
        .iter()
        .try_for_each(|file| remove_if_present(file))
        .map_err(|err| err.to_string())
}

/// Closes the index, sets its files aside, so that the recovery key can bring
/// them back, and then removes its key. A failure on the files opens the index
/// again; one on the key only becomes a warning.
pub(in crate::filing) fn start_over(
    db_path: &Path,
    data_dir: &Path,
    plain: bool,
    store: &dyn KeyStore,
    slot: &IndexSlot,
    note: &SettleNote,
) -> StartOverReport {
    let mut held = lock(slot);
    drop(held.take());
    let removed = if plain {
        remove_index(db_path, data_dir)
    } else {
        set_index_aside(db_path, data_dir)
    };
    match removed {
        Ok(()) => StartOverReport {
            result: {
                drop(clear_unconfirmed(db_path));
                Ok(())
            },
            key_entry_warning: store.delete().err().map(|err| err.to_string()),
            opened: open_into(&mut held, db_path, store, note),
        },
        Err(reason) => StartOverReport {
            result: Err(reason),
            key_entry_warning: None,
            opened: reopen_after_failure(&mut held, db_path, store, note),
        },
    }
}

pub(in crate::filing) fn create_index(
    db_path: &Path,
    store: &dyn KeyStore,
    slot: &IndexSlot,
    note: &SettleNote,
    encrypted: bool,
) -> Result<Option<ShownRecoveryKey>, String> {
    if let Some(parent) = db_path.parent() {
        create_private_dir(parent).map_err(|err| err.to_string())?;
    }
    if encrypted {
        mark_unconfirmed(db_path).map_err(|err| err.to_string())?;
        let (index, recovery_key) = Index::create_encrypted(db_path, store).map_err(|err| {
            drop(clear_unconfirmed(db_path));
            err.to_string()
        })?;
        put_index_noting(slot, index, note);
        Ok(Some(ShownRecoveryKey::new(recovery_key)))
    } else {
        let index = Index::create_plain(db_path).map_err(|err| err.to_string())?;
        drop(clear_unconfirmed(db_path));
        put_index_noting(slot, index, note);
        Ok(None)
    }
}

pub(in crate::filing) fn apply_key(
    db_path: &Path,
    store: &dyn KeyStore,
    slot: &IndexSlot,
    note: &SettleNote,
    write_key: impl FnOnce(&Path, &dyn KeyStore) -> Result<(), IndexError>,
) -> Result<OpenOutcome, KeyError> {
    write_key(db_path, store).map_err(key_error)?;
    drop(clear_unconfirmed(db_path));
    let index = Index::open(db_path, store).map_err(key_error)?;
    put_index_noting(slot, index, note);
    Ok(OpenOutcome::Opened)
}

pub(in crate::filing) fn key_error(err: IndexError) -> KeyError {
    match err {
        IndexError::WrongKey => KeyError::DoesNotOpen,
        other => KeyError::Other(other.to_string()),
    }
}

/// The folder holds the extracted text of every document, so only the user
/// may enter it.
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700)
    };
    builder.create(path)
}

/// An index whose recovery key the user never confirmed shows it again. Where
/// the key cannot be read the index opens as usual: the marker stays, so the
/// next start tries again.
pub(in crate::filing) fn unconfirmed_key(
    opened: OpenOutcome,
    db_path: &Path,
    store: &dyn KeyStore,
) -> OpenOutcome {
    if opened != OpenOutcome::Opened || !is_unconfirmed(db_path) {
        return opened;
    }
    match Index::stored_recovery_key(store) {
        Ok(Some(key)) => OpenOutcome::OpenedKeyUnconfirmed(ShownRecoveryKey::new(key)),
        Ok(None) | Err(_) => opened,
    }
}
