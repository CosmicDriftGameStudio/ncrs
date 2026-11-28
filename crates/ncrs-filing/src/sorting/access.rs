use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_config::config::{SourceSetting, TargetSetting};
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_index::{BatchId, Index, Kinds, RuleId, TargetId};
use ncrs_intake::{canonical_path, PlainText, SortTarget, TextChain};
use ncrs_pipeline::{Load, OwnDetails};

use super::ready::{all_gone_below, state_of, TargetState, WaitReason};
use crate::chat_work::{runs_of_index, undo_run, RecentRun, Snapshots, UndoReport};
use crate::execute::{
    trash_can_restore, trash_own_items, trash_run, OwnTrashItem, OwnTrashRun, TrashItem,
    TrashReport,
};
use crate::index_work::IndexWork;

/// What sorting works with; one copy per job.
#[derive(Clone)]
pub struct SortingAccess {
    pub slot: Arc<Mutex<Option<Index>>>,
    pub sources: Vec<SourceSetting>,
    pub targets: Vec<TargetSetting>,
    pub text_chain: Arc<TextChain>,
    pub own: OwnDetails,
    pub load: Load,
    pub trash: Arc<dyn ncrs_fs::Trash>,
    pub snapshots: Snapshots,
    pub index_work: IndexWork,
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

    /// The configured targets in their order (the first one is T1), their paths
    /// spelled the way a plan spells them.
    pub fn configured_targets(&self) -> Vec<SortTarget> {
        self.targets
            .iter()
            .map(|entry| resolved(&entry.target))
            .collect()
    }

    /// What a rule is called in a plan: its name, or its pattern without one.
    pub fn rule_names(&self) -> Result<HashMap<RuleId, String>, String> {
        let rules = self.with_index(|index| index.rules().map_err(text))?;
        Ok(rules
            .into_iter()
            .map(|rule| {
                let name = rule.spec.name.unwrap_or(rule.spec.filing_pattern);
                (rule.id, name)
            })
            .collect())
    }

    /// Duplicates only go where they can be taken back from.
    pub fn trash_can_restore(&self) -> bool {
        trash_can_restore(self.trash.as_ref())
    }

    /// Moves own items below `root` to the trash as one run of the history.
    pub fn trash_own(
        &self,
        name: &str,
        root: &Path,
        items: &[OwnTrashItem],
        empty_after: bool,
    ) -> OwnTrashRun {
        let _work = self.index_work.begin();
        let journaled = self.with_index(|index| {
            Ok(trash_own_items(
                Some(index),
                name,
                root,
                items,
                self.trash.as_ref(),
                empty_after,
            ))
        });
        journaled.unwrap_or_else(|_index_not_open| {
            trash_own_items(None, name, root, items, self.trash.as_ref(), empty_after)
        })
    }

    /// Moves duplicates below `root` to the trash as one run of the history.
    pub fn trash_duplicates(
        &self,
        name: &str,
        root: &Path,
        items: &[TrashItem],
    ) -> Result<TrashReport, String> {
        let _work = self.index_work.begin();
        self.with_index_mut(|index| {
            trash_run(index, name, root, items, self.trash.as_ref()).map_err(text)
        })
    }

    /// Reads text files only, so a test of another crate needs no engine.
    #[doc(hidden)]
    pub fn for_tests(
        index: Index,
        sources: Vec<SourceSetting>,
        targets: Vec<TargetSetting>,
        trash: Arc<dyn ncrs_fs::Trash>,
    ) -> Self {
        Self {
            slot: Arc::new(Mutex::new(Some(index))),
            sources,
            targets,
            text_chain: Arc::new(TextChain::new().with_source(PlainText)),
            own: OwnDetails::default(),
            load: Load::default(),
            trash,
            snapshots: Snapshots::default(),
            index_work: IndexWork::default(),
        }
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

    /// How many documents the index knows below `folder`, those it misses
    /// included, while none of them is there; 0 when one is. Blocking.
    pub fn all_gone_below(&self, folder: &Path) -> Result<usize, String> {
        all_gone_below(self, folder)
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
pub fn text<E: fmt::Display>(err: E) -> String {
    err.to_string()
}
