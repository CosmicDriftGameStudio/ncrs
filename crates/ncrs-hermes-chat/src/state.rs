use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ncrs_ai::{ChatTurn, Schedule};
use ncrs_filing::chat_work::{
    AnsweredGroups, ChangePreview, ChatWork, FolderReading, OrderChange, RunSummary, TrashItem,
    TrialPlan,
};
use ncrs_index::Transfer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChipReply {
    Cancel,
    Folder(PathBuf),
    PickFolder,
    Setup,
    ReadAnyway,
    KeepYes,
    WantChange,
    ShowAll,
    ScopeAll,
    ScopeOnly(String),
    FineAsIs,
    ToTrash,
    LeaveThem,
    Later,
    Once,
    Always,
    ModeCopy,
    ModeMove,
    FileAll,
    ShowMore,
    /// Files the group in question below this first-level folder.
    GroupArea(String),
    /// One of the folders a name the model heard can mean.
    NamedFolder(PathBuf),
    GroupNewFolder,
    GroupLeave,
    SinglesShow,
    SinglesLeave,
    NamesNumbered,
    NamesLeave,
    UnsureChoice {
        entry: usize,
        choice: usize,
    },
    PutBack,
    Undo,
    OpenSearch,
    IntentSearch,
    IntentTidy,
    IntentStructure,
    IntentUndo,
    DownloadModel,
    ConfirmTidy,
    ConfirmChange,
    OtherTarget,
    SwitchToCopy,
    SwitchToMove,
}

