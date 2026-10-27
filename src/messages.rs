//! Central message type. Every state change in the app is triggered by one
//! of these messages and handled in [`crate::app::App::update`].

use std::path::PathBuf;

use ncrs_index::{HermesCounts, IncludeOutcome, RuleId, SavedQuery, SavedQueryId, TaxContext};
use ncrs_intake::LocationMode;
use ncrs_pipeline::Load;

use iced::Size;
use serde::{Deserialize, Serialize};

use crate::ai_model::DownloadError as AiDownloadError;
use crate::app::{RowFailure, Transfer};
use crate::command_line::ShellOutcome;
use crate::config::{
    AiProviderChoice, AiProviderName, Config, Filing, FilingWriteError, ProgramLine,
};
use crate::dialog::PromptKind;
use crate::execute::UndoReport;
use crate::filing::{FilingMessage, IndexFigures};
use crate::filing_work::FilingSummary;
use crate::fs::search::SearchEvent;
use crate::fs::{CreateDirError, OpenKind};
use crate::fs::{Destination, FileEntry, PathProblem, ReadError, SortColumn, SortKey};
use crate::hermes_column::{ColumnReading, ColumnStage, FileDetail, FolderRole};
use crate::i18n::Language;
use crate::jobs::JobEvent;
use crate::keymap::Binding;
#[cfg(any(target_os = "macos", test))]
use crate::keymap::KeyAction;
use crate::ocr_models::DownloadError;
use crate::open_vsx::FetchError;
use crate::palette::{ColorRole, ThemePreset};
use crate::query_work::{ExportSummary, QueryHit};
use crate::reorganisation_work::{ExecutionSummary, PlannedItem};
use crate::review_list::FormField;
use crate::review_work::{AcceptSummary, ConfirmOutcome, ReviewItem};
use crate::saved_queries::QueryField;
use crate::suggestion_work::SuggestedRule;
use crate::theme_fetch::{Fetched, Installed};
use crate::transfer_dialog::TransferDialogMessage;

/// Identifies one of the two file panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelSide {
    Left,
    Right,
}

impl PanelSide {
    pub fn other(self) -> Self {
        match self {
            PanelSide::Left => PanelSide::Right,
            PanelSide::Right => PanelSide::Left,
        }
    }
}

