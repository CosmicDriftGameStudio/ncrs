use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_config::config::{SourceSetting, TargetSetting};
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_index::{BatchId, Index, Kinds, TargetId};
use ncrs_intake::{canonical_path, SortTarget, TextChain};
use ncrs_pipeline::{Load, OwnDetails};

use super::ready::{state_of, TargetState, WaitReason};
use crate::chat_work::{runs_of_index, undo_run, RecentRun, Snapshots, UndoReport};
use crate::index_work::IndexWork;

/// What sorting works with; one copy per job.
#[derive(Clone)]
pub struct SortingAccess {
    pub(crate) slot: Arc<Mutex<Option<Index>>>,
    pub(crate) sources: Vec<SourceSetting>,
    pub(crate) targets: Vec<TargetSetting>,
    pub(crate) text_chain: Arc<TextChain>,
    pub(crate) own: OwnDetails,
    pub(crate) load: Load,
    pub(crate) trash: Arc<dyn ncrs_fs::Trash>,
    pub(crate) snapshots: Snapshots,
    pub(crate) index_work: IndexWork,
}

impl fmt::Debug for SortingAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SortingAccess")
            .field("sources", &self.sources)
            .field("targets", &self.targets)
            .finish_non_exhaustive()
    }
}

impl SortingAccess {
    /// Takes back all batches of the run `batch` belongs to.
    pub fn undo(&self, batch: BatchId) -> Result<UndoReport, String> {
        let _work = self.index_work.begin();
        self.with_index(|index| undo_run(index, self.trash.as_ref(), batch))
    }

    pub fn recent_runs(&self, limit: usize) -> Result<Vec<RecentRun>, String> {
        self.with_index(|index| runs_of_index(index, limit))
    }

    /// Whether the target can be filed into right now; asked again before
    /// every file.
    pub fn target_state(&self, id: &TargetId) -> TargetState {
        match self.setting(id) {
            None => TargetState::Waiting(WaitReason::Missing),
            Some(setting) if setting.paused.is_some() => TargetState::Waiting(WaitReason::Paused),
            Some(setting) => state_of(self, &setting.target),
        }
    }

    /// The targets out of `ids` that cannot be filed into.
    pub(super) fn waiting<'a>(
        &self,
        ids: impl IntoIterator<Item = &'a TargetId>,
    ) -> Vec<(TargetId, WaitReason)> {
        ids.into_iter()
            .filter_map(|id| match self.target_state(id) {
                TargetState::Ready => None,
                TargetState::Waiting(reason) => Some((id.clone(), reason)),
            })
            .collect()
    }

    fn setting(&self, id: &TargetId) -> Option<&TargetSetting> {
        self.targets.iter().find(|entry| entry.target.id == *id)
    }

    /// The target as configured, its path spelled the way the walk of a source
    /// spells its files.
    pub(super) fn target(&self, id: &TargetId) -> Option<SortTarget> {
        self.setting(id).map(|entry| resolved(&entry.target))
    }

    /// The folders of all configured targets, paused ones included.
    pub(super) fn target_folders(&self) -> Vec<PathBuf> {
        self.targets
            .iter()
            .map(|entry| resolved(&entry.target).path)
            .collect()
    }

    pub(super) fn is_in_target(&self, folder: &Path) -> bool {
        self.target_folders()
            .iter()
            .any(|target| folder.starts_with(target))
    }

    /// The targets strictly below `root`: what lies there is filed already.
    pub(super) fn targets_below(&self, root: &Path) -> Vec<PathBuf> {
        let mut folders = self.target_folders();
        folders.retain(|target| target != root && target.starts_with(root));
        folders
    }

    pub(super) fn kinds(&self, index: &Index) -> Kinds {
        let kinds = Kinds::new(Arc::clone(index.kinds()), self.own.kind_language);
        self.snapshots.remember_kinds(&kinds);
        kinds
    }

    pub(super) fn with_index<T>(
        &self,
        work: impl FnOnce(&Index) -> Result<T, String>,
    ) -> Result<T, String> {
        let guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_ref().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        work(index)
    }

    pub(super) fn with_index_mut<T>(
        &self,
        work: impl FnOnce(&mut Index) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        work(index)
    }
}

fn resolved(target: &SortTarget) -> SortTarget {
    SortTarget {
        path: canonical_path(&target.path).unwrap_or_else(|_| target.path.clone()),
        ..target.clone()
    }
}

/// For `map_err`.
pub(super) fn text<E: fmt::Display>(err: E) -> String {
    err.to_string()
}
