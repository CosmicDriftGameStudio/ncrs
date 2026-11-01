//! Central message type. Every state change in the app is triggered by one
//! of these messages and handled in [`crate::app::App::update`].

use std::path::PathBuf;

use iced::Size;

use crate::app::{RowFailure, Transfer};
use crate::clipboard_paste::PastedContent;
use crate::dialog::PromptKind;
use crate::execute::UndoReport;
use crate::filing::FilingMessage;
use crate::filing_work::FilingSummary;
use crate::fs::CreateDirError;
use crate::fs::{Destination, PathProblem};
use crate::jobs::JobEvent;
#[cfg(any(target_os = "macos", test))]
use crate::keymap::KeyAction;
use crate::reorganisation_work::{ExecutionSummary, PlannedItem};
use crate::replan_work::ReplanSummary;
pub use ncrs_core::PanelSide;
pub use ncrs_files::hit_list::HitListMessage;
pub use ncrs_files::message::PanelMessage;
pub use ncrs_hermes_ui::message::{
    FilingSetupMessage, FilingStatusMessage, HermesColumnMessage, HermesFileMessage, HermesFocus,
    HermesOverviewMessage, HermesPage, HermesSetupMessage, QuickFixMessage,
    ReorganisationPlanMessage, ReviewListMessage, RuleFormMessage, RuleSuggestionsMessage,
    RulesMessage, SavedQueriesMessage, TagsMessage,
};
pub use ncrs_ops::{ConflictChoice, OpsMessage, TransferDialogMessage, TrashMessage};
pub use ncrs_search::message::SearchMessage;
#[cfg(test)]
pub use ncrs_settings::SettingField;
pub use ncrs_settings::{
    AiModelMessage, KeyBindingsMessage, OcrModelsMessage, SettingChange, SettingsMessage,
    ThemeFetchMessage,
};

/// PartialEq so bindings can be compared; Eq is not derivable because
/// WindowResized carries floats.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    // --- The file panels: navigation, selection, sorting, the pointer over them ---
    Panel(PanelMessage),

    // --- copy / move (F5, F6) ---
    /// The abort key (Ctrl+C on a Mac) or Escape while a job runs: stop it.
    /// Its own message rather than reusing a key binding, because "stop"
    /// only exists while a job does.
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
    /// Cmd+V / Ctrl+V: read the system clipboard and bring what it holds into
    /// the active panel.
    Paste,
    /// The system clipboard has been read for a paste; `None` when it holds
    /// nothing usable.
    ClipboardRead(Option<PastedContent>),
    /// The tagged files, else the one under the cursor, to the system
    /// clipboard as files.
    CopyFilesToClipboard,
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

    /// The file F3 unpacked from an archive is ready, or unpacking failed.
    ViewExtracted {
        /// The copy's folder, so a result that arrives after its job was
        /// stopped can still clean up after itself.
        slot: std::path::PathBuf,
        /// The path of the copy.
        result: Result<std::path::PathBuf, RowFailure>,
        /// The run it belongs to; a result of a stopped one only removes its slot.
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
    /// A pack, a copy into an archive or a delete from one finished.
    ArchiveWritten {
        result: Result<crate::fs::archive::WriteReport, RowFailure>,
        /// The write it belongs to; the result of a stopped one is dropped.
        generation: u64,
    },
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
    /// Alt+F5: ask for the name of a new archive in the other panel's folder.
    PackPrompt,
    /// A keystroke went to the open prompt.
    PromptInput(String),
    /// The "Keep folders" box of the pack prompt for a hit list was clicked.
    PromptToggleKeepFolders,
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

    /// The window row: moving, zooming and, where the system draws no frame,
    /// the window buttons and the edges that resize.
    TitleBar(TitleBarMessage),
    /// Temporarily switches the UI language. Not yet persisted in config.
    SwitchLanguage,
    /// The system drag out of the window could not be started, or did.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    DragOutStarted(Result<(), String>),

    /// Measure the tagged folders, or the one under the cursor.
    ComputeFolderSizes,
    /// One folder of a run is measured; `size` is `None` when the walk was
    /// stopped.
    FolderSizeMeasured {
        run: u64,
        folder: PathBuf,
        size: Option<crate::fs::FolderSize>,
    },

    // --- Search (Alt+F7, Cmd/Ctrl+Shift+F) ---
    /// Show the search in the active panel, or move on to its next field when
    /// it is already there.
    OpenSearch,
    Search(SearchMessage),
    HitList(HitListMessage),
    Trash(TrashMessage),

    // --- Filing module: the index and its first-start questions ---
    Filing(FilingMessage),

    // --- Filing status page ---
    FilingStatus(FilingStatusMessage),

    // --- The Hermes column of the panels ---
    HermesColumn(HermesColumnMessage),

    // --- Filing setup: the locations, written to the config ---
    FilingSetup(FilingSetupMessage),

    // --- The first-time setup of Hermes ---
    HermesSetup(HermesSetupMessage),
    /// Moves the dots of the flow picture one step on.
    HermesFlowTick,
    // --- The overview, the first page of the Hermes card ---
    HermesOverview(HermesOverviewMessage),

    // --- Rule suggestions: rules read out of how the archive is filed ---
    RuleSuggestions(RuleSuggestionsMessage),
    Rules(RulesMessage),
    ReorganisationPlan(ReorganisationPlanMessage),
    ReviewList(ReviewListMessage),
    QuickFix(QuickFixMessage),
    HermesFile(HermesFileMessage),
    SavedQueries(SavedQueriesMessage),
    Tags(TagsMessage),
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
    /// The job that files the previewed documents is done.
    DocumentsRefiled(Result<ReplanSummary, String>),
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
    About(AboutMessage),
    AiModel(AiModelMessage),

    // --- Folder grants of the App Store build ---
    Grant(GrantMessage),

    // --- SFTP servers ---
    Remote(RemoteMessage),

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
    /// The window got the focus back: what changed on disk meanwhile shows up.
    WindowFocused,
    /// Files from another app are held over the window.
    FilesHovered,
    /// The files held over the window were taken away again.
    FilesHoverLeft,
    /// One file of a drop; a drop sends one of these per file.
    FileDropped(PathBuf),
    /// The drop with this generation has had all its files delivered.
    DropSettled(u64),
    Quit,
}

