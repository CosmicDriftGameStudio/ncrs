//! The messages of the Hermes pages. A view sends [`Message`]; the app passes
//! each variant on as its own message of the same name.

use std::path::PathBuf;

use ncrs_config::config::{Config, Filing, FilingWriteError};
use ncrs_config::file_list_tags::{TagExceptions, TagGroup};
use ncrs_core::PanelSide;
use ncrs_filing::filing::{FilingMessage, IndexFigures};
use ncrs_filing::query_work::{ExportSummary, QueryHit};
use ncrs_filing::reorganisation_work::PlannedItem;
use ncrs_filing::replan_work::ReplanItem;
use ncrs_filing::review_work::{AcceptSummary, ConfirmOutcome, ReviewItem};
use ncrs_filing::rules_work::{RulesLoad, TryResult};
use ncrs_filing::suggestion_work::SuggestedRule;
use ncrs_filing::tags_work::{TagsFailure, TagsLoaded};
use ncrs_index::{HermesCounts, IncludeOutcome, QuerySpec, RuleId, SavedQuery, SavedQueryId};
use ncrs_intake::LocationMode;
use ncrs_pipeline::Load;

use crate::condition_editor::ConditionEdit;
use crate::document_preview::PreviewContent;
use crate::filing_pieces::TargetEdit;
use crate::hermes_column::{ColumnReading, ColumnStage, FileDetail, FolderRole};
use crate::hermes_setup::FolderSlot;
use crate::review_list::FormField;

/// What the views of the Hermes pages send.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Filing(FilingMessage),
    FilingStatus(FilingStatusMessage),
    FilingSetup(FilingSetupMessage),
    HermesSetup(HermesSetupMessage),
    HermesOverview(HermesOverviewMessage),
    RuleSuggestions(RuleSuggestionsMessage),
    Rules(RulesMessage),
    ReorganisationPlan(ReorganisationPlanMessage),
    ReviewList(ReviewListMessage),
    QuickFix(QuickFixMessage),
    SavedQueries(SavedQueriesMessage),
    Tags(TagsMessage),
    HermesPage(HermesPage),
    /// Runs the entries of the reorganisation plan that are checked.
    ExecuteReorganisation(Vec<PlannedItem>),
    /// The tags page offers to show the tags in the file list: the settings
    /// switch of the same name.
    ToggleFileListTags,
}

