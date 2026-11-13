//! The messages of the Hermes pages. A view sends [`Message`]; the app passes
//! each variant on as its own message of the same name.

use std::path::PathBuf;

use ncrs_config::config::{Filing, FilingWriteError};
use ncrs_config::file_list_tags::{TagExceptions, TagGroup};
use ncrs_core::PanelSide;
use ncrs_filing::filing::FilingMessage;
use ncrs_filing::filing_work::FilingSummary;
use ncrs_filing::query_work::{ExportSummary, QueryHit};
use ncrs_filing::review_work::{AcceptSummary, ReviewItem};
use ncrs_filing::rules_work::{RulesLoad, TryResult};
use ncrs_filing::tags_work::{TagsFailure, TagsLoaded};
use ncrs_index::{HermesCounts, IncludeOutcome, RuleId, SavedQuery, SavedQueryId};
use ncrs_pipeline::Load;

use crate::condition_editor::ConditionEdit;
use crate::filing_pieces::TargetEdit;
use crate::hermes_column::{ColumnReading, ColumnStage, FileDetail, FolderRole};

/// What the views of the Hermes pages send.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Filing(FilingMessage),
    FilingStatus(FilingStatusMessage),
    HermesOverview(HermesOverviewMessage),
    Rules(RulesMessage),
    QuickFix(QuickFixMessage),
    SavedQueries(SavedQueriesMessage),
    Tags(TagsMessage),
    /// The filing job on its blocking thread is done.
    DocumentsFiled(Result<FilingSummary, String>),
    HermesColumn(HermesColumnMessage),
    HermesFile(HermesFileMessage),
    /// The tags page offers to show the tags in the file list: the settings
    /// switch of the same name.
    ToggleFileListTags,
}

/// Everything the filing status page sends.
#[derive(Debug, Clone, PartialEq)]
pub enum FilingStatusMessage {
    /// Shows the page with the files that could not be read.
    Open,
    /// The once-a-second look at the pipeline.
    Tick,
    /// The module and runner of the previous settings are gone; the one of
    /// the generation starts now.
    Restarted(u64),
    TogglePause,
    /// The counts of Hermes, read in the background.
    HermesCounts(Result<HermesCounts, String>),
    /// The "Zeigen" of an error row: show its file in the panel.
    ErrorActivated(PathBuf),
    /// The button of one error row: that file is read again.
    ProcessOneAgain(PathBuf),
    /// The button under the list: every file of the list is read again.
    ProcessAllAgain,
}

/// What the background reads for the Hermes column answer with.
#[derive(Debug, Clone, PartialEq)]
pub enum HermesColumnMessage {
    /// One stage of the reading for `directory` of a panel.
    Read {
        side: PanelSide,
        directory: PathBuf,
        stage: ColumnStage,
        result: Result<ColumnReading, String>,
    },
    /// What the review list knows about the file under the cursor.
    Detail {
        path: PathBuf,
        result: Result<Option<FileDetail>, String>,
    },
    /// Whether a file read again that finished now waits in the review list.
    ReadAgainEnded {
        path: PathBuf,
        result: Result<bool, String>,
    },
    /// Hermes was told to leave these paths alone.
    Excluded {
        paths: Vec<PathBuf>,
        result: Result<(), String>,
    },
}

/// What the buttons for the protection of the index send.
#[derive(Debug, Clone, PartialEq)]
pub enum HermesOverviewMessage {
    SetLoad(Load),
    EncryptNow,
    TryAgain,
    EnterRecoveryKey,
    /// Opens the question whether to remove the index and begin again.
    AskStartOver,
    ConfirmStartOver,
    CancelStartOver,
}

/// The pages of the settings that the Hermes crate fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HermesPage {
    Rules,
    SavedQueries,
    Tags,
    Inspection,
}

/// Everything the saved queries page sends.
#[derive(Debug, Clone, PartialEq)]
pub enum SavedQueriesMessage {
    /// The work of the generation is done, with the saved queries. Dropped
    /// when the page was closed or opened again meanwhile.
    Loaded {
        generation: u64,
        result: Result<Vec<SavedQuery>, String>,
    },
    /// The hits of every saved query, for the list.
    Counted {
        generation: u64,
        result: Result<Vec<(SavedQueryId, usize)>, String>,
    },
    /// "Ändern" on the report at this position: the dialog opens.
    Edit(usize),
    /// "+ Report": the dialog of a new report opens.
    New,
    /// An entry of the template list of the new report (0: none).
    Template(usize),
    /// The search opens on the criteria of the report, to change that one.
    EditInSearch,
    NameChanged(String),
    /// Enter in the name field or the button: saves the name and criteria.
    Save,
    Saved {
        generation: u64,
        result: Result<SavedQueryId, String>,
    },
    /// "Entfernen" on the report at this position: the question opens.
    AskDelete(usize),
    /// Closes the dialog that is up; a search that is not saved is dropped.
    CancelStep,
    ConfirmDelete,
    Deleted {
        generation: u64,
        result: Result<SavedQueryId, String>,
    },
    /// Asks the index for the hits of the selected report.
    Ran {
        generation: u64,
        serial: u64,
        result: Result<Vec<QueryHit>, String>,
    },
    /// Lists the files that are gone, or the first hits again.
    ToggleMissing,
    /// "Exportieren" on the report at this position: it runs, then the
    /// dialog asks where to and shows what would be copied.
    StartExport(usize),
    /// Opens the system folder dialog for the target folder.
    PickFolder,
    FolderPicked(Option<PathBuf>),
    /// Enter in the export step: copies.
    Export,
    Exported {
        generation: u64,
        result: Result<ExportSummary, String>,
    },
}

