//! The work behind the Hermes chat. It blocks, so the chat runs it on a
//! blocking thread, never in `update`.
//!
//! Reading touches nothing on disk and writes nothing to the index; `Undo`
//! puts files back through the journal.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_core::INDEX_NOT_OPEN;
use ncrs_fs::Trash;
use ncrs_index::{BatchId, Index, KindLanguage, Kinds};
use ncrs_intake::TextChain;
use ncrs_pipeline::{Load, OwnDetails};

use crate::index_slot::try_lock;

mod branches;
mod reading;
mod runs;
mod snapshots;
mod words;

#[cfg(test)]
mod tests;

pub use crate::execute::UndoReport;
pub use crate::reading_support::{ReadError, ReadProgress, FILE_LIMIT};
pub use branches::{branch_example, segments};
pub use reading::{AiCoverage, FolderReading};
pub use runs::{remove_emptied_folders, runs_of_index, undo_run};
pub use runs::{RecentRun, RunSummary};
pub use snapshots::Snapshots;
pub use words::same_word;

type IndexSlot = Arc<Mutex<Option<Index>>>;

/// Never shown: the UI treats it like "no answer now" and tries again later.
const INDEX_BUSY: &str = "the index is busy with a run";

/// One piece of work for the chat.
#[derive(Debug, Clone)]
pub enum ChatWork {
    Read {
        folder: PathBuf,
        progress: Arc<ReadProgress>,
    },
    LastRun,
    Undo {
        batch: BatchId,
    },
}

#[derive(Debug, Clone)]
pub enum ChatResult {
    Read(Result<Arc<FolderReading>, ReadError>),
    LastRun(Result<Option<RunSummary>, String>),
    Undone(Result<UndoReport, String>),
}

/// The handle of the chat: the index the pipeline shares, the text sources to
/// read files it does not know yet, and the trash files are put back from.
#[derive(Clone)]
pub struct ChatAccess {
    slot: IndexSlot,
    text_chain: Arc<TextChain>,
    trash: Arc<dyn Trash>,
    own: OwnDetails,
    load: Load,
    index_work: crate::index_work::IndexWork,
    snapshots: Snapshots,
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
            load: Load::default(),
            index_work: crate::index_work::IndexWork::default(),
            snapshots: Snapshots::default(),
        }
    }

    #[must_use]
    pub fn with_index_work(mut self, index_work: crate::index_work::IndexWork) -> Self {
        self.index_work = index_work;
        self
    }

    /// How many threads and at which priority a folder is read.
    #[must_use]
    pub fn with_load(mut self, load: Load) -> Self {
        self.load = load;
        self
    }

    #[must_use]
    pub(crate) fn with_snapshots(mut self, snapshots: Snapshots) -> Self {
        self.snapshots = snapshots;
        self
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

    pub fn kind_language(&self) -> KindLanguage {
        self.own.kind_language
    }

    /// How many documents the index holds below `folder`.
    pub fn documents_under(&self, folder: &Path) -> Result<usize, String> {
        self.with_index(|index| {
            index
                .documents_under(folder)
                .map(|found| found.len())
                .map_err(|err| err.to_string())
        })
    }

    /// How many documents below `folder` have a text, and how many of them
    /// the AI has read and sorted.
    pub fn ai_coverage_under(&self, folder: &Path) -> Result<(usize, usize), String> {
        self.with_index(|index| {
            index
                .ai_coverage_beneath(folder)
                .map_err(|err| err.to_string())
        })
    }

    pub fn run(&self, work: ChatWork) -> ChatResult {
        let _running = self.index_work.begin();
        match work {
            ChatWork::Read { folder, progress } => {
                ChatResult::Read(reading::read_folder(self, &folder, &progress).map(Arc::new))
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

    /// For the UI thread: while a run holds the index the kinds last read
    /// are returned; `Err` at once only when there are none yet.
    pub fn try_kinds(&self) -> Result<Kinds, String> {
        match self.try_with_index(|index| Ok(self.kinds_of(index))) {
            Ok(kinds) => {
                self.snapshots.remember_kinds(&kinds);
                Ok(kinds)
            }
            Err(err) if err == INDEX_BUSY => self.snapshots.kinds().ok_or(err),
            Err(err) => Err(err),
        }
    }

    fn kinds_of(&self, index: &Index) -> Kinds {
        Kinds::new(Arc::clone(index.kinds()), self.own.kind_language)
    }

    fn try_with_index<T>(
        &self,
        work: impl FnOnce(&Index) -> Result<T, String>,
    ) -> Result<T, String> {
        let guard = try_lock(&self.slot).ok_or_else(|| INDEX_BUSY.to_owned())?;
        let index = guard.as_ref().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        work(index)
    }
}
