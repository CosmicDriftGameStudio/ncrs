use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ncrs_ai::{ChatIntent, ChatTurn, Schedule};
use ncrs_filing::chat_work::{
    AnsweredGroups, ChatWork, FolderReading, PatternLevel, ReadProgress, RunPlan, RunSummary,
    TrashItem, TrialPlan, WaitingGroup,
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
    TurnOn,
    ReadAnyway,
    KeepYes,
    ShowAll,
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
    PhotosYes,
    PhotosNo,
    /// Opens the picker for a new folder to file into.
    NewTarget,
    DecidePerFile,
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
    OtherTarget,
    SwitchToCopy,
    SwitchToMove,
    /// Reads the folder again that could not be read.
    Retry,
    /// Moves the files that are in the filing folder already to the trash.
    TrashDuplicatesNow,
    /// Lists the files of the trial that stay.
    ShowStaying,
    /// Runs the trial again against a folder the user picks.
    TrialOtherTarget,
    /// Goes through the documents that wait, group by group.
    WaitingShow,
    /// Files the group of waiting documents in question where it is proposed.
    WaitingFile,
    /// Files the waiting document in question in this folder below the target.
    WaitingFileInto(PathBuf),
    /// Opens the picker for the folder the waiting document in question goes to.
    WaitingOtherFolder,
    /// Accepts the levels shown for the folder.
    LevelsKeep,
    /// Starts the change of the levels in words.
    LevelsChange,
    /// Takes the standard levels.
    LevelsDefault,
    /// Takes the levels as changed so far: the rule's pattern.
    LevelsApply,
    /// Asks for one more change of the levels.
    LevelsMore,
    /// Throws the changes away: the levels found are shown again.
    LevelsDiscard,
    /// The user is done with the levels: they are taken as they are.
    LevelsDone,
    /// Describes what goes into the folder, in these words.
    RuleWhat(String),
    /// Stores the proposed rule.
    RuleCreate,
    /// Asks for the condition in a sentence; the levels stay.
    RuleCondition,
    /// Files what lies differently with the new rule.
    RuleNow,
    /// Leaves what exists as it is: the new rule is for new documents only.
    RuleOnlyNew,
    /// Leaves the group of waiting documents in question as it is.
    WaitingSkip,
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
        matches!(self, Self::TurnOn | Self::PickFolder)
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
            | Self::WaitingFile
            | Self::WaitingFileInto(_)
            | Self::RuleCreate
            | Self::RuleNow
            | Self::Undo => true,
            Self::Cancel
            | Self::Folder(_)
            | Self::PickFolder
            | Self::TurnOn
            | Self::ReadAnyway
            | Self::ShowAll
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
            | Self::OtherTarget
            | Self::SwitchToCopy
            | Self::SwitchToMove
            | Self::Retry
            | Self::ShowStaying
            | Self::WaitingShow
            | Self::WaitingSkip
            | Self::LevelsKeep
            | Self::LevelsChange
            | Self::LevelsDefault
            | Self::LevelsApply
            | Self::LevelsMore
            | Self::LevelsDiscard
            | Self::LevelsDone
            | Self::RuleWhat(_)
            | Self::RuleCondition
            | Self::RuleOnlyNew
            | Self::WaitingOtherFolder
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

/// The levels of a filing pattern with a real value for each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelsCard {
    pub title: String,
    pub rows: Vec<ncrs_ui_kit::hermes_kit::LevelRow>,
    /// The files the changed levels would rename or move.
    pub moves: Option<ncrs_ui_kit::hermes_kit::MovesBlock>,
    pub footer: Option<String>,
}

/// A rule proposal: what it applies to, where it files, some examples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalCard {
    pub title: String,
    pub facts: Vec<ncrs_ui_kit::hermes_kit::FactRow>,
    pub moves: Option<ncrs_ui_kit::hermes_kit::MovesBlock>,
    pub counts: Option<String>,
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
    Levels(LevelsCard),
    Proposal(ProposalCard),
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
    /// Typed words change the levels shown.
    RuleLevels,
    /// Typed words say what goes into the folder.
    RuleWhat,
    /// The next picked folder is where the unsure waiting document goes.
    WaitingFolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadFor {
    Learn,
    /// The folder is opened for its levels.
    Open,
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
    /// Started by "Rearrange now": target and transfer are given and the run
    /// starts without a question. Holds the rule's folder below the target;
    /// only documents filed there are filed.
    pub rule_now: Option<PathBuf>,
    pub source: PathBuf,
    pub target: Option<PathBuf>,
    pub transfer: Option<Transfer>,
    /// Archive documents the stored rule renames or moves with this run.
    pub inside: Vec<ncrs_filing::rules_work::ArchiveMove>,
}

