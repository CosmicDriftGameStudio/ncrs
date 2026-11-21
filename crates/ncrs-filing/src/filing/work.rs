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
    /// The interrupted runs of an index that was held back until the user
    /// accepted it.
    Settle,
    /// `data_dir` is the folder the module keeps its files in; only an
    /// `index.db` directly in it is removed.
    StartOver {
        data_dir: PathBuf,
        /// A plain index has no recovery key to bring it back, so it is
        /// removed instead of set aside.
        plain: bool,
    },
}

/// One blocking step. Synchronous; the app runs it on a blocking thread.
pub struct Work {
    generation: u64,
    db_path: PathBuf,
    store: SharedStore,
    index: IndexSlot,
    settle_problem: SettleNote,
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
        let slot = &self.index;
        let note = &self.settle_problem;
        let outcome = match self.job {
            Job::Start { locations } => {
                let opened = open_existing(db_path, store, slot, note);
                let damage = lock(slot)
                    .as_ref()
                    .and_then(|index| index.damage().unwrap_or_else(|err| Some(err.to_string())));
                WorkOutcome::Started {
                    locations: resolve_locations(&locations),
                    opened,
                    damage,
                }
            }
            Job::Create { encrypted } => {
                WorkOutcome::Created(create_index(db_path, store, slot, note, encrypted))
            }
            Job::Restore { key } => {
                WorkOutcome::KeyApplied(apply_key(db_path, store, slot, note, |path, target| {
                    Index::restore_key(path, &key, target)
                }))
            }
            Job::Replace { key } => {
                WorkOutcome::KeyApplied(apply_key(db_path, store, slot, note, |path, target| {
                    Index::replace_key(path, &key, target)
                }))
            }
            Job::Encrypt => WorkOutcome::Encrypted(encrypt_index(db_path, store, slot, note)),
            Job::Settle => {
                settle_held(slot, note);
                WorkOutcome::Settled
            }
            Job::StartOver { data_dir, plain } => {
                WorkOutcome::StartedOver(start_over(db_path, &data_dir, plain, store, slot, note))
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
            settle_problem: Arc::clone(&self.settle_problem),
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
            Phase::FromNewerVersion | Phase::Failed(_) => return Err(HeldReason::Failed),
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
