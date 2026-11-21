//! The work behind the Hermes chat. It blocks, so the chat runs it on a
//! blocking thread, never in `update`.
//!
//! Reading and the trial touch nothing on disk and write nothing to the
//! index. Only `File` and `TrashDuplicates` change files, and both go
//! through the journal, so `Undo` can put them back.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_ai::ChatModel;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_fs::Trash;
use ncrs_index::{BatchId, Index, KindLanguage, Kinds, Rule, RuleOrigin, RuleSpec, SenderAliases};
use ncrs_intake::{rule_problem, TextChain};
use ncrs_pipeline::{Load, OwnDetails};

use crate::index_slot::try_lock;
use crate::review_work::{AcceptSummary, ReviewAccess, ReviewItem};
use crate::rules_work::{ArchiveMove, RuleTry, RulesAccess, TriedDocument, TryFile};

mod change;
mod folder_list;
mod groups;
mod live;
mod pattern;
mod picks;
mod reading;
mod runs;
mod trial;
mod waiting;
mod words;

#[cfg(test)]
mod tests;

pub use change::{branch_example, segments};
pub use groups::{
    area_pattern, group_rule, numbered_name, stay_groups, AnsweredGroups, GroupKey, NameTaken,
    RuleSaveFailure, StayGroup, StayGroups, MAX_GROUP_QUESTIONS,
};
pub use live::{
    apply_overrides, classify, mark_files, plan_live, run_plan, tally, LiveFile, LivePlan, LiveRun,
    LiveState, LiveTags, Override, Tally,
};
pub use pattern::PatternLevel;
pub use picks::{remember_folder_picks, LEFT_UNKNOWN};
pub use reading::{AiCoverage, FolderReading, ReadError, ReadProgress, FILE_LIMIT};
pub use runs::{RecentRun, RunItem, RunPlan, RunReport, RunSummary};
pub use trial::{
    ModelScope, PlanFailure, StayReason, TrialEntry, TrialOutcome, TrialPlan, MODEL_FIRST_PAGE,
};
pub use waiting::WaitingGroup;
pub use words::{normalize_word, same_word, word_within};

pub use crate::execute::{TrashItem, TrashReport, UndoReport};

type IndexSlot = Arc<Mutex<Option<Index>>>;

/// Never shown: the UI treats it like "no answer now" and tries again later.
const INDEX_BUSY: &str = "the index is busy with a run";

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
        answered: AnsweredGroups,
        /// The order a new target without neighbours gets.
        layout: Option<Vec<PatternLevel>>,
        /// Which files the model is asked about now.
        scope: ModelScope,
    },
    SaveGroupRule {
        target: Arc<FolderReading>,
        key: GroupKey,
        label: String,
        area: String,
    },
    /// Where a rule would file the documents the index knows; nothing is
    /// written.
    RuleTrial {
        spec: RuleSpec,
        /// The files of the folder as read from the disk: the index may not
        /// know them yet.
        files: Vec<TryFile>,
    },
    /// Stores a rule the user confirmed.
    SaveRule {
        spec: RuleSpec,
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
    /// The batches of one tidy-up, newest first.
    UndoAll {
        batches: Vec<BatchId>,
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
    Trial(Result<Box<TrialPlan>, String>),
    /// The name of the rule that was stored.
    RuleSaved(Result<String, RuleSaveFailure>),
    RuleTried(Result<RuleTrial, String>),
    RuleStored(Result<ncrs_index::RuleSaved, RuleSaveFailure>),
    Filed(Result<RunReport, String>),
    Trashed(Result<TrashReport, String>),
    LastRun(Result<Option<RunSummary>, String>),
    Undone(Result<UndoReport, String>),
    WaitingCount(Result<usize, String>),
    WaitingLoaded(Result<Vec<WaitingGroup>, String>),
    WaitingFiled(Result<AcceptSummary, String>),
}

/// What a rule would do to the documents the index knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleTrial {
    /// All documents the rule matches.
    pub matches: usize,
    /// The archive documents that already lie where the rule puts them.
    pub right: usize,
    /// The matches that are not in the archive, so they come from the inbox.
    pub from_inbox: usize,
    /// The first of those that would move, as they stand and as they would
    /// stand.
    pub examples: Vec<TriedDocument>,
    /// The files given to the trial that the index does not know yet.
    pub not_indexed: usize,
    /// The archive documents the rule renames or moves.
    pub archive_moves: Vec<ArchiveMove>,
    /// The archive documents that lie wrong but cannot move in one run: two
    /// want the same name, or they swap names.
    pub archive_blocked: usize,
}

