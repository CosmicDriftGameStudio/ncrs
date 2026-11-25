//! The state of the filing tab: one session, from the first plan to the run.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use ncrs_filing::chat_work::ReadProgress;
use ncrs_filing::sorting::{PlanRow, ReadProblem, RowPlace, RowState, RunEntry, WaitReason};
use ncrs_i18n::{Language, Msg};
use ncrs_index::{ContentHash, DocumentId, RuleId, TargetId};
use ncrs_intake::{LocationTransfer, OnNoText, OnSeveralTargets, SortSource, SortTarget};

use crate::question::Question;
use crate::run::RunProgress;
use crate::texts::say;

/// Tells a session from an earlier one when work it started comes back.
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Default)]
pub struct State {
    pub session: Option<Session>,
}

/// A file of the source with what the rules make of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Below the source.
    pub relative: PathBuf,
    pub path: PathBuf,
    pub hash: Option<ContentHash>,
    pub document: Option<DocumentId>,
    pub state: RowState,
}

impl From<PlanRow> for Row {
    fn from(row: PlanRow) -> Self {
        Self {
            relative: row.relative,
            path: row.path,
            hash: row.hash,
            document: row.document,
            state: row.state,
        }
    }
}

/// What the user decided for a file in place of the rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Places(Vec<(TargetId, PathBuf)>),
    Stay,
}

/// What the user can do about a row right there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    MakeRule,
    SuggestRule,
    Assign,
    Leave,
    Ask,
}

/// What a run left.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub filed: usize,
    pub left: usize,
    pub trashed: usize,
    /// One line per file that was not filed.
    pub problems: Vec<String>,
}

#[derive(Debug)]
pub struct Session {
    pub id: u64,
    pub source: SortSource,
    /// The whole folder once; otherwise the open questions of a watched source.
    pub once: bool,
    /// All configured targets in their order: the first one is T1.
    pub targets: Vec<SortTarget>,
    pub rule_names: HashMap<RuleId, String>,
    pub rows: Vec<Row>,
    pub waiting: Vec<(TargetId, WaitReason)>,
    /// A plan came back, so no rows means there are none.
    pub planned: bool,
    /// A plan is being made; with `reading` it reads files too.
    pub planning: bool,
    pub reading: bool,
    /// The rules changed while a plan was made.
    pub stale: bool,
    pub progress: Arc<ReadProgress>,
    pub stop: Arc<AtomicBool>,
    /// By the path below the source.
    pub changes: BTreeMap<PathBuf, Change>,
    /// Below the source.
    pub selected: Option<PathBuf>,
    /// The link of the selected row the keyboard is on.
    pub marked_link: Option<usize>,
    /// The name typed for the selected file.
    pub renaming: Option<String>,
    pub question: Option<Question>,
    pub only_new: bool,
    pub trash_dupes: bool,
    /// Duplicates can be taken back from the trash of this system.
    pub trash_can_restore: bool,
    /// A target, and a folder below it.
    pub closed_folders: BTreeSet<(TargetId, PathBuf)>,
    /// The names of the files that are in a folder of a target.
    pub existing: BTreeMap<(TargetId, PathBuf), Vec<String>>,
    /// The folders of the targets, read when the tree shows them all.
    pub target_folders: BTreeSet<(TargetId, PathBuf)>,
    /// The folders whose files are being read right now.
    pub reading_folders: BTreeSet<(TargetId, PathBuf)>,
    /// The folders of the targets are being listed.
    pub listing: bool,
    pub running: Option<Arc<RunProgress>>,
    /// The rows whose question is being answered.
    pub answering: BTreeSet<PathBuf>,
    pub outcome: Option<Outcome>,
    pub error: Option<String>,
}

impl Session {
    pub fn new(source: SortSource, once: bool) -> Self {
        Self {
            id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
            source,
            once,
            targets: Vec::new(),
            rule_names: HashMap::new(),
            rows: Vec::new(),
            waiting: Vec::new(),
            planned: false,
            planning: false,
            reading: false,
            stale: false,
            progress: Arc::default(),
            stop: Arc::default(),
            changes: BTreeMap::new(),
            selected: None,
            marked_link: None,
            renaming: None,
            question: None,
            only_new: true,
            trash_dupes: false,
            trash_can_restore: false,
            closed_folders: BTreeSet::new(),
            existing: BTreeMap::new(),
            target_folders: BTreeSet::new(),
            reading_folders: BTreeSet::new(),
            listing: false,
            running: None,
            answering: BTreeSet::new(),
            outcome: None,
            error: None,
        }
    }

