use std::path::{Path, PathBuf};
use std::sync::Arc;

use ncrs_ai::ChatTurn;
use ncrs_filing::chat_work::{ChatWork, ReadProgress, RunSummary};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChipReply {
    Cancel,
    Folder(PathBuf),
    PickFolder,
    TurnOn,
    ReadAnyway,
    ShowAll,
    Later,
    /// One of the folders a name the model heard can mean.
    NamedFolder(PathBuf),
    PutBack,
    OpenSearch,
    IntentSearch,
    IntentTidy,
    IntentStructure,
    IntentUndo,
    DownloadModel,
    TurnOnAi,
    /// Reads the folder again that could not be read.
    Retry,
    /// Accepts the levels shown for the folder.
    LevelsKeep,
    /// Starts the change of the levels in words.
    LevelsChange,
    /// Takes the standard levels.
    LevelsDefault,
    /// Takes the levels as changed so far.
    LevelsApply,
    /// Asks for one more change of the levels.
    LevelsMore,
    /// Throws the changes away: the levels found are shown again.
    LevelsDiscard,
    /// The user is done with the levels: they are taken as they are.
    LevelsDone,
    /// Hands the folder and its confirmed levels to the rule suggestions.
    SuggestRules,
    /// Opens the dialog that files this folder, or the active one.
    OpenSort(Option<PathBuf>),
    OpenRuleForm,
    /// Deletes the safe folders of the Platz card, through the Platz tab.
    SpaceDeleteSafe,
    /// Removes the merged worktrees of the Platz card, through the Platz tab.
    SpaceRemoveWorktrees,
    /// Opens the Platz tab.
    SpaceOverview,
    /// Opens the Platz tab on the Downloads folder.
    SpaceShowDownloads,
    /// Opens the Platz tab on the session logs.
    SpaceShowOldLogs,
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
        matches!(
            self,
            Self::TurnOn
                | Self::PickFolder
                | Self::OpenSort(_)
                | Self::OpenRuleForm
                | Self::SpaceDeleteSafe
                | Self::SpaceRemoveWorktrees
                | Self::SpaceOverview
                | Self::SpaceShowDownloads
                | Self::SpaceShowOldLogs
        )
    }

    /// Whether handling the reply writes files, trash, rules or settings. A
    /// typed answer never runs such a reply on its own.
    pub fn changes_something(&self) -> bool {
        match self {
            Self::PutBack | Self::SpaceDeleteSafe | Self::SpaceRemoveWorktrees => true,
            Self::Cancel
            | Self::Folder(_)
            | Self::PickFolder
            | Self::TurnOn
            | Self::ReadAnyway
            | Self::ShowAll
            | Self::Later
            | Self::NamedFolder(_)
            | Self::OpenSearch
            | Self::IntentSearch
            | Self::IntentTidy
            | Self::IntentStructure
            | Self::IntentUndo
            | Self::DownloadModel
            | Self::TurnOnAi
            | Self::Retry
            | Self::LevelsKeep
            | Self::LevelsChange
            | Self::LevelsDefault
            | Self::LevelsApply
            | Self::LevelsMore
            | Self::LevelsDiscard
            | Self::LevelsDone
            | Self::SuggestRules
            | Self::OpenSort(_)
            | Self::OpenRuleForm
            | Self::SpaceOverview
            | Self::SpaceShowDownloads
            | Self::SpaceShowOldLogs => false,
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
    Areas(AreasCard),
    Folder(FolderMention),
    Space(crate::space_card::SpaceCard),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Hermes { parts: Vec<Part>, chips: Vec<Chip> },
    User(String),
    UserFolder(FolderMention),
    Working(String),
}

/// What the next answer is for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Step {
    #[default]
    Idle,
    /// A folder to read, to learn its order.
    ReadFolder,
    /// The opening question: any folder.
    Greeting,
    ConfirmUnknown,
    UndoAsk,
    /// Typed words change the levels shown.
    RuleLevels,
}

