//! The work behind the Hermes chat. It blocks, so the chat runs it on a
//! blocking thread, never in `update`.
//!
//! Reading and the trial touch nothing on disk and write nothing to the
//! index. Only `File` and `TrashDuplicates` change files, and both go
//! through the journal, so `Undo` can put them back.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_ai::NameSpec;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_fs::Trash;
use ncrs_index::{BatchId, Index, KindLanguage, Kinds, Rule, RuleOrigin, SenderAliases};
use ncrs_intake::{rule_problem, TextChain};
use ncrs_pipeline::{Load, OwnDetails};

use crate::review_work::{AcceptSummary, ReviewAccess, ReviewItem};

mod change;
mod groups;
mod pattern;
mod reading;
mod runs;
mod trial;
mod waiting;
mod words;

#[cfg(test)]
mod tests;

pub use change::{
    already_ordered, apply_changes, area_lacks_fields, branch_example, names_known_kind,
    ChangeOrder, ChangePreview, OrderChange, Scope,
};
pub use groups::{
    area_pattern, group_rule, numbered_name, stay_groups, AnsweredGroups, GroupKey, NameTaken,
    RuleSaveFailure, StayGroup, StayGroups, MAX_GROUP_QUESTIONS,
};
pub use pattern::{
    pattern_choices, PatternChoice, PatternLevel, PatternPreview, PatternSpec, PatternTarget,
    OFFERED_PATTERNS,
};
pub use reading::{AiCoverage, FolderReading, ReadError, ReadProgress, FILE_LIMIT};
pub use runs::{RecentRun, RunItem, RunPlan, RunReport, RunSummary};
pub use trial::{StayReason, TrialEntry, TrialOutcome, TrialPlan};
pub use waiting::WaitingGroup;
pub use words::{normalize_word, same_word, word_within};

pub use crate::execute::{TrashItem, TrashReport, UndoReport};

type IndexSlot = Arc<Mutex<Option<Index>>>;

/// One piece of work for the chat.
#[derive(Debug, Clone)]
pub enum ChatWork {
    Read {
        folder: std::path::PathBuf,
        progress: Arc<ReadProgress>,
    },
    Trial {
        source: Arc<FolderReading>,
        target: Arc<FolderReading>,
        changes: Vec<OrderChange>,
        answered: AnsweredGroups,
        /// The order a new target without neighbours gets.
        layout: Option<Vec<PatternLevel>>,
    },
    SaveGroupRule {
        target: Arc<FolderReading>,
        key: GroupKey,
        label: String,
        area: String,
    },
    PreviewChange {
        reading: Arc<FolderReading>,
        kind_word: String,
        order: ChangeOrder,
        name: Option<NameSpec>,
    },
    /// `levels` empty: only the file names change.
    PreviewPattern {
        reading: Arc<FolderReading>,
        levels: Vec<PatternLevel>,
        name: Option<NameSpec>,
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
    WaitingCount,
    WaitingLoad,
    WaitingFile {
        items: Vec<ReviewItem>,
    },
    /// Files the documents in `folder`: below the target when relative.
    WaitingFileInto {
        items: Vec<ReviewItem>,
        folder: std::path::PathBuf,
    },
}

#[derive(Debug, Clone)]
pub enum ChatResult {
    Read(Result<Arc<FolderReading>, ReadError>),
    Trial(Result<TrialPlan, String>),
    /// The name of the rule that was stored.
    RuleSaved(Result<String, RuleSaveFailure>),
    Preview(Option<ChangePreview>),
    PatternPreviewed(Option<PatternPreview>),
    Filed(Result<RunReport, String>),
    Trashed(Result<TrashReport, String>),
    LastRun(Result<Option<RunSummary>, String>),
    Undone(Result<UndoReport, String>),
    WaitingCount(Result<usize, String>),
    WaitingLoaded(Result<Vec<WaitingGroup>, String>),
    WaitingFiled(Result<AcceptSummary, String>),
}

/// The handle of the chat: the index the pipeline shares, the text sources to
/// read files it does not know yet, and the trash for duplicates.
#[derive(Clone)]
pub struct ChatAccess {
    slot: IndexSlot,
    text_chain: Arc<TextChain>,
    trash: Arc<dyn Trash>,
    own: OwnDetails,
    review: Option<ReviewAccess>,
    load: Load,
}

impl fmt::Debug for ChatAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatAccess").finish_non_exhaustive()
    }
}

impl ChatAccess {
    pub fn kind_language(&self) -> KindLanguage {
        self.own.kind_language
    }

    pub fn new(slot: IndexSlot, text_chain: Arc<TextChain>, trash: Arc<dyn Trash>) -> Self {
        Self {
            slot,
            text_chain,
            trash,
            own: OwnDetails::default(),
            review: None,
            load: Load::default(),
        }
    }