/// PartialEq so bindings can be compared; Eq is not derivable because
/// WindowResized carries floats.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Result of an async directory read.
    DirectoryLoaded {
        side: PanelSide,
        /// Matches `PanelState::request_id`; stale results are dropped.
        request_id: u64,
        path: PathBuf,
        entries: Vec<FileEntry>,
        error: Option<ReadError>,
        /// The order the reader put `entries` in.
        sorted_by: SortKey,
        /// Entry name to select after loading (e.g. the dir we came from).
        select: Option<String>,
        /// The archive file, when `path` lies inside one.
        archive: Option<PathBuf>,
    },

    // --- Navigation inside the active panel ---
    MoveSelection(isize),
    PageUp,
    PageDown,
    SelectFirst,
    SelectLast,
    OpenSelected,
    GoUp,
    /// A printable key: jump to the first entry that starts with what was
    /// typed in the last second.
    TypeAhead(String),

    // --- copy / move (F5, F6) ---
    /// Ctrl+C or Escape while a job runs: stop it. Its own message rather than
    /// reusing a key binding, because "stop" only exists while a job does.
    AbortJob,
    /// Escape with the config error up: take it off the status line.
    DismissConfigError,
    /// One tick of a running transfer, sent from the blocking thread.
    JobProgress(crate::fs::transfer::Tick),
    /// F5 or F6, decided by the action rather than a message each.
    Transfer(TransferKind),
    /// The copy or move dialog, and the progress mask after it.
    TransferDialog(TransferDialogMessage),
    /// The status bar's notice of this generation has been up long enough. An
    /// older timer finds a newer notice and leaves it.
    NoticeExpired(u64),
    /// The debounce of this generation has run out: write the session state,
    /// unless a newer change has scheduled a later write.
    SaveSessionState(u64),
    /// Path or name of the tagged rows, else the cursor row, to the clipboard.
    CopyToClipboard(ClipboardKind),
    /// The user answered the conflict dialog.
    TransferConflict(ConflictChoice),
    /// Ticks or unticks "for all files" in the conflict dialog.
    ToggleConflictAll,
    /// One row of a transfer finished. Carries the whole transfer so the next
    /// row can start without the app keeping it in a field that a later
    /// message could overwrite.
    TransferRowDone {
        /// `Err` carries a typed [`RowFailure`], not a bare string: the app has
        /// to tell "the name is taken" from any other error, and a message that
        /// cannot be told apart is the reason the overwrite dialog was
        /// unreachable.
        result: Result<Transfer, RowFailure>,
        /// Which row this was, so a conflict on the next one points at it.
        index: usize,
        /// The transfer it belongs to; a result from an earlier one is dropped.
        generation: u64,
    },

    /// Left/Right/Tab in a button dialog: move the focus by one.
    DialogFocus(isize),
    /// Enter in a button dialog: press the focused button.
    DialogActivate,
    /// A modifier key went down or up.
    ModifiersChanged(iced::keyboard::Modifiers),

    // --- view / edit (F3, F4) ---
    /// F3 or F4: hand the file under the cursor to an external program.
    OpenExternal(crate::fs::OpenKind),
    /// Click on the "access blocked" error: open the system's privacy settings.
    #[cfg(target_os = "macos")]
    OpenPrivacySettings,

    // --- delete (F8, Shift+F8) ---
    /// F8 (`permanent: false`) or Shift+F8: open the confirmation dialog.
    Delete {
        permanent: bool,
    },
    /// The user confirmed the delete dialog.
    DeleteConfirm,
    /// The user dismissed the delete dialog.
    DeleteCancel,
    /// One entry of a delete finished.
    DeleteRowDone {
        /// The reason, as the OS or the trash worded it; the app adds the name.
        result: Result<(), String>,
        index: usize,
        /// The delete it belongs to; a result from an earlier one is dropped.
        generation: u64,
    },

    // --- drive menu (Alt+F1, Alt+F2) ---
    /// Alt+F1 or Alt+F2: choose the drive of that panel.
    VolumeMenu(PanelSide),
    /// Up or Down in the open menu.
    VolumeMenuMove(isize),
    VolumeMenuFirst,
    VolumeMenuLast,
    /// Enter in the open menu: go to the highlighted drive.
    VolumeMenuActivate,
    /// A click on the entry at this index: highlight it and go there.
    VolumeMenuClick(usize),
    VolumeMenuClose,
    /// Delete or Backspace in the drive menu: take the highlighted favourite
    /// out of the list. A drive row ignores it.
    VolumeMenuRemove,
    /// Alt+Left: the active panel returns to the folder it was in before.
    HistoryBack,
    /// Alt+Right: undo of [`Message::HistoryBack`].
    HistoryForward,
    /// Adds the active panel's folder to the favourites.
    AddFavourite,
    /// Takes the active panel's folder out of the favourites.
    RemoveFavourite,
    /// Opens the drive menu on the first favourite.
    OpenFavourites,

    // --- context menu (right click, Option tap, Shift+F10) ---
    /// Right click (or Ctrl+click on a Mac) on a row: open the menu at the pointer.
    ContextMenuAt {
        side: PanelSide,
        index: usize,
    },
    /// Shift+F10 or a tap of Option: open the menu at the cursor row.
    ContextMenuKey,
    ContextMenuMove(isize),
    ContextMenuFirst,
    ContextMenuLast,
    /// Enter in the open menu: run the highlighted entry.
    ContextMenuActivate,
    /// A click on the entry at this index: run it.
    ContextMenuClick(usize),
    /// The pointer is over the entry at this index.
    ContextMenuHover(usize),
    ContextMenuClose,
    /// Right or Enter on the Hermes entry: open its submenu.
    ContextMenuOpenSub,
    /// Left: back from the submenu to the entry.
    ContextMenuCloseSub,
    /// The pointer is over the entry at this index of the submenu.
    ContextMenuSubHover(usize),
    /// A click on the entry at this index of the submenu: run it.
    ContextMenuSubClick(usize),

    // --- path field in the status bar ---
    /// The path field's text changed: from then on it shows what was typed
    /// instead of the panel's path.
    PathFieldInput(String),
    /// Enter in the field: go to the typed path.
    PathFieldSubmit,
    /// The background look at the path that Enter submitted. A result whose
    /// `request_id` is not the latest is dropped.
    PathChecked {
        request_id: u64,
        side: PanelSide,
        target: PathBuf,
        result: Result<Destination, PathProblem>,
    },
    /// Escape, Tab or a click elsewhere: back to the panel's own path.
    PathFieldCancel,
    /// A mouse press that no widget took.
    ClickedOutside,

    // --- Job queue (see crate::jobs) ---
    /// A job started, made progress, or finished. One variant for all three:
    /// they are the same event arriving at different times, and splitting them
    /// would mean the UI matching on the same enum in three places.
    JobFinished(JobEvent),

    // --- Modal prompt (see crate::dialog) ---
    /// F7: open the create-directory prompt.
    CreateDirPrompt,
    /// Shift+F4: open the new-file prompt.
    CreateFilePrompt,
    /// A keystroke went to the open prompt.
    PromptInput(String),
    /// The user confirmed the prompt.
    PromptSubmit,
    /// The user dismissed the prompt.
    PromptCancel,
    /// The background task finished.
    PromptFinished {
        /// The operation, so a stale result can be matched to its prompt.
        prompt: PromptKind,
        request_id: u64,
        result: Result<(), CreateDirError>,
    },

    // --- Selection ---
    /// Insert or Space: tag or untag the row under the cursor and go down one.
    ToggleTag,
    /// Shift+Up/Down: tag or untag the row under the cursor, then move.
    TagMove(isize),
    /// `*`: tag everything except `..`.
    TagAll,
    /// Ctrl+`*`: drop every tag.
    ClearTags,

    // --- Panel handling ---
    /// Wheel or trackpad over a panel.
    PanelScrolled {
        side: PanelSide,
        delta: iced::mouse::ScrollDelta,
    },
    SwitchPanel,
    /// A click on a path tab in the title bar: that panel becomes the active one.
    ActivatePanel(PanelSide),
    /// The window row: moving, zooming and, where the system draws no frame,
    /// the window buttons and the edges that resize.
    TitleBar(TitleBarMessage),
    /// Temporarily switches the UI language. Not yet persisted in config.
    SwitchLanguage,
    /// Mouse click on a row of a panel.  is true when the click landed in
    /// the tag column, which toggles the tag instead of moving the cursor.
    RowClicked {
        side: PanelSide,
        index: usize,
        on_tag: bool,
    },

    /// A click on a column title: sort that panel by the column.
    SortBy {
        side: PanelSide,
        column: SortColumn,
    },
    /// Ctrl+F3, Ctrl+F5 or Ctrl+F6: sort the active panel.
    SortActive(SortColumn),

    // --- Search (Alt+F7, Cmd/Ctrl+Shift+F) ---
    /// Show the search in the active panel, or move on to its next field when
    /// it is already there.
    OpenSearch,
    Search(SearchMessage),

    // --- Filing module: the index and its first-start questions ---
    Filing(FilingMessage),

    // --- Filing status page ---
    FilingStatus(FilingStatusMessage),

    // --- The Hermes column of the panels ---
    HermesColumn(HermesColumnMessage),

    // --- Filing setup: the locations, written to the config ---
    FilingSetup(FilingSetupMessage),

    // --- Rule suggestions: rules read out of how the archive is filed ---
    RuleSuggestions(RuleSuggestionsMessage),
    ReorganisationPlan(ReorganisationPlanMessage),
    ReviewList(ReviewListMessage),
    QuickFix(QuickFixMessage),
    HermesFile(HermesFileMessage),
    SavedQueries(SavedQueriesMessage),
    /// The sidebar of the Hermes card: closes the card that is up and opens
    /// this page.
    HermesPage(HermesPage),
    /// The arrow keys move to the sidebar or back into the page.
    HermesFocus(HermesFocus),
    /// Up or down in the sidebar: the previous or next page.
    HermesStep(isize),
    /// The user asked to carry out these entries of the reorganisation plan.
    /// Nothing acts on it yet.
    ExecuteReorganisation(Vec<PlannedItem>),
    /// The executor on its blocking thread is done.
    ReorganisationExecuted(Result<ExecutionSummary, String>),
    UndoReorganisation,
    /// The filing job on its blocking thread is done.
    DocumentsFiled(Result<FilingSummary, String>),
    /// `None`: there was no carried out reorganisation left to undo.
    ReorganisationUndone(Result<Option<UndoReport>, String>),

    // --- Quick Open (Cmd/Ctrl+P) ---
    QuickOpen(QuickOpenMessage),

    // --- Command line below the panels (F2) ---
    CommandLine(CommandLineMessage),

    // --- Command palette (Cmd/Ctrl+Shift+P) ---
    CommandPalette(CommandPaletteMessage),

    // --- Theme download (command palette: fetch a color theme) ---
    ThemeFetch(ThemeFetchMessage),

    // --- Text recognition models (command palette: download them after asking) ---
    OcrModels(OcrModelsMessage),
    AiModel(AiModelMessage),

    // --- Settings tab ---
    Settings(SettingsMessage),
    KeyBindings(KeyBindingsMessage),

    // --- Native menu bar (macOS) ---
    /// A command chosen from the menu bar or the command list.
    #[cfg(any(target_os = "macos", test))]
    RunCommand(KeyAction),
    #[cfg(target_os = "macos")]
    InstallNativeMenu,

    // --- Window / app ---
    WindowResized(Size),
    Quit,
}