pub use ncrs_config::SettingsFeature;

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

/// Everything the about card sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AboutMessage {
    Open,
    Close,
    OpenLicenses,
    OpenRepository,
}

/// Everything the folder grants of the App Store build send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantMessage {
    /// The key action: opens the system folder dialog, or says that this
    /// build needs no grants.
    Open,
    /// The blocked-folder error was clicked: opens the system folder dialog in
    /// the folder, or the nearest one above it that exists.
    OpenFor(PathBuf),
    /// The dialog's button: opens the system folder dialog and keeps the
    /// dialog up until a folder is chosen.
    Choose,
    /// The dialog's other button, and Escape.
    Later,
    /// The system folder dialog answered; `None` when it was cancelled.
    Picked(Option<PathBuf>),
    /// The grant of the folder was saved, or why it could not be.
    Saved {
        folder: PathBuf,
        result: Result<(), String>,
    },
}

#[cfg(test)]
pub use ncrs_remote::ConnectField;
pub use ncrs_remote::Message as RemoteMessage;

/// What the copy chords put on the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardKind {
    Path,
    Name,
    /// The directory the active panel shows, not a row in it.
    Directory,
}

pub use ncrs_core::TransferKind;
pub use ncrs_palette::{CommandLineMessage, CommandPaletteMessage, QuickOpenMessage};

/// The job queue of the app, which `ncrs-core` knows nothing about.
pub trait TransferKindJob {
    fn job_kind(self) -> crate::jobs::JobKind;
}

impl TransferKindJob for TransferKind {
    fn job_kind(self) -> crate::jobs::JobKind {
        match self {
            TransferKind::Copy => crate::jobs::JobKind::Copy,
            TransferKind::Move => crate::jobs::JobKind::Move,
        }
    }
}

impl From<ncrs_remote::Message> for Message {
    fn from(message: ncrs_remote::Message) -> Self {
        Self::Remote(message)
    }
}

impl From<ncrs_hermes_ui::Message> for Message {
    fn from(message: ncrs_hermes_ui::Message) -> Self {
        use ncrs_hermes_ui::Message as Hermes;
        match message {
            Hermes::Filing(inner) => Self::Filing(inner),
            Hermes::FilingStatus(inner) => Self::FilingStatus(inner),
            Hermes::FilingSetup(inner) => Self::FilingSetup(inner),
            Hermes::HermesSetup(inner) => Self::HermesSetup(inner),
            Hermes::HermesOverview(inner) => Self::HermesOverview(inner),
            Hermes::RuleSuggestions(inner) => Self::RuleSuggestions(inner),
            Hermes::Rules(inner) => Self::Rules(inner),
            Hermes::ReorganisationPlan(inner) => Self::ReorganisationPlan(inner),
            Hermes::ReviewList(inner) => Self::ReviewList(inner),
            Hermes::QuickFix(inner) => Self::QuickFix(inner),
            Hermes::SavedQueries(inner) => Self::SavedQueries(inner),
            Hermes::Tags(inner) => Self::Tags(inner),
            Hermes::HermesPage(page) => Self::HermesPage(page),
            Hermes::ExecuteReorganisation(entries) => Self::ExecuteReorganisation(entries),
            Hermes::ToggleFileListTags => Self::Settings(SettingsMessage::ToggleFileListTags),
        }
    }
}

impl From<OpsMessage> for Message {
    fn from(ops: OpsMessage) -> Self {
        match ops {
            OpsMessage::TransferDialog(dialog) => Self::TransferDialog(dialog),
            OpsMessage::TransferConflict(choice) => Self::TransferConflict(choice),
            OpsMessage::ToggleConflictAll => Self::ToggleConflictAll,
            OpsMessage::DeleteCancel => Self::DeleteCancel,
            OpsMessage::DeleteConfirm => Self::DeleteConfirm,
            OpsMessage::AbortJob => Self::AbortJob,
            OpsMessage::Trash(trash) => Self::Trash(trash),
        }
    }
}

impl Message {
    /// The message of the app for one of the settings.
    pub fn from_settings(message: ncrs_settings::Message) -> Self {
        match message {
            ncrs_settings::Message::Settings(settings_message) => Self::Settings(settings_message),
            ncrs_settings::Message::KeyBindings(keys_message) => Self::KeyBindings(keys_message),
            ncrs_settings::Message::ThemeFetch(theme_message) => Self::ThemeFetch(theme_message),
            ncrs_settings::Message::OcrModels(ocr_message) => Self::OcrModels(ocr_message),
            ncrs_settings::Message::AiModel(ai_message) => Self::AiModel(ai_message),
        }
    }
}