/// Everything the filing status page sends.
#[derive(Debug, Clone, PartialEq)]
pub enum FilingStatusMessage {
    Open,
    Close,
    Toggle,
    /// The once-a-second look at the pipeline.
    Tick,
    /// The module and runner of the previous settings are gone; the one of
    /// the generation starts now.
    Restarted(u64),
    TogglePause,
    /// One worker more (`1`) or fewer (`-1`) than now.
    StepWorkers(i8),
    IndexFigures(Result<IndexFigures, String>),
    /// The counts of Hermes, read in the background.
    HermesCounts(Result<HermesCounts, String>),
    /// The link under the selected error: show its file in the panel.
    ErrorActivated(PathBuf),
    MoveError(i32),
    /// A click on the row at this position of the list as shown, newest first.
    SelectError(usize),
    /// The box of an error file, or Space on the selected one.
    ToggleErrorChecked(PathBuf),
    ToggleSelectedErrorChecked,
    /// Enter and the button under the page: the checked files, or the selected
    /// one when none is checked, are read again.
    ProcessAgain,
    /// Shows or hides the tables of the steps and of the workers.
    ToggleSteps,
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

/// Everything the filing setup sends.
#[derive(Debug, Clone, PartialEq)]
pub enum FilingSetupMessage {
    Toggle,
    Close,
    /// Open the system folder dialog to add a folder of this mode.
    PickFolder(LocationMode),
    /// What the dialog returned; `None` when it was cancelled or could not open.
    FolderPicked {
        mode: LocationMode,
        folder: Option<PathBuf>,
    },
    /// Add the active panel's folder: the fallback for keyboard use and for
    /// systems without a dialog.
    AddPanelFolder(LocationMode),
    /// The folder of a pick or a panel add, looked up on disk: its canonical
    /// path, or why it cannot be used.
    Added {
        mode: LocationMode,
        folder: PathBuf,
        resolved: Result<PathBuf, String>,
    },
    /// Make the document folder at this index of the chosen list the
    /// filing location.
    SetTarget(usize),
    ToggleImages(usize),
    Remove(usize),
    Save,
    /// The filing settings read back from the written file.
    Saved(Result<Filing, FilingWriteError>),
    /// The access to a picked folder was kept, or why not (sandbox only).
    Granted {
        folder: PathBuf,
        result: Result<(), String>,
    },
}

/// Everything the first-time setup of Hermes sends.
#[derive(Debug, Clone, PartialEq)]
pub enum HermesSetupMessage {
    /// Open the system folder dialog for one of the two folders.
    PickFolder(FolderSlot),
    /// What the dialog returned; `None` when it was cancelled or could not open.
    FolderPicked {
        slot: FolderSlot,
        folder: Option<PathBuf>,
    },
    /// The folder picked, looked up on disk: its canonical path, or why it
    /// cannot be used.
    Resolved {
        slot: FolderSlot,
        folder: PathBuf,
        resolved: Result<PathBuf, String>,
    },
    ChooseEncrypted(bool),
    ToggleLoadModel,
    Start,
    /// The filing settings read back from the written file.
    Written(Result<Filing, FilingWriteError>),
    Later,
    /// The access to a picked folder was kept, or why not (sandbox only).
    Granted {
        folder: PathBuf,
        result: Result<(), String>,
    },
}

/// Everything the Hermes overview sends.
#[derive(Debug, Clone, PartialEq)]
pub enum HermesOverviewMessage {
    Close,
    /// An edit of the default target; it is written at once.
    TargetEdited(TargetEdit),
    PatternWritten(Result<Box<Config>, FilingWriteError>),
    SetAiEnabled(bool),
    SetLoad(Load),
    /// The user wants other folders: the old card, or the setup without any.
    ChangeFolders,
    LoadOcrModels,
    LoadAiModel,
    ShowOwnServerSettings,
    /// Opens the settings of filing at the protection of the index.
    ShowProtectionSettings,
    /// Opens the review list, when there is something to review.
    ShowReviews,
    SwitchHermesOn,
    ContinueSetup,
    TryAgain,
    EnterRecoveryKey,
    EncryptNow,
    /// Opens the question whether to remove the index and begin again.
    AskStartOver,
    ConfirmStartOver,
    CancelStartOver,
}

/// Everything the reorganisation plan view sends. Running the selection is
/// `Message::ExecuteReorganisation`, which carries the entries.
#[derive(Debug, Clone, PartialEq)]
pub enum ReorganisationPlanMessage {
    Open,
    Close,
    /// Works the plan out again.
    Replan,
    /// The work of the generation is done. Dropped when the view was closed or
    /// opened again meanwhile.
    Loaded {
        generation: u64,
        result: Result<Vec<PlannedItem>, String>,
    },
    /// The wheel or trackpad over the list.
    Scrolled(iced::mouse::ScrollDelta),
    Move(isize),
    /// A click on the row at this position: the cursor goes there.
    Select(usize),
    /// A click on the checkbox of the line at this position: an entry or a
    /// group heading.
    Toggle(usize),
    ToggleCursor,
    /// Enter: hands the selection over as `Message::ExecuteReorganisation`.
    Execute,
    /// The link under the list: opens the page of the rule behind the entry
    /// under the cursor.
    OpenRule(RuleId),
}

/// Which part of the Hermes card the arrow keys act on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum HermesFocus {
    /// Up and down switch the page.
    Sidebar,
    /// Up and down move in the page's list.
    #[default]
    Content,
}

/// The pages of the Hermes card, in the order of its sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HermesPage {
    Overview,
    Review,
    Rules,
    RuleSuggestions,
    Reorganisation,
    SavedQueries,
    Tags,
    Inspection,
}

/// Everything the saved queries page sends.
#[derive(Debug, Clone, PartialEq)]
pub enum SavedQueriesMessage {
    Open,
    /// Escape backs out of the step that is up first, then leaves the page.
    Close,
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
    /// The wheel or trackpad over the list.
    Scrolled(iced::mouse::ScrollDelta),
    Move(isize),
    /// A click on the query at this position.
    Select(usize),
    /// An entry of "New": the search opens with these criteria.
    NewSearch(QuerySpec),
    /// The search opens on the criteria of the report, to change that one.
    EditInSearch,
    /// The search opens on the criteria of the report, as a search of its own.
    OpenInSearch,
    NameChanged(String),
    /// Enter in the name field or the button: saves the name and criteria.
    Save,
    Saved {
        generation: u64,
        result: Result<SavedQueryId, String>,
    },
    /// Enter: answers the step that is up, or else does what the page offers
    /// first: saves the unsaved search, exports the hits.
    Confirm,
    /// The first step of deleting: the question in the page.
    AskDelete,
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
    /// The first step of exporting: the page asks where to and shows what
    /// would be copied.
    StartExport,
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
    /// Escape backs out of the step that is up first, then leaves the page.
    Close,
    /// The work of the generation is done, with what the page shows now.
    /// Dropped when the page was closed or opened again meanwhile.
    Done {
        generation: u64,
        result: Result<TagsLoaded, TagsFailure>,
    },
    /// The tags the file list shows against their group, read when the index
    /// opens.
    ExceptionsRead(Result<TagExceptions, String>),
    Scrolled(iced::mouse::ScrollDelta),
    Move(isize),
    /// A click on the row at this position.
    Select(usize),
    /// Space: switches the tag or the group under the cursor, or unfolds the
    /// rest of a long group.
    ToggleSelected,
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
    /// Enter in the field of the new name.
    ConfirmRename,
    AskDelete(String),
    /// Enter: answers the delete question, or else switches the row under
    /// the cursor.
    ConfirmDelete,
}