/// A page of the settings tab, one entry of its feature list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsFeature {
    Look,
    Language,
    Programs,
    Keys,
    Hermes,
}

impl SettingsFeature {
    pub const ALL: [Self; 5] = [
        Self::Look,
        Self::Language,
        Self::Programs,
        Self::Keys,
        Self::Hermes,
    ];
}

/// A setting the settings tab changes: applied at once and written to the
/// config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingChange {
    Preset(ThemePreset),
    Language(Language),
    HermesEnabled(bool),
    Load(Load),
    AiEnabled(bool),
    /// `None` hands the file to the system default again.
    Program {
        kind: OpenKind,
        line: Option<ProgramLine>,
    },
    /// The theme file as `[theme] vscode` names it, `None` drops it.
    VscodeTheme(Option<String>),
    /// `#rrggbb` or `#rgb`, `None` leaves the role to the color scheme.
    Color {
        role: ColorRole,
        color: Option<String>,
    },
    AiProvider(AiProviderChoice),
}

/// A text field of the settings tab. What is typed stays a draft until
/// Enter, which checks it and writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SettingField {
    Program(OpenKind),
    Color(ColorRole),
    AiThreads,
    AiEndpoint,
    AiModel,
}

/// Everything the settings tab sends.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsMessage {
    /// The key and the command list: opens the tab, or closes it when it is up.
    Toggle,
    Close,
    Select(SettingsFeature),
    SearchChanged(String),
    Change(SettingChange),
    Edit {
        field: SettingField,
        text: String,
    },
    Submit(SettingField),
    /// Opens the system file dialog for the program behind F3 or F4.
    PickProgram(OpenKind),
    ProgramPicked {
        kind: OpenKind,
        path: Option<PathBuf>,
    },
    PickThemeFile,
    ThemeFilePicked(Option<PathBuf>),
    ChooseProvider(AiProviderName),
    /// The config as read back after the write of `change`.
    Written {
        change: SettingChange,
        result: Result<Box<Config>, FilingWriteError>,
    },
}