impl ChipReply {
    /// Whether handling the reply writes files, trash, rules or settings, or
    /// settles what a later filing run does. A typed answer never runs such a
    /// reply on its own.
    pub fn changes_something(&self) -> bool {
        match self {
            Self::KeepYes
            | Self::FineAsIs
            | Self::ToTrash
            | Self::FileAll
            | Self::GroupArea(_)
            | Self::NamesNumbered
            | Self::PutBack
            | Self::Undo => true,
            Self::Cancel
            | Self::Folder(_)
            | Self::PickFolder
            | Self::Setup
            | Self::ReadAnyway
            | Self::WantChange
            | Self::ShowAll
            | Self::ScopeAll
            | Self::ScopeOnly(_)
            | Self::LeaveThem
            | Self::Later
            | Self::Once
            | Self::Always
            | Self::ModeCopy
            | Self::ModeMove
            | Self::ShowMore
            | Self::NamedFolder(_)
            | Self::GroupNewFolder
            | Self::GroupLeave
            | Self::SinglesShow
            | Self::SinglesLeave
            | Self::NamesLeave
            | Self::UnsureChoice { .. }
            | Self::OpenSearch
            | Self::IntentSearch
            | Self::IntentTidy
            | Self::IntentStructure
            | Self::IntentUndo
            | Self::DownloadModel
            | Self::ConfirmTidy
            | Self::ConfirmChange
            | Self::OtherTarget
            | Self::SwitchToCopy
            | Self::SwitchToMove => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub label: String,
    pub reply: ChipReply,
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaRow {
    pub name: String,
    pub description: String,
    pub documents: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountRow {
    pub text: String,
    pub documents: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreasCard {
    pub rows: Vec<AreaRow>,
    pub more: Option<CountRow>,
    pub unordered: Option<CountRow>,
}

/// A path as segments, one of them highlighted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCard {
    pub segments: Vec<String>,
    pub highlighted: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowOutcome {
    Filed {
        segments: Vec<String>,
    },
    Unsure {
        question: String,
        choices: Vec<Chip>,
        chosen: Option<usize>,
    },
    AlreadyThere(String),
    Stays(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialRow {
    /// Position in the trial plan.
    pub entry: usize,
    pub name: String,
    pub outcome: RowOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialCard {
    pub header: String,
    pub rows: Vec<TrialRow>,
    pub more: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Text(String),
    Areas(AreasCard),
    File(FileCard),
    Trial(TrialCard),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Hermes { parts: Vec<Part>, chips: Vec<Chip> },
    User(String),
    Working(String),
}

/// What the next answer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Idle,
    /// A folder to read, to learn its order.
    ReadFolder,
    ConfirmUnknown,
    Keep,
    Change,
    Scope,
    ConfirmChange,
    AfterChange,
    ConfirmTidy,
    OtherTarget,
    Source,
    TidyFolder,
    Target,
    Once,
    Mode,
    Trial,
    /// The next typed text is the name of a new folder, not for the model.
    GroupFolderName,
    AfterRun,
    UndoAsk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadFor {
    Learn,
    Source,
    Target,
}

#[derive(Debug, Clone)]
pub struct Tidy {
    pub source: PathBuf,
    pub target: Option<PathBuf>,
    pub transfer: Option<Transfer>,
}

#[derive(Debug, Clone)]
pub struct TrialSession {
    pub plan: TrialPlan,
    pub resolved: BTreeMap<usize, PathBuf>,
    pub shown: usize,
}

/// Where the questions about the staying files stand.
#[derive(Debug, Default)]
pub struct GroupFlow {
    pub answered: AnsweredGroups,
    /// The unsure files the user decided, as source path and chosen folder,
    /// to carry over when the trial runs again.
    pub carried: Vec<(PathBuf, PathBuf)>,
    /// The rule stored last, announced with the next trial.
    pub fresh_rule: Option<String>,
    pub singles_done: bool,
    pub singles_shown: usize,
    pub names_done: bool,
}

/// Duplicates the chat offers to trash.
#[derive(Debug, Clone)]
pub struct Offer {
    pub name: String,
    pub root: PathBuf,
    pub items: Vec<TrashItem>,
}

/// What the chat understood from a sentence and now waits for a yes to.
#[derive(Debug, Clone)]
pub struct Proposal {
    pub source: PathBuf,
    pub target: Option<PathBuf>,
    pub schedule: Option<Schedule>,
    /// Only set when the user said it.
    pub transfer: Option<Transfer>,
}

/// The question the user answered by typing: its text and its buttons, which
/// the next bubble of the user clears.
#[derive(Debug, Clone, Default)]
pub struct Asked {
    pub question: Option<String>,
    pub chips: Vec<Chip>,
}

/// A sentence waiting for the language model to be downloaded.
#[derive(Debug, Clone)]
pub struct Unsent {
    pub message: String,
    pub asked: Asked,
}

/// The sentence the language model works on.
#[derive(Debug, Clone)]
pub struct Thinking {
    pub turn: ChatTurn,
    pub asked: Asked,
}

#[derive(Debug, Default)]
pub struct Session {
    pub readings: HashMap<PathBuf, Arc<FolderReading>>,
    /// Folders learned in this chat that are not yet a target of the app.
    pub learned: Vec<PathBuf>,
    /// The reading the order questions are about.
    pub learning: Option<Arc<FolderReading>>,
    pub read_for: Option<ReadFor>,
    pub resume_tidy: bool,
    pub tidy: Option<Tidy>,
    pub changes: Vec<OrderChange>,
    pub pending_change: Option<ChangePreview>,
    pub change_word: String,
    /// The folder the running read is for.
    pub requested: PathBuf,
    pub trial: Option<TrialSession>,
    pub groups: GroupFlow,
    pub offer: Option<Offer>,
    pub last_run: Option<RunSummary>,
    /// A typed folder waiting for a yes, and the step it was typed in.
    pub confirm: Option<(PathBuf, Step)>,
    pub proposal: Option<Proposal>,
    /// The names the user is asked to choose a folder for.
    pub choosing: Option<crate::named_folders::Naming>,
    /// What the model said about the scope of a change, for the preview.
    pub change_scope: Option<String>,
    pub unsent: Option<Unsent>,
    /// The duplicates of the folder learned first, offered after the first
    /// filing, once.
    pub archive_duplicates: Option<Offer>,
    pub archive_offered: bool,
}

#[derive(Debug)]
pub struct State {
    pub open: bool,
    pub input: String,
    pub thread: Vec<Entry>,
    pub pending: Option<ChatWork>,
    /// Work set during this `update` that no task runs yet.
    pub fresh: bool,
    pub thinking: Option<Thinking>,
    /// The turn set during this `update` that no task runs yet.
    pub fresh_turn: bool,
    /// A typed folder being checked off the UI thread, and the step it was
    /// typed in.
    pub resolving: Option<(PathBuf, Step)>,
    /// The check set during this `update` that no task runs yet.
    pub fresh_resolve: bool,
    /// The folders a sentence named, being checked off the UI thread.
    pub naming: Option<crate::named_folders::Naming>,
    /// The check set during this `update` that no task runs yet.
    pub fresh_naming: bool,
    pub step: Step,
    pub session: Session,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            input: String::new(),
            thread: Vec::new(),
            pending: None,
            fresh: false,
            thinking: None,
            fresh_turn: false,
            resolving: None,
            fresh_resolve: false,
            naming: None,
            fresh_naming: false,
            step: Step::Idle,
            session: Session::default(),
        }
    }
}

impl State {
    pub fn thread(&self) -> &[Entry] {
        &self.thread
    }

    /// The work the chat waits for; the app runs it on a blocking thread.
    pub fn pending_work(&self) -> Option<&ChatWork> {
        self.pending.as_ref()
    }

    /// The turn the language model waits for; the app asks the model on a
    /// blocking thread.
    pub fn pending_turn(&self) -> Option<&ChatTurn> {
        self.thinking.as_ref().map(|thinking| &thinking.turn)
    }

    /// The typed folder the app waits to have checked, and its step.
    pub fn pending_resolve(&self) -> Option<(&Path, Step)> {
        self.resolving
            .as_ref()
            .map(|(typed, step)| (typed.as_path(), *step))
    }

    /// The names the app waits to have checked.
    pub fn pending_naming(&self) -> Option<&crate::named_folders::Naming> {
        self.naming.as_ref()
    }

    /// The plan of the trial that is on screen.
    pub fn trial_plan(&self) -> Option<&TrialPlan> {
        self.session.trial.as_ref().map(|trial| &trial.plan)
    }

    /// The chips of the newest entry, when it is Hermes'.
    pub fn chips(&self) -> &[Chip] {
        match self.thread.last() {
            Some(Entry::Hermes { chips, .. }) => chips,
            _ => &[],
        }
    }
}