/// What the Hermes commands do with the file under the cursor of the active
/// panel.
#[derive(Debug, Clone, PartialEq)]
pub enum HermesFileMessage {
    HandToInbox,
    ProcessAgain,
    /// Plans the documents of the folder again and shows the preview.
    ReprocessFolder,
    SetRole(FolderRole),
    /// Excludes the file, or includes it again when it is excluded.
    ToggleExclusion,
    ShowInInspection,
    /// The configured locations after a role was set, as written.
    RoleSaved(Result<Filing, FilingWriteError>),
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

/// Everything the review list sends.
#[derive(Debug, Clone, PartialEq)]
pub enum ReviewListMessage {
    Open,
    /// Escape leaves the form first, the list second.
    Close,
    /// The work of the generation is done, with the open entries. Dropped when
    /// the list was closed or opened again meanwhile.
    Loaded {
        generation: u64,
        result: Result<Vec<ReviewItem>, String>,
    },
    /// The wheel or trackpad over the list.
    Scrolled(iced::mouse::ScrollDelta),
    Move(isize),
    /// A click on the entry at this position.
    Select(usize),
    Edit,
    FieldChanged(FormField, String),
    /// F3 or the thumbnail: the page of the document instead of the fields.
    TogglePreview,
    /// "Add tag": the field for a new tag shows and takes the focus.
    OpenTagInput,
    TargetEdited(TargetEdit),
    /// Enter in the tag field: the typed tag joins the user's own.
    AddTag,
    RemoveTag(String),
    /// Tab (`false`) or Shift+Tab (`true`) in the form.
    FocusField {
        backwards: bool,
    },
    /// Enter or the button: files the document with the fields as shown.
    Confirm,
    Confirmed {
        generation: u64,
        result: Result<ConfirmOutcome, String>,
    },
    /// The AI can read now: every open entry goes through Hermes again.
    ReadAgainWithAi,
    /// The offer after filing: opens the rule form for this sender.
    CreateRule,
    /// The button or its key: plans the open cases again, as a preview.
    ReplanOpenCases,
    /// The work of the generation planned the documents again, without
    /// changing anything. Dropped when the list was closed or opened again.
    Previewed {
        generation: u64,
        serial: u64,
        result: Result<Vec<ReplanItem>, String>,
    },
    /// Files the previewed documents.
    ReplanConfirm,
    /// Leaves the preview; nothing was filed.
    ReplanCancel,
    /// The preview of a document is ready. Dropped when the list was closed
    /// or opened again, or the cursor has moved on since.
    PreviewLoaded {
        generation: u64,
        serial: u64,
        content: PreviewContent,
    },
}

/// Everything the rules page sends.
#[derive(Debug, Clone, PartialEq)]
pub enum RulesMessage {
    Open,
    /// Escape backs out of the delete question first, then leaves the page.
    Close,
    /// The work of the generation is done, with the rules. Dropped when the
    /// page was closed or opened again meanwhile.
    Loaded {
        generation: u64,
        result: Result<RulesLoad, String>,
    },
    Move(isize),
    /// The wheel or trackpad over the list.
    Scrolled(iced::mouse::ScrollDelta),
    /// A click on the rule at this position.
    Select(usize),
    /// Space: switches the rule under the cursor on or off.
    Toggle,
    /// A click on the switch of the rule at this position.
    ToggleAt(usize),
    /// Moves the rule under the cursor up (`-1`) or down (`1`) in the order.
    Reorder(isize),
    AskDelete,
    CancelDelete,
    ConfirmDelete,
    Changed {
        generation: u64,
        result: Result<(), String>,
    },
    /// Opens the form for a new rule.
    New,
    /// Opens the form for the rule under the cursor.
    Edit,
    Form(RuleFormMessage),
    /// Shows the documents of the try instead of the form, or the form again.
    ToggleTryList,
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

/// Everything the rule suggestions view sends.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleSuggestionsMessage {
    Open,
    Close,
    /// Learns the archive again and refreshes the list.
    Reanalyse,
    /// The work of the generation is done, with the open suggestions. Dropped
    /// when the view was closed or the analysis started again meanwhile.
    Loaded {
        generation: u64,
        result: Result<Vec<SuggestedRule>, String>,
    },
    /// The wheel or trackpad over the list.
    Scrolled(iced::mouse::ScrollDelta),
    Move(isize),
    /// A click on the suggestion at this position.
    Select(usize),
    /// Opens the rule form for the selected suggestion.
    Edit,
    /// Enter or the button: confirms the suggestion as it is.
    Confirm,
    Discard,
    /// The documents a suggestion's condition finds.
    MatchesFound {
        id: RuleId,
        result: Result<Vec<PathBuf>, String>,
    },
}
