use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ncrs_ai::{ChatTurn, Schedule};
use ncrs_filing::chat_work::{
    AnsweredGroups, ChangeOrder, ChangePreview, ChatWork, FolderReading, OrderChange,
    PatternChoice, PatternLevel, ReadProgress, RunPlan, RunSummary, TrashItem, TrialPlan,
};
use ncrs_index::Transfer;

use crate::action::Action;

/// How a new, empty folder to file into is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewLayout {
    Year,
    Sender,
    Kind,
    Flat,
}

/// Filing a source file by file: where the walk stands and what is decided.
#[derive(Debug, Clone)]
pub struct Deciding {
    pub source: Arc<FolderReading>,
    pub next: usize,
    /// The position of a file in the source and the folder it goes to.
    pub picks: Vec<(usize, PathBuf)>,
    /// One run for every folder the files go to, the next one last.
    pub runs: Vec<RunPlan>,
    pub done: usize,
    pub skipped: usize,
}

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
    RoleSource,
    RoleTarget,
    /// Uses the pattern at this position of `Session::patterns` everywhere.
    AdoptPattern(usize),
    PhotosYes,
    PhotosNo,
    /// Opens the picker for a new folder to file into.
    NewTarget,
    DecidePerFile,
    /// The area the user meant by a word Hermes found no folder for.
    ChangeArea(String),
    DecideTarget(PathBuf),
    DecideSkip,
    NewLayout(NewLayout),
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
    TurnOnAi,
    ConfirmTidy,
    ConfirmChange,
    OtherTarget,
    SwitchToCopy,
    SwitchToMove,
    /// Reads the folder again that could not be read.
    Retry,
    /// Asks the user to type the name of the area.
    OtherArea,
    /// Asks for this order in this area.
    ChangeAreaOrder {
        area: String,
        order: ChangeOrder,
    },
    /// Moves the files that are in the filing folder already to the trash.
    TrashDuplicatesNow,
    /// Lists the files of the trial that stay.
    ShowStaying,
    /// Runs the trial again against a folder the user picks.
    TrialOtherTarget,
}

impl Session {
    /// Something the user asked that waits for Hermes to start.
    pub fn holds_something(&self) -> bool {
        self.unsent.is_some() || self.unsent_reply.is_some()
    }
}

impl ChipReply {
    /// Pressing it does not need the filing module.
    pub fn needs_no_hermes(&self) -> bool {
        matches!(self, Self::Setup | Self::PickFolder)
    }

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
            | Self::RoleSource
            | Self::DecideTarget(_)
            | Self::NewLayout(_)
            | Self::NamesNumbered
            | Self::PutBack
            | Self::TrashDuplicatesNow
            | Self::Undo => true,
            Self::Cancel
            | Self::Folder(_)
            | Self::PickFolder
            | Self::Setup
            | Self::ReadAnyway
            | Self::WantChange
            | Self::ShowAll
            | Self::ScopeAll
            | Self::ChangeArea(_)
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
            | Self::RoleTarget
            | Self::AdoptPattern(_)
            | Self::PhotosYes
            | Self::NewTarget
            | Self::DecidePerFile
            | Self::DecideSkip
            | Self::PhotosNo
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
            | Self::TurnOnAi
            | Self::ConfirmTidy
            | Self::ConfirmChange
            | Self::OtherTarget
            | Self::SwitchToCopy
            | Self::SwitchToMove
            | Self::Retry
            | Self::OtherArea
            | Self::ChangeAreaOrder { .. }
            | Self::ShowStaying
            | Self::TrialOtherTarget => false,
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
    /// The folders below the area and the file, "Year / Sender / File".
    pub description: String,
    /// The path of a real document laid out that way; empty for none.
    pub example: String,
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

/// A folder named in the chat: its name, and where it lies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderMention {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Text(String),
    Areas(AreasCard),
    File(FileCard),
    Trial(TrialCard),
    Folder(FolderMention),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Hermes { parts: Vec<Part>, chips: Vec<Chip> },
    User(String),
    UserFolder(FolderMention),
    Working(String),
}

/// What the next answer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Idle,
    /// A folder to read, to learn its order.
    ReadFolder,
    /// The opening question: any folder, routed by what it is for.
    Greeting,
    /// What a folder with no role is for.
    Role,
    /// Whether to read the photos of a source.
    Photos,
    /// The next picked folder is a new one to file into.
    NewTarget,
    /// How a new folder to file into is ordered.
    NewLayout,
    /// Which folder a file goes to.
    Decide,
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
    /// The next typed text is the name of the area a change is for.
    AreaName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadFor {
    Learn,
    Source,
    Target,
    NewTarget,
}

/// A read that failed because the folder was not there, to run again.
#[derive(Debug, Clone)]
pub struct Retry {
    pub folder: PathBuf,
    pub purpose: ReadFor,
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
    /// The folder whose role the chat asked about.
    pub role_asked: Option<PathBuf>,
    /// The source whose photos the chat asked about.
    pub photos_of: Option<Arc<FolderReading>>,
    /// The order a new target without neighbours gets.
    pub layout: Option<Vec<PatternLevel>>,
    pub deciding: Option<Deciding>,
    pub tidy: Option<Tidy>,
    pub changes: Vec<OrderChange>,
    pub pending_change: Option<ChangePreview>,
    /// The order asked for, kept while Hermes asks which area is meant.
    pub change_order: Option<ChangeOrder>,
    /// The area was picked from the buttons: no second question if it fails.
    pub area_chosen: bool,
    /// The patterns on offer after the areas card.
    pub patterns: Vec<PatternChoice>,
    /// The pattern change waiting for a yes.
    pub pending_pattern: Option<OrderChange>,
    pub change_word: String,
    /// The folder the running read is for.
    pub requested: PathBuf,
    pub retry: Option<Retry>,
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
    /// A button or picked folder that waits for Hermes to start.
    pub unsent_reply: Option<ChipReply>,
    /// How far the running read is.
    pub progress: Option<Arc<ReadProgress>>,
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
    /// The opening question waits for the AI or the model; it follows once
    /// they are there.
    pub greeting_owed: bool,
    /// The chat was opened or written to while Hermes was still starting;
    /// the greeting or the sentence follows once it is ready.
    pub waiting_for_hermes: bool,
    /// Counts the ticks of the wait; moves the dots of the working line.
    pub tick: usize,
    /// An action a result of work asks for; `update` hands it to the app.
    pub queued_action: Option<Action>,
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
            greeting_owed: false,
            waiting_for_hermes: false,
            tick: 0,
            queued_action: None,
            session: Session::default(),
        }
    }
}

impl State {
    pub fn thread(&self) -> &[Entry] {
        &self.thread
    }

    /// Something the user asked waits for the filing module to start.
    pub fn waits_for_hermes(&self) -> bool {
        self.waiting_for_hermes && self.session.holds_something()
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