/// Everything the keyboard shortcuts dialog sends.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyBindingsMessage {
    /// The key, the command list and the settings tab open the dialog.
    Open,
    Close,
    SearchChanged(String),
    /// A character typed while the list has the keys: it goes to the search
    /// field, which takes the keys again.
    Typed(String),
    /// A chord pressed in the search field: finds what it is bound to.
    SearchByKey(Binding),
    /// The arrows: a row up or down.
    Move(isize),
    /// The arrows: the key cap left or right; past the last one is the slot
    /// that adds a key.
    MoveKey(isize),
    /// A click on a row, or on one of its key caps.
    Select {
        row: usize,
        key: Option<usize>,
    },
    Scrolled(iced::mouse::ScrollDelta),
    /// Enter: records a new key for the focused cap, or a further one on the
    /// slot that adds a key.
    Record,
    /// Records a further key for the selected action.
    RecordAdditional,
    /// The key pressed while recording.
    Recorded(Binding),
    /// Escape while recording.
    CancelRecording,
    /// Delete or Backspace: removes the focused key.
    RemoveKey,
    /// Back to the keys the action ships with.
    Reset,
    /// Asks before every action goes back to its built-in keys.
    ResetAll,
    /// Takes the key from the action it is bound to, or confirms a reset of
    /// everything.
    Confirm,
    /// Leaves the clash or the question without changing anything.
    Dismiss,
    /// The config as read back after a write.
    Written(Result<Box<Config>, FilingWriteError>),
}