    pub fn source_name(&self) -> String {
        self.source.path.file_name().map_or_else(
            || self.source.path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    /// The targets of the source in their order, with the index that gives
    /// each its number and colour.
    pub fn source_targets(&self) -> impl Iterator<Item = (usize, &SortTarget)> {
        self.targets
            .iter()
            .enumerate()
            .filter(|(_, target)| self.source.targets.contains(&target.id))
    }

    pub fn target_index(&self, id: &TargetId) -> Option<usize> {
        self.targets.iter().position(|target| target.id == *id)
    }

    /// "T1".
    pub fn target_number(&self, id: &TargetId) -> String {
        self.target_index(id)
            .map_or_else(|| id.to_string(), |index| format!("T{}", index + 1))
    }

    pub fn rule_name(&self, id: RuleId) -> &str {
        self.rule_names.get(&id).map_or("", String::as_str)
    }

    pub fn row(&self, relative: &Path) -> Option<&Row> {
        self.rows.iter().find(|row| row.relative == relative)
    }

    pub fn selected_row(&self) -> Option<&Row> {
        self.row(self.selected.as_ref()?)
    }

    /// Where a run would file the row: what the user decided, or else the rule.
    pub fn places<'a>(&'a self, row: &'a Row) -> Cow<'a, [(TargetId, PathBuf)]> {
        match (self.changes.get(&row.relative), &row.state) {
            (Some(Change::Places(places)), _) => Cow::Borrowed(places),
            (Some(Change::Stay), _) => Cow::Borrowed(&[]),
            (None, RowState::Place(place)) => Cow::Owned(vec![place_of(place)]),
            (None, RowState::Several(places)) if self.copies_to_all() => {
                Cow::Owned(places.iter().map(place_of).collect())
            }
            (None, _) => Cow::Borrowed(&[]),
        }
    }

    pub fn copies_to_all(&self) -> bool {
        self.once && self.source.several == OnSeveralTargets::CopyToAll
    }

    pub fn copy_only(&self) -> bool {
        self.source.transfer == LocationTransfer::Copy
    }

    /// The one place a new name or folder is given to.
    pub fn one_place(&self, row: &Row) -> Option<(TargetId, PathBuf)> {
        if let [place] = &*self.places(row) {
            return Some(place.clone());
        }
        match &row.state {
            RowState::NameTaken { place, .. } if !self.changes.contains_key(&row.relative) => {
                Some(place_of(place))
            }
            _ => None,
        }
    }

    /// What a run would carry out.
    pub fn entries(&self) -> Vec<RunEntry> {
        if !self.once {
            return Vec::new();
        }
        self.rows
            .iter()
            .filter_map(|row| {
                let places = self.places(row);
                if places.is_empty() {
                    return None;
                }
                Some(RunEntry {
                    path: row.path.clone(),
                    hash: row.hash?,
                    document: row.document,
                    places: places.into_owned(),
                })
            })
            .collect()
    }

    pub fn links(&self, row: &Row) -> Vec<Link> {
        if self.answering.contains(&row.relative) || self.running.is_some() {
            return Vec::new();
        }
        let several = matches!(&row.state, RowState::Several(_)) && !self.copies_to_all();
        if self.changes.contains_key(&row.relative) {
            return if several { vec![Link::Ask] } else { Vec::new() };
        }
        match &row.state {
            RowState::NoRule if self.once => vec![Link::MakeRule, Link::SuggestRule],
            RowState::NoRule => vec![Link::MakeRule, Link::SuggestRule, Link::Leave],
            RowState::NoText if self.asks_without_text() => vec![Link::Assign, Link::Leave],
            RowState::Several(_) if several => vec![Link::Ask],
            _ => Vec::new(),
        }
    }

    /// A watched source only has a question about such a file when it asks.
    pub fn asks_without_text(&self) -> bool {
        !self.once || self.source.no_text == OnNoText::Ask
    }

    /// Nothing may be filed while one of the targets is not ready, and not from
    /// a plan that is still being made.
    pub fn can_file(&self) -> bool {
        self.once
            && self.planned
            && !self.planning
            && !self.is_filing()
            && self.waiting.is_empty()
            && !self.entries().is_empty()
    }

    pub fn is_filing(&self) -> bool {
        self.running.is_some() || !self.answering.is_empty()
    }

    /// Still being read, so not a problem yet.
    pub fn is_being_read(&self, row: &Row) -> bool {
        self.reading && row.state == RowState::NotRead(ReadProblem::Unknown)
    }
}

pub fn place_of(place: &RowPlace) -> (TargetId, PathBuf) {
    (place.target.clone(), place.relative.clone())
}

/// The app ticks while it is.
pub fn is_running(state: &State) -> bool {
    state
        .session
        .as_ref()
        .is_some_and(|session| session.planning || session.is_filing())
}

/// Asks the planning and the run to end; the app calls it on quit.
pub fn stop(state: &State) {
    if let Some(session) = &state.session {
        session.stop.store(true, Ordering::Relaxed);
        if let Some(running) = &session.running {
            running.stop();
        }
    }
}

/// The source the tab is open for. The background filing leaves it alone.
pub fn busy_source(state: &State) -> Option<&Path> {
    state
        .session
        .as_ref()
        .map(|session| session.source.path.as_path())
}

/// A question lies over the tab and takes the keys.
pub fn question_is_open(state: &State) -> bool {
    state
        .session
        .as_ref()
        .is_some_and(|session| session.question.is_some())
}

/// The title of the tab, `None` without a session.
pub fn tab_title(state: &State, lang: Language) -> Option<String> {
    let session = state.session.as_ref()?;
    Some(say(
        lang,
        Msg::FilingViewTabTitle,
        &[("folder", &session.source_name())],
    ))
}