/// A read that failed because the folder was not there, to run again.
#[derive(Debug, Clone)]
pub struct Retry {
    pub folder: PathBuf,
    /// The step a typed path failed to resolve in: the retry resolves and
    /// confirms it again instead of reading the path as typed.
    pub unresolved_in: Option<Step>,
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
    /// Leaving the conversation drops it, and the answer of the model that
    /// is still being waited for then never arrives.
    pub discard_answer_on_drop: Option<iced::task::Handle>,
}

#[derive(Debug, Default)]
pub struct Session {
    /// The folder the running read is for.
    pub requested: PathBuf,
    pub retry: Option<Retry>,
    pub last_run: Option<RunSummary>,
    /// A typed folder waiting for a yes.
    pub confirm: Option<PathBuf>,
    /// The name the user is asked to choose a folder for.
    pub choosing: Option<crate::named_folders::Naming>,
    pub unsent: Option<Unsent>,
    /// A button or picked folder that waits for Hermes to start.
    pub unsent_reply: Option<ChipReply>,
    /// How far the running read is.
    pub progress: Option<Arc<ReadProgress>>,
    /// The folder whose order is being recognised.
    pub rule: Option<crate::rule_flow::RuleFlow>,
    /// The Platz measure the app was asked for.
    pub measuring_space: bool,
}

/// What the last measure left for the user to decide, for the answer after a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceLeft {
    pub untouched: Vec<(String, u64)>,
    pub has_downloads: bool,
    pub has_session_logs: bool,
    pub has_worktrees: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceMeasured {
    pub free: u64,
    pub at: std::time::SystemTime,
}

#[derive(Debug, Default)]
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
    /// What the model is asked about levels.
    pub rule_ask: Option<crate::rule_flow::RuleAsk>,
    /// The ask set during this `update` that no task runs yet.
    pub fresh_rule_ask: bool,
    /// The folders the model is asked to describe for the Platz card, until
    /// its answer is in.
    pub describing: Option<Vec<ncrs_ai::FolderFacts>>,
    /// The ask set during this `update` that no task runs yet.
    pub fresh_describe: bool,
    /// A typed folder being checked off the UI thread, and the step it was
    /// typed in.
    pub resolving: Option<(PathBuf, Step)>,
    /// What was typed while `resolving` checks a path in a step that asks
    /// for no folder: it goes to the model if the path is none.
    pub unresolved_sentence: Option<(String, Asked)>,
    /// The check set during this `update` that no task runs yet.
    pub fresh_resolve: bool,
    /// The index was replaced while a turn ran: the chat starts over once it is idle.
    pub restart_when_idle: bool,
    /// The folder a sentence named, being checked off the UI thread.
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
    pub session: Session,
    /// What the last measure for the card found, for the status bar.
    pub space_measured: Option<SpaceMeasured>,
    pub space_left: Option<SpaceLeft>,
    /// The chats of the list; the conversation above is the one of
    /// `chats.live()`.
    pub chats: crate::chat_list::ChatList,
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

    /// What the model is asked about levels; the app asks it on a blocking
    /// thread.
    pub fn pending_rule_ask(&self) -> Option<&crate::rule_flow::RuleAsk> {
        self.rule_ask.as_ref()
    }

    /// The folders the model is asked to describe for the Platz card; the chat
    /// runs this itself through `ctx.model`.
    pub fn pending_describe(&self) -> Option<&[ncrs_ai::FolderFacts]> {
        self.describing.as_deref()
    }

    /// The typed folder the app waits to have checked, and its step.
    pub fn pending_resolve(&self) -> Option<(&Path, Step)> {
        self.resolving
            .as_ref()
            .map(|(typed, step)| (typed.as_path(), *step))
    }

    /// The name the app waits to have checked.
    pub fn pending_naming(&self) -> Option<&crate::named_folders::Naming> {
        self.naming.as_ref()
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