/// What the title bar asks of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleBarMessage {
    /// A press on an empty part of the row: the window follows the pointer.
    Drag,
    /// A double click on an empty part, or the maximize button: zoom on macOS,
    /// maximize or restore elsewhere.
    ToggleMaximize,
    Minimize,
    Resize(ResizeEdge),
}

/// The side or corner of the window a resize starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl ResizeEdge {
    pub fn direction(self) -> iced::window::Direction {
        use iced::window::Direction;
        match self {
            Self::North => Direction::North,
            Self::South => Direction::South,
            Self::East => Direction::East,
            Self::West => Direction::West,
            Self::NorthEast => Direction::NorthEast,
            Self::NorthWest => Direction::NorthWest,
            Self::SouthEast => Direction::SouthEast,
            Self::SouthWest => Direction::SouthWest,
        }
    }
}

/// Everything the search view sends. Its own enum so the panel messages stay
/// the panels': the same key means "move the cursor" in both.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchMessage {
    QueryChanged(String),
    IncludeChanged(String),
    ExcludeChanged(String),
    /// A printable key typed while the result list has the keys: it goes to the
    /// end of the query, as in VS Code.
    AppendToQuery(String),
    ToggleCase,
    ToggleWholeWord,
    ToggleRegex,
    /// Tag the cursor's file and move to the next file row.
    ToggleTag,
    /// Tag the cursor's file and move to the next (`1`) or previous (`-1`) file row.
    TagMove(isize),
    TagAll,
    ClearTags,
    /// The typing pause of this generation is over. An older timer finds a
    /// newer generation and does nothing.
    Debounced(u64),
    /// A batch of what the running search found. Dropped when `generation` is
    /// not the current search's.
    Events {
        generation: u64,
        events: Vec<SearchEvent>,
    },
    Close,
    Move(isize),
    Page(isize),
    First,
    Last,
    /// Right (`true`) or Left (`false`) on the cursor's file.
    Expand(bool),
    /// Enter: open the file in the other panel, or the line in the editor.
    Activate,
    RowClicked(usize),
    /// Double click on a row.
    RowActivated(usize),
    /// The chevron of the file at this index in the results.
    ToggleGroup(usize),
    Scrolled(iced::mouse::ScrollDelta),
}

