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
    match Index::encrypt_plain(db_path, store) {
        Ok(recovery_key) => EncryptReport {
            // The copies from before a migration are plain files.
            result: {
                ncrs_index::migration_backups(db_path)
                    .iter()
                    .for_each(|copy| drop(remove_if_present(copy)));
                Ok(ShownRecoveryKey::new(recovery_key))
            },
            opened: open_into(&mut held, db_path, store, note),
        },
        Err(err) => EncryptReport {
            result: Err(err.to_string()),
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

/// Closes the index, removes its files and then its key. A failure on the
/// files opens the index again; one on the key only becomes a warning.
pub(in crate::filing) fn start_over(
    db_path: &Path,
    data_dir: &Path,
    store: &dyn KeyStore,
    slot: &IndexSlot,
    note: &SettleNote,
) -> StartOverReport {
    let mut held = lock(slot);
    drop(held.take());
    let removed = index_files(db_path, data_dir).and_then(|files| {
        let copies = ncrs_index::migration_backups(db_path);
        files
            .iter()
            .chain(copies.iter())
            .try_for_each(|file| remove_if_present(file))
            .map_err(|err| err.to_string())
    });
    match removed {
        Ok(()) => StartOverReport {
            result: Ok(()),
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
        let (index, recovery_key) =
            Index::create_encrypted(db_path, store).map_err(|err| err.to_string())?;
        put_index_noting(slot, index, note);
        Ok(Some(ShownRecoveryKey::new(recovery_key)))
    } else {
        let index = Index::create_plain(db_path).map_err(|err| err.to_string())?;
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
