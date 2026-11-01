//! The blocking steps of the filing module and the handle the pipeline gets.

use super::*;

#[derive(Debug)]
pub(in crate::filing) enum Job {
    Start {
        locations: Vec<FilingLocation>,
    },
    Create {
        encrypted: bool,
    },
    Restore {
        key: RecoveryKey,
    },
    Replace {
        key: RecoveryKey,
    },
    Encrypt,
    /// `data_dir` is the folder the module keeps its files in; only an
    /// `index.db` directly in it is removed.
    StartOver {
        data_dir: PathBuf,
    },
}

/// One blocking step. Synchronous; the app runs it on a blocking thread.
pub struct Work {
    generation: u64,
    db_path: PathBuf,
    store: SharedStore,
    index: IndexSlot,
    job: Job,
}

impl fmt::Debug for Work {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Work")
            .field("generation", &self.generation)
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}

impl Work {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn run(self) -> FilingMessage {
        let db_path = self.db_path.as_path();
        let store = self.store.as_ref();
        let outcome = match self.job {
            Job::Start { locations } => WorkOutcome::Started {
                locations: resolve_locations(&locations),
                opened: open_existing(db_path, store, &self.index),
            },
            Job::Create { encrypted } => {
                WorkOutcome::Created(create_index(db_path, store, &self.index, encrypted))
            }
            Job::Restore { key } => {
                WorkOutcome::KeyApplied(apply_key(db_path, store, &self.index, |path, target| {
                    Index::restore_key(path, &key, target)
                }))
            }
            Job::Replace { key } => {
                WorkOutcome::KeyApplied(apply_key(db_path, store, &self.index, |path, target| {
                    Index::replace_key(path, &key, target)
                }))
            }
            Job::Encrypt => WorkOutcome::Encrypted(encrypt_index(db_path, store, &self.index)),
            Job::StartOver { data_dir } => {
                WorkOutcome::StartedOver(start_over(db_path, &data_dir, store, &self.index))
            }
        };
        FilingMessage::WorkDone {
            generation: self.generation,
            outcome,
        }
    }
}

pub(in crate::filing) fn lock(slot: &IndexSlot) -> MutexGuard<'_, Option<Index>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the runner works through. Private to this module so that nothing else
/// can build one around the slot.
pub(in crate::filing) struct IntakeIndex(IndexSlot);

impl fmt::Debug for IntakeIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IntakeIndex")
    }
}

impl IndexAccess for IntakeIndex {
    fn with_index(&self, work: &mut dyn FnMut(&mut Index)) -> bool {
        match lock(&self.0).as_mut() {
            Some(index) => {
                work(index);
                true
            }
            None => false,
        }
    }
}

impl FilingModule {
    pub(in crate::filing) fn new_work(&mut self, job: Job) -> Work {
        self.generation = next_generation();
        Work {
            generation: self.generation,
            db_path: self.db_path.clone(),
            store: Arc::clone(&self.store),
            index: Arc::clone(&self.index),
            job,
        }
    }

    pub(in crate::filing) fn intake_index(&self) -> Result<IntakeIndex, HeldReason> {
        match &self.phase {
            Phase::Ready => {}
            Phase::Starting => return Err(HeldReason::Starting),
            Phase::AskEncryption { .. }
            | Phase::ShowRecoveryKey { .. }
            | Phase::AskRecoveryKey { .. } => return Err(HeldReason::WaitingForUser),
            Phase::Encrypting | Phase::StartingOver => return Err(HeldReason::Starting),
            Phase::PlainFileWarning => return Err(HeldReason::PlainFileUnconfirmed),
            Phase::Dismissed => return Err(HeldReason::Dismissed),
            Phase::Failed(_) => return Err(HeldReason::Failed),
        }
        // Never waits: a job on the blocking pool may hold the index, and a
        // held index is an open one.
        let is_open = match self.index.try_lock() {
            Ok(guard) => guard.is_some(),
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner().is_some(),
            Err(TryLockError::WouldBlock) => true,
        };
        if !is_open {
            return Err(HeldReason::Failed);
        }
        Ok(IntakeIndex(Arc::clone(&self.index)))
    }
}