/// Everything the filing status page sends.
#[derive(Debug, Clone, PartialEq)]
pub enum FilingStatusMessage {
    Open,
    Close,
    Toggle,
    /// The once-a-second look at the pipeline.
    Tick,
    TogglePause,
    SetLoad(Load),
    /// One worker more (`1`) or fewer (`-1`) than now.
    StepWorkers(i8),
    IndexFigures(Result<IndexFigures, String>),
    /// The counts of Hermes, read in the background.
    HermesCounts(Result<HermesCounts, String>),
    /// An error row was pressed: show its file in the panel.
    ErrorActivated(PathBuf),
    MoveError(i32),
    ActivateSelectedError,
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
    /// Make the document folder at this index of the chosen list the Central
    /// Filing.
    SetTarget(usize),
    ToggleImages(usize),
    Remove(usize),
    Save,
    /// The filing settings read back from the written file.
    Saved(Result<Filing, FilingWriteError>),
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
    /// A click on the line at this position: an entry or a group heading.
    Toggle(usize),
    ToggleCursor,
    /// Enter: hands the selection over as `Message::ExecuteReorganisation`.
    Execute,
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
    Inspection,
    Review,
    RuleSuggestions,
    Reorganisation,
    SavedQueries,
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
    /// The wheel or trackpad over the list.
    Scrolled(iced::mouse::ScrollDelta),
    Move(isize),
    /// A click on the query at this position.
    Select(usize),
    New,
    FieldChanged(QueryField, String),
    TaxContextChosen(Option<TaxContext>),
    /// Tab (`false`) or Shift+Tab (`true`) in the form.
    FocusField {
        backwards: bool,
    },
    /// Left in the form: asks which field has the focus.
    LeftInForm,
    /// The answer: the field that has the focus, if any. Left only goes on to
    /// the sidebar when that field is empty (iced does not tell where its
    /// caret is) or none has the focus.
    LeftChecked(Option<QueryField>),
    Save,
    Saved {
        generation: u64,
        result: Result<SavedQueryId, String>,
    },
    /// The first step of deleting: the question in the page.
    AskDelete,
    CancelStep,
    ConfirmDelete,
    Deleted {
        generation: u64,
        result: Result<SavedQueryId, String>,
    },
    /// Enter or the button: asks the index.
    Run,
    Ran {
        generation: u64,
        result: Result<Vec<QueryHit>, String>,
    },
    ToggleHits,
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
    TaxContextChosen(Option<TaxContext>),
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
    /// The offer after filing: a rule for this sender.
    CreateRule,
    RuleCreated {
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
    Edit,
    CancelEdit,
    ConditionChanged(String),
    KindChanged(String),
    PatternChanged(String),
    /// Tab (`false`) or Shift+Tab (`true`) in the form.
    FocusField {
        backwards: bool,
    },
    /// Enter or the button: confirms the suggestion as it is, or as edited.
    Confirm,
    Discard,
    /// The documents a suggestion's condition finds.
    MatchesFound {
        id: RuleId,
        result: Result<Vec<PathBuf>, String>,
    },
}

/// Everything the card that fetches a color theme sends.
#[derive(Debug, Clone, PartialEq)]
pub enum ThemeFetchMessage {
    /// Opens the card, or gives its field the focus again when it is up.
    Open,
    Close,
    TextChanged(String),
    /// Enter: fetches the typed extension, or installs the theme under the
    /// cursor, whichever the card is waiting for.
    Activate,
    Move(isize),
    /// A click on the theme at this position of the list: installs it.
    Pick(usize),
    /// The download of the generation is done. Dropped when the card was
    /// closed or opened again meanwhile.
    Fetched {
        generation: u64,
        result: Result<Fetched, FetchError>,
    },
    /// The installation of the generation is done.
    Installed {
        generation: u64,
        result: Result<Box<Installed>, FetchError>,
    },
}

/// Everything the card that asks for the text recognition models sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrModelsMessage {
    Open,
    Close,
    Download,
    /// The download of the generation is done. Dropped when the card was
    /// closed or opened again meanwhile.
    Downloaded {
        generation: u64,
        result: Result<(), DownloadError>,
    },
}

