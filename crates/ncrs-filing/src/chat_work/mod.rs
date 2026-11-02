//! The work behind the Hermes chat. It blocks, so the chat runs it on a
//! blocking thread, never in `update`.
//!
//! Reading and the trial touch nothing on disk and write nothing to the
//! index. Only `File` and `TrashDuplicates` change files, and both go
//! through the journal, so `Undo` can put them back.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_core::INDEX_NOT_OPEN;
use ncrs_fs::Trash;
use ncrs_index::{BatchId, Index, KindLanguage, Kinds};
use ncrs_intake::TextChain;
use ncrs_pipeline::OwnDetails;

mod change;
mod reading;
mod runs;
mod trial;

#[cfg(test)]
mod tests;

pub use change::{ChangePreview, OrderChange, Scope};
pub use reading::{FolderReading, ReadError, FILE_LIMIT};
pub use runs::{RunItem, RunPlan, RunReport, RunSummary};
pub use trial::{StayReason, TrialEntry, TrialOutcome, TrialPlan};

pub use crate::execute::{TrashItem, TrashReport, UndoReport};
pub use ncrs_intent::TimeUnit;

type IndexSlot = Arc<Mutex<Option<Index>>>;

/// One piece of work for the chat.
#[derive(Debug, Clone)]
pub enum ChatWork {
    Read {
        folder: std::path::PathBuf,
    },
    Trial {
        source: Arc<FolderReading>,
        target: Arc<FolderReading>,
        changes: Vec<OrderChange>,
    },
    PreviewChange {
        reading: Arc<FolderReading>,
        kind_word: String,
        unit: TimeUnit,
    },
    File {
        run: RunPlan,
    },
    TrashDuplicates {
        name: String,
        root: std::path::PathBuf,
        items: Vec<TrashItem>,
    },
    LastRun,
    Undo {
        batch: BatchId,
    },
}

#[derive(Debug, Clone)]
pub enum ChatResult {
    Read(Result<Arc<FolderReading>, ReadError>),
    Trial(Result<TrialPlan, String>),
    Preview(Option<ChangePreview>),
    Filed(Result<RunReport, String>),
    Trashed(Result<TrashReport, String>),
    LastRun(Result<Option<RunSummary>, String>),
    Undone(Result<UndoReport, String>),
}

/// The handle of the chat: the index the pipeline shares, the text sources to
/// read files it does not know yet, and the trash for duplicates.
#[derive(Clone)]
pub struct ChatAccess {
    slot: IndexSlot,
    text_chain: Arc<TextChain>,
    trash: Arc<dyn Trash>,
    own: OwnDetails,
}

impl fmt::Debug for ChatAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatAccess").finish_non_exhaustive()
    }
}

impl ChatAccess {
    pub fn new(slot: IndexSlot, text_chain: Arc<TextChain>, trash: Arc<dyn Trash>) -> Self {
        Self {
            slot,
            text_chain,
            trash,
            own: OwnDetails::default(),
        }
    }

    #[must_use]
    pub fn with_own(mut self, own: OwnDetails) -> Self {
        self.own = own;
        self
    }

    #[must_use]
    pub fn with_kind_language(mut self, kind_language: KindLanguage) -> Self {
        self.own.kind_language = kind_language;
        self
    }

    /// Whether what ncrs trashes can be listed and put back; without that the
    /// chat does not offer to trash anything.
    pub fn trash_can_restore(&self) -> bool {
        crate::execute::trash_can_restore(&*self.trash)
    }

    pub fn run(&self, work: ChatWork) -> ChatResult {
        match work {
            ChatWork::Read { folder } => {
                ChatResult::Read(reading::read_folder(self, &folder).map(Arc::new))
            }
            ChatWork::Trial {
                source,
                target,
                changes,
            } => ChatResult::Trial(trial::trial(self, &source, &target, &changes)),
            ChatWork::PreviewChange {
                reading,
                kind_word,
                unit,
            } => ChatResult::Preview(
                self.kinds()
                    .ok()
                    .and_then(|kinds| change::preview(&kinds, &reading, &kind_word, unit)),
            ),
            ChatWork::File { run } => ChatResult::Filed(runs::file(self, &run)),
            ChatWork::TrashDuplicates { name, root, items } => {
                ChatResult::Trashed(runs::trash_duplicates(self, &name, &root, &items))
            }
            ChatWork::LastRun => ChatResult::LastRun(runs::last_run(self)),
            ChatWork::Undo { batch } => ChatResult::Undone(runs::undo(self, batch)),
        }
    }

    fn with_index<T>(&self, work: impl FnOnce(&Index) -> Result<T, String>) -> Result<T, String> {
        let guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_ref().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        work(index)
    }

    fn kinds(&self) -> Result<Kinds, String> {
        self.with_index(|index| {
            Ok(Kinds::new(
                Arc::clone(index.kinds()),
                self.own.kind_language,
            ))
        })
    }
}