/// How many matching documents the chat shows.
pub const RULE_TRIAL_EXAMPLES: usize = 3;

/// The handle of the chat: the index the pipeline shares, the text sources to
/// read files it does not know yet, and the trash for duplicates.
#[derive(Clone)]
pub struct ChatAccess {
    slot: IndexSlot,
    text_chain: Arc<TextChain>,
    trash: Arc<dyn Trash>,
    own: OwnDetails,
    review: Option<ReviewAccess>,
    rules: Option<RulesAccess>,
    load: Load,
    model: Option<Arc<dyn ChatModel>>,
    copy_roots: Vec<PathBuf>,
    index_work: crate::index_work::IndexWork,
    last_kinds: Arc<Mutex<Option<Kinds>>>,
    last_rules: Arc<Mutex<Option<Vec<Rule>>>>,
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
            rules: None,
            load: Load::default(),
            model: None,
            copy_roots: Vec::new(),
            index_work: crate::index_work::IndexWork::default(),
            last_kinds: Arc::default(),
            last_rules: Arc::default(),
        }
    }

    #[must_use]
    pub fn with_index_work(mut self, index_work: crate::index_work::IndexWork) -> Self {
        self.index_work = index_work;
        self
    }

    /// The work of the caller counts as running until the guard is dropped.
    pub fn begin_work(&self) -> crate::index_work::IndexWorkGuard {
        self.index_work.begin()
    }

    /// The read folders whose files are copied, never emptied: a run out of
    /// one of them copies, and duplicates in it are not trashed.
    #[must_use]
    pub fn with_copy_roots(mut self, copy_roots: Vec<PathBuf>) -> Self {
        self.copy_roots = copy_roots;
        self
    }

    /// Whether the files below `folder` are copied out, not moved.
    pub fn copies_from(&self, folder: &Path) -> bool {
        runs::lies_in_copy_root(&self.copy_roots, folder)
    }

    /// The model that picks a folder for the files the neighbours cannot place.
    #[must_use]
    pub fn with_model(mut self, model: Option<Arc<dyn ChatModel>>) -> Self {
        self.model = model;
        self
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

    /// A rule can be tried against the documents only with this.
    #[must_use]
    pub fn with_rules(mut self, rules: Option<RulesAccess>) -> Self {
        self.rules = rules;
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

    /// How many documents below `folder` have a text, and how many of them
    /// the AI has read and sorted.
    pub fn ai_coverage_under(&self, folder: &std::path::Path) -> Result<(usize, usize), String> {
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
                ChatResult::Read(reading::read_folder(self, &folder, &progress, None).map(Arc::new))
            }
            ChatWork::Trial {
                source,
                target,
                answered,
                layout,
                scope,
            } => ChatResult::Trial(
                trial::trial(
                    self,
                    &source,
                    &target,
                    &answered,
                    layout.as_deref(),
                    self.model.as_deref(),
                    &scope,
                )
                .map(Box::new),
            ),
            ChatWork::SaveGroupRule {
                target,
                key,
                label,
                area,
            } => ChatResult::RuleSaved(self.save_group_rule(&target, &key, &label, &area)),
            ChatWork::RuleTrial { spec, files } => {
                ChatResult::RuleTried(self.try_rule(spec, &files))
            }
            ChatWork::SaveRule { spec } => ChatResult::RuleStored(self.save_rule(&spec)),
            ChatWork::File { run } => ChatResult::Filed(runs::file(self, &run)),
            ChatWork::TrashDuplicates { name, root, items } => {
                ChatResult::Trashed(runs::trash_duplicates(self, &name, &root, &items))
            }
            ChatWork::LastRun => ChatResult::LastRun(runs::last_run(self)),
            ChatWork::Undo { batch } => ChatResult::Undone(runs::undo(self, batch)),
            ChatWork::UndoAll { batches } => ChatResult::Undone(runs::undo_all(self, &batches)),
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
            let created = index
                .create_rule_from(&spec, RuleOrigin::Chat)
                .map_err(|err| err.to_string())?;
            if let Ok(rules) = index.rules() {
                self.remember_rules(&rules);
            }
            Ok(created)
        })
        .map_err(RuleSaveFailure::Failed)?;
        Ok(label.to_owned())
    }

    fn try_rule(&self, spec: RuleSpec, files: &[TryFile]) -> Result<RuleTrial, String> {
        let replaces =
            self.with_index(|index| index.rule_replaced_by(&spec).map_err(|err| err.to_string()))?;
        let rules = self
            .rules
            .as_ref()
            .ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        let tried = rules.try_rule_with(&RuleTry { replaces, spec }, files)?;
        Ok(RuleTrial {
            matches: tried.documents.len() + tried.more,
            right: tried
                .in_archive
                .saturating_sub(tried.archive_moves.len() + tried.archive_blocked),
            from_inbox: tried.documents.len() + tried.more - tried.in_archive,
            examples: tried
                .documents
                .into_iter()
                .filter(|document| !document.is_in_archive())
                .take(RULE_TRIAL_EXAMPLES)
                .collect(),
            archive_moves: tried.archive_moves,
            archive_blocked: tried.archive_blocked,
            not_indexed: tried.not_indexed,
        })
    }

    fn save_rule(&self, spec: &RuleSpec) -> Result<ncrs_index::RuleSaved, RuleSaveFailure> {
        if let Some(problem) = rule_problem(spec) {
            return Err(RuleSaveFailure::Refused(problem));
        }
        self.with_index_mut(|index| {
            let saved = index
                .save_rule_from(spec, RuleOrigin::Chat)
                .map_err(|err| err.to_string())?;
            if let Ok(rules) = index.rules() {
                self.remember_rules(&rules);
            }
            Ok(saved)
        })
        .map_err(RuleSaveFailure::Failed)
    }

    fn with_index_mut<T>(
        &self,
        work: impl FnOnce(&mut Index) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        work(index)
    }

    /// For the UI thread: while a run holds the index the rules last read or
    /// saved are returned; `Err` at once only when there are none yet.
    pub fn try_rules(&self) -> Result<Vec<Rule>, String> {
        match self.try_with_index(|index| index.rules().map_err(|err| err.to_string())) {
            Ok(rules) => {
                self.remember_rules(&rules);
                Ok(rules)
            }
            Err(err) if err == INDEX_BUSY => self
                .last_rules
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .ok_or(err),
            Err(err) => Err(err),
        }
    }

    fn remember_rules(&self, rules: &[Rule]) {
        *self
            .last_rules
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(rules.to_vec());
    }

    /// For the UI thread: while a run holds the index the kinds last read
    /// are returned; `Err` at once only when there are none yet.
    pub fn try_kinds(&self) -> Result<Kinds, String> {
        match self.try_with_index(|index| Ok(self.kinds_of(index))) {
            Ok(kinds) => {
                self.remember_kinds(&kinds);
                Ok(kinds)
            }
            Err(err) if err == INDEX_BUSY => self
                .last_kinds
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .ok_or(err),
            Err(err) => Err(err),
        }
    }

    pub fn kinds(&self) -> Result<Kinds, String> {
        let kinds = self.with_index(|index| Ok(self.kinds_of(index)))?;
        self.remember_kinds(&kinds);
        Ok(kinds)
    }

    fn remember_kinds(&self, kinds: &Kinds) {
        *self
            .last_kinds
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(kinds.clone());
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