/// Everything the tags page sends.
#[derive(Debug, Clone, PartialEq)]
pub enum TagsMessage {
    /// The work of the generation is done, with what the page shows now.
    /// Dropped when the page was closed or opened again meanwhile.
    Done {
        generation: u64,
        result: Result<TagsLoaded, TagsFailure>,
    },
    /// The tags the file list shows against their group, read when the index
    /// opens.
    ExceptionsRead(Result<TagExceptions, String>),
    /// Folds a group shut or open.
    ToggleOpen(TagGroup),
    /// The link behind a long group.
    Unfold(TagGroup),
    ToggleTag {
        group: TagGroup,
        tag: String,
    },
    ToggleGroup(TagGroup),
    SearchChanged(String),
    StartRename(String),
    RenameChanged(String),
    /// Enter in the field of the new name, or the button of the dialog.
    ConfirmRename,
    AskDelete(String),
    ConfirmDelete,
    /// Closes the dialog that is up.
    CancelStep,
}

/// What the Hermes commands do with the file under the cursor of the active
/// panel.
#[derive(Debug, Clone, PartialEq)]
pub enum HermesFileMessage {
    HandToInbox,
    ProcessAgain,
    SetRole(FolderRole),
    /// Excludes the file, or includes it again when it is excluded.
    ToggleExclusion,
    ShowInInspection,
    /// The configured locations after a role was set, as written.
    RoleSaved(Result<Filing, FilingWriteError>),
    /// The files "process again" stands for: folders resolved to their
    /// documents, excluded ones left out.
    ProcessAgainFound(Result<Vec<PathBuf>, String>),
    Included {
        path: PathBuf,
        result: Result<IncludeOutcome, String>,
    },
}

/// Everything the quick-fix menu of an inquiry sends.
#[derive(Debug, Clone, PartialEq)]
pub enum QuickFixMessage {
    /// The key: opens the menu on the cursor row or the marked files.
    Open,
    Close,
    /// The reviews of the generation's files. Dropped when the menu was closed
    /// or opened again meanwhile.
    Loaded {
        generation: u64,
        result: Result<Vec<ReviewItem>, String>,
    },
    Move(isize),
    First,
    Last,
    Hover(usize),
    Activate,
    Click(usize),
    /// The sequential filing of the accepted suggestions is done.
    Filed(AcceptSummary),
}

/// Everything the rules page sends.
#[derive(Debug, Clone, PartialEq)]
pub enum RulesMessage {
    /// The work of the generation is done, with the rules. Dropped when the
    /// page was closed or opened again meanwhile.
    Loaded {
        generation: u64,
        result: Result<RulesLoad, String>,
    },
    /// The switch of the rule at this position.
    ToggleAt(usize),
    /// Moves the rule at this position up (`-1`) or down (`1`) in the order.
    ReorderAt {
        index: usize,
        delta: isize,
    },
    /// "Entfernen" on the rule at this position: the question opens.
    AskDelete(usize),
    CancelDelete,
    ConfirmDelete,
    Changed {
        generation: u64,
        result: Result<(), String>,
    },
    /// Opens the form for a new rule.
    New,
    /// "Ändern" on the rule at this position: its dialog opens.
    Edit(usize),
    Form(RuleFormMessage),
    /// The form has rested for the debounce time since the edit of this
    /// try generation.
    TryElapsed(u64),
    /// The try of the generation is done. Dropped when the form was edited
    /// meanwhile.
    Tried {
        generation: u64,
        result: Result<TryResult, String>,
    },
}

/// Everything the form of a rule sends.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleFormMessage {
    Name(String),
    Condition(ConditionEdit),
    Target(TargetEdit),
    TagInput(String),
    /// The link that offers a tag of its own.
    OpenTagInput,
    AddTag,
    RemoveTag(String),
    Save,
    Cancel,
    /// The work of the generation is done. Dropped when the page was closed
    /// or opened again meanwhile.
    Saved {
        generation: u64,
        result: Result<RuleId, String>,
    },
}