/// Everything the card that asks for the AI model sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiModelMessage {
    Open,
    Close,
    /// Download, or continue where an earlier download stopped.
    Download,
    Stop,
    /// The download of the generation has ended. Dropped when the card was
    /// closed or opened again meanwhile.
    Downloaded {
        generation: u64,
        result: Result<(), AiDownloadError>,
    },
}

/// Everything Quick Open sends.
#[derive(Debug, Clone, PartialEq)]
pub enum QuickOpenMessage {
    Open,
    QueryChanged(String),
    /// A batch of the file walk. Dropped when `generation` is not the current
    /// walk's.
    Events {
        generation: u64,
        events: Vec<SearchEvent>,
    },
    /// The background ranking of `chunks_scored` chunks for the query of
    /// `generation` in the box of `walk_generation` is done.
    Ranked {
        walk_generation: u64,
        generation: u64,
        hits: Vec<crate::quick_open::Hit>,
        chunks_scored: usize,
    },
    Move(isize),
    Activate,
    RowActivated(usize),
    Close,
}

/// Everything the command line below the panels sends.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandLineMessage {
    Focus,
    Leave,
    Input(String),
    Submit,
    HistoryPrevious,
    HistoryNext,
    /// The background look at the directory a `cd` asked for. An answer whose
    /// `request_id` is not the latest is dropped.
    CdChecked {
        request_id: u64,
        side: PanelSide,
        target: PathBuf,
        result: Result<Destination, PathProblem>,
    },
    /// A shell command ended.
    Finished {
        command: String,
        outcome: ShellOutcome,
    },
}

/// Everything the command palette sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandPaletteMessage {
    /// Opens the palette, or gives its field the focus again when it is up.
    Toggle,
    QueryChanged(String),
    Move(isize),
    /// A click on the entry at this position of the list: runs it.
    Select(usize),
    /// Enter: runs the entry under the cursor.
    Run,
    Close,
}

/// What the copy chords put on the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardKind {
    Path,
    Name,
    /// The directory the active panel shows, not a row in it.
    Directory,
}

/// Copy or move. One message with the kind inside, so the keymap and the job
/// queue do not each need a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    Copy,
    Move,
}

impl TransferKind {
    pub fn job_kind(self) -> crate::jobs::JobKind {
        match self {
            TransferKind::Copy => crate::jobs::JobKind::Copy,
            TransferKind::Move => crate::jobs::JobKind::Move,
        }
    }
}

/// What to do about a name that is already taken.
///
/// The "All" options are the point: answering the same question per file turns
/// five files into five dialogs and fifty thousand into an afternoon.
///
/// The two `All` variants are what the single-file answers become when "for all
/// files" is ticked; see `App::answer_conflict`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    AllOverwrite,
    AllKeep,
    ThisOverwrite,
    ThisKeep,
    Cancel,
}

impl ConflictChoice {
    /// What to do with this one file.
    pub fn conflict(self) -> crate::fs::transfer::OnConflict {
        match self {
            ConflictChoice::AllOverwrite | ConflictChoice::ThisOverwrite => {
                crate::fs::transfer::OnConflict::Overwrite
            }
            // Cancel never reaches the filesystem; `Fail` is the rule that
            // would refuse rather than touch anything.
            ConflictChoice::AllKeep | ConflictChoice::ThisKeep => {
                crate::fs::transfer::OnConflict::Skip
            }
            ConflictChoice::Cancel => crate::fs::transfer::OnConflict::Fail,
        }
    }

    /// The same answer, applied to every later conflict too.
    pub fn for_all(self) -> Self {
        match self {
            ConflictChoice::ThisOverwrite => ConflictChoice::AllOverwrite,
            ConflictChoice::ThisKeep => ConflictChoice::AllKeep,
            other => other,
        }
    }

    /// Whether the answer also settles every later one.
    pub fn applies_to_all(self) -> bool {
        matches!(self, ConflictChoice::AllOverwrite | ConflictChoice::AllKeep)
    }
}