    /// How many threads and at which priority a folder is read.
    #[must_use]
    pub fn with_load(mut self, load: Load) -> Self {
        self.load = load;
        self
    }

    /// The documents that wait for the user can be shown and filed only with
    /// this.
    #[must_use]
    pub fn with_review(mut self, review: Option<ReviewAccess>) -> Self {
        self.review = review;
        self
    }

    pub fn has_review(&self) -> bool {
        self.review.is_some()
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

    /// What the index knows of the order below `folder`; reads no file.
    pub fn known_structure(
        &self,
        folder: &std::path::Path,
    ) -> Result<ncrs_intake::LocationStructure, ReadError> {
        reading::known_structure(self, folder)
    }

    /// The runs that can still be put back, newest first.
    pub fn recent_runs(&self, limit: usize) -> Result<Vec<RecentRun>, String> {
        runs::recent_runs(self, limit)
    }

    /// How many documents the index holds below `folder`.
    pub fn documents_under(&self, folder: &std::path::Path) -> Result<usize, String> {
        self.with_index(|index| {
            index
                .documents_under(folder)
                .map(|found| found.len())
                .map_err(|err| err.to_string())
        })
    }

    /// How many archive documents the AI has read and sorted.
    pub fn understood_documents(&self) -> Result<u64, String> {
        self.with_index(|index| {
            index
                .archive_documents_understood()
                .map_err(|err| err.to_string())
        })
    }

    pub fn run(&self, work: ChatWork) -> ChatResult {
        match work {
            ChatWork::Read { folder, progress } => {
                ChatResult::Read(reading::read_folder(self, &folder, &progress).map(Arc::new))
            }
            ChatWork::Trial {
                source,
                target,
                changes,
                answered,
                layout,
            } => ChatResult::Trial(trial::trial(
                self,
                &source,
                &target,
                &changes,
                &answered,
                layout.as_deref(),
            )),
            ChatWork::SaveGroupRule {
                target,
                key,
                label,
                area,
            } => ChatResult::RuleSaved(self.save_group_rule(&target, &key, &label, &area)),
            ChatWork::PreviewChange {
                reading,
                kind_word,
                order,
                name,
            } => ChatResult::Preview(self.kinds().ok().and_then(|kinds| {
                change::preview(&kinds, &reading, &kind_word, order, name.as_ref())
            })),
            ChatWork::PreviewPattern {
                reading,
                levels,
                name,
            } => ChatResult::PatternPreviewed(
                self.kinds()
                    .ok()
                    .and_then(|kinds| pattern::preview(&kinds, &reading, &levels, name.as_ref())),
            ),
            ChatWork::File { run } => ChatResult::Filed(runs::file(self, &run)),
            ChatWork::TrashDuplicates { name, root, items } => {
                ChatResult::Trashed(runs::trash_duplicates(self, &name, &root, &items))
            }
            ChatWork::LastRun => ChatResult::LastRun(runs::last_run(self)),
            ChatWork::Undo { batch } => ChatResult::Undone(runs::undo(self, batch)),
            ChatWork::WaitingCount => ChatResult::WaitingCount(waiting::count(self)),
            ChatWork::WaitingLoad => ChatResult::WaitingLoaded(waiting::load(self)),
            ChatWork::WaitingFile { items } => {
                ChatResult::WaitingFiled(waiting::file(self, &items))
            }
            ChatWork::WaitingFileInto { items, folder } => {
                ChatResult::WaitingFiled(waiting::file_into(self, &items, &folder))
            }
        }
    }

    fn with_index<T>(&self, work: impl FnOnce(&Index) -> Result<T, String>) -> Result<T, String> {
        let guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_ref().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        work(index)
    }

    fn rules_and_aliases(&self) -> Result<(Vec<Rule>, SenderAliases), String> {
        self.with_index(|index| {
            let rules = index.rules().map_err(|err| err.to_string())?;
            let aliases = index
                .confirmed_sender_aliases()
                .map_err(|err| err.to_string())?;
            Ok((rules, aliases))
        })
    }

    /// Stores a confirmed rule that files the group below `area`; the name of
    /// the rule is the label of the group.
    fn save_group_rule(
        &self,
        target: &FolderReading,
        key: &GroupKey,
        label: &str,
        area: &str,
    ) -> Result<String, RuleSaveFailure> {
        let spec = group_rule(key, label, area, target);
        if let Some(problem) = rule_problem(&spec) {
            return Err(RuleSaveFailure::Refused(problem));
        }
        self.with_index_mut(|index| {
            index
                .create_rule_from(&spec, RuleOrigin::Hand)
                .map_err(|err| err.to_string())
        })
        .map_err(RuleSaveFailure::Failed)?;
        Ok(label.to_owned())
    }

    fn with_index_mut<T>(
        &self,
        work: impl FnOnce(&mut Index) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
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
