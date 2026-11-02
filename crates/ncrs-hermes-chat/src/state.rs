use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use ncrs_ai::{ChatTurn, Schedule};
use ncrs_filing::chat_work::{
    ChangePreview, ChatWork, FolderReading, OrderChange, RunSummary, TrashItem, TrialPlan,
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
    UnsureChoice { entry: usize, choice: usize },
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
    pub offer: Option<Offer>,
    pub last_run: Option<RunSummary>,
    /// A typed folder waiting for a yes, and the step it was typed in.
    pub confirm: Option<(PathBuf, Step)>,
    pub proposal: Option<Proposal>,
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