/// What a rule would move: the documents of the inbox and, because the
/// levels may have changed, the ones that already lie in the archive.
#[derive(Debug, Clone, Default)]
pub struct RuleMoves {
    /// The target the rule files into.
    pub target: PathBuf,
    /// The rule's folder below the target.
    pub prefix: PathBuf,
    pub from_inbox: usize,
    pub archive: Vec<ncrs_filing::rules_work::ArchiveMove>,
}

impl RuleMoves {
    pub fn total(&self) -> usize {
        self.from_inbox + self.archive.len()
    }
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

/// The walk through the waiting documents: the groups still to ask about, the
/// one asked now, and how many documents there were and were filed.
#[derive(Debug)]
pub struct Walk {
    pub groups: std::collections::VecDeque<WaitingGroup>,
    pub current: Option<WaitingGroup>,
    pub total: usize,
    pub filed: usize,
}

#[derive(Debug, Default)]
pub struct Session {
    pub waiting: Option<Walk>,
    /// The greeting follows the count of the waiting documents.
    pub greeting_after_count: bool,
    /// How many waiting documents the user was told about last.
    pub waiting_offered: usize,
    pub last_check: Option<chrono::DateTime<chrono::Local>>,
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
    /// The last tidy or structure change Hermes understood.
    pub previous_request: Option<ChatIntent>,
    /// The sentence before, when Hermes could do nothing with it.
    pub previous_message: Option<String>,
    pub unsent: Option<Unsent>,
    /// A button or picked folder that waits for Hermes to start.
    pub unsent_reply: Option<ChipReply>,
    /// How far the running read is.
    pub progress: Option<Arc<ReadProgress>>,
    /// The duplicates of the folder learned first, offered after the first
    /// filing, once.
    pub archive_duplicates: Option<Offer>,
    pub archive_offered: bool,
    /// The rule being made from a folder or a description.
    pub rule: Option<crate::rule_flow::RuleFlow>,
    /// What the stored rule moves, kept for "File now".
    pub rule_moves: Option<RuleMoves>,
    /// The sources "Rearrange now" still has to go through, one run each.
    pub rule_queue: Option<RuleQueue>,
}

#[derive(Debug, Clone)]
pub struct RuleQueue {
    pub target: PathBuf,
    pub prefix: PathBuf,
    pub sources: Vec<PathBuf>,
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
    /// What the model is asked about a rule or levels.
    pub rule_ask: Option<crate::rule_flow::RuleAsk>,
    /// The ask set during this `update` that no task runs yet.
    pub fresh_rule_ask: bool,
    /// A typed folder being checked off the UI thread, and the step it was
    /// typed in.
    pub resolving: Option<(PathBuf, Step)>,
    /// What was typed while `resolving` checks a path in a step that asks
    /// for no folder: it goes to the model if the path is none.
    pub unresolved_sentence: Option<(String, Asked)>,
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
            rule_ask: None,
            fresh_rule_ask: false,
            resolving: None,
            unresolved_sentence: None,
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

    /// What the model is asked about a rule; the app asks it on a blocking
    /// thread.
    pub fn pending_rule_ask(&self) -> Option<&crate::rule_flow::RuleAsk> {
        self.rule_ask.as_ref()
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

    /// Whether the newest entry is Hermes', so its chips are the ones on screen.
    pub fn chips_owner_is_hermes(&self) -> bool {
        matches!(self.thread.last(), Some(Entry::Hermes { .. }))
    }

    /// The chips of the newest entry, when it is Hermes'.
    pub fn chips(&self) -> &[Chip] {
        match self.thread.last() {
            Some(Entry::Hermes { chips, .. }) => chips,
            _ => &[],
        }
    }
}
