//! Central message type. Every state change in the app is triggered by one
//! of these messages and handled in [`crate::app::App::update`].

use std::path::PathBuf;

use iced::Size;

use crate::app::RowFailure;
use crate::clipboard_paste::PastedContent;
use crate::filing::FilingMessage;
use crate::filing_work::FilingSummary;
use crate::fs::{Destination, PathProblem};
#[cfg(any(target_os = "macos", test))]
use crate::keymap::KeyAction;
pub use ncrs_core::PanelSide;
pub use ncrs_files::hit_list::HitListMessage;
pub use ncrs_files::message::PanelMessage;
pub use ncrs_hermes_chat::Message as HermesChatMessage;
#[cfg(test)]
pub use ncrs_hermes_ui::message::RuleFormMessage;
pub use ncrs_hermes_ui::message::{
    FilingStatusMessage, HermesColumnMessage, HermesFileMessage, HermesOverviewMessage, HermesPage,
    QuickFixMessage, RulesMessage, SavedQueriesMessage, TagsMessage,
};
pub use ncrs_ops::{
    BatchRenameMessage, ConflictChoice, OpsMessage, PermissionsMessage, TransferDialogMessage,
    TrashMessage,
};
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
    /// Ctrl+Tab: the next of all open tabs.
    NextTab,
    /// Ctrl+Shift+Tab: the previous of all open tabs.
    PreviousTab,
    /// A click on the tab of an open page.
    ShowTab(ShownTab),
    /// Tab or Shift+Tab inside a page that stands in for the panels: asks the
    /// widgets whether a text field has the keyboard.
    TabInPage {
        backward: bool,
    },
    /// A click on a row of a page the app fills, or on its control: the
    /// keyboard focus moves to the row and the control's message is handled.
    FocusPageThen(usize, Option<Box<Message>>),
    /// The answer: a field keeps Tab for moving between fields, else Tab moves
    /// between the stops of the page.
    TabInPageAnswered {
        backward: bool,
        field_focused: bool,
    },
    /// The system clipboard has been read for a paste; `None` when it holds
    /// nothing usable.
    ClipboardRead(Option<PastedContent>),
    /// The tagged files, else the one under the cursor, to the system
    /// clipboard as files.
    CopyFilesToClipboard,
    /// The rename of the entry at `from` has finished.
    Renamed {
        request_id: u64,
        side: PanelSide,
        from: PathBuf,
        result: Result<PathBuf, RowFailure>,
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
    /// "Open terminal here": the terminal starts in the folder under the
    /// cursor, else in the panel's folder.
    OpenTerminal,
    /// Click on the "access blocked" error or the command: open the system's
    /// privacy settings. Only macOS has such a page; elsewhere it does nothing.
    OpenPrivacySettings,

    // --- delete (F8, Shift+F8) ---
    /// F8 (`permanent: false`) or Shift+F8: open the confirmation dialog.
    Delete {
        permanent: bool,
    },
    /// The user confirmed the delete dialog.
    DeleteConfirm,
    /// The user chose to stop the running job and quit.
    QuitConfirm,
    /// A message of the file operations that needs nothing from the app but
    /// their own state: handled by `ncrs_ops::update`.
    Ops(OpsMessage),

    // --- drive menu (Alt+F1, Alt+F2) ---
    /// Alt+F1 or Alt+F2: choose the drive of that panel.
    VolumeMenu(PanelSide),
    /// Adds the active panel's folder to the favourites.
    AddFavourite,
    /// Takes the active panel's folder out of the favourites.
    RemoveFavourite,
    /// Opens the drive menu on the first favourite.
    OpenFavourites,
    /// What the drive menu lists has been read. Dropped when `request` is not
    /// the latest one.
    VolumeMenuFacts {
        request: u64,
        side: PanelSide,
        on_favourites: bool,
        facts: ncrs_files::VolumeFacts,
    },
    /// Opens favourite N (counted from 0) in the active panel: a key, the
    /// command list or the menu bar, only while the panels have the keys.
    OpenFavourite(usize),
    /// The bar under the path tabs and the menu of its slots.
    FavouritesBar(FavouritesBarMessage),

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

    // --- Modal prompt (see crate::dialog) ---
    /// F7: open the create-directory prompt.
    CreateDirPrompt,
    /// Shift+F4: open the new-file prompt.
    CreateFilePrompt,
    /// Alt+F5: ask for the name of a new archive in the other panel's folder.
    PackPrompt,
    /// Unpack the archives under the cursor or tagged next to themselves.
    ExtractHere,
    /// Option+Cmd+R (Ctrl+Alt+R elsewhere): open the permissions dialog for the tagged entries.
    PermissionsPrompt,
    /// A keystroke went to the open prompt.
    PromptInput(String),
    /// The "Keep folders" box of the pack prompt for a hit list was clicked.
    PromptToggleKeepFolders,
    /// A format of the pack dialog was chosen.
    PromptFormat(crate::fs::archive::ArchiveFormat),
    /// The user confirmed the prompt.
    PromptSubmit,
    /// The user dismissed the prompt.
    PromptCancel,

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
    // --- The System page (F12) ---
    Monitor(ncrs_monitor::MonitorMessage),
    // --- The file viewer tab (F3 on a file the config opens internally) ---
    Viewer(ncrs_viewer::Message),
    // --- The notes tab, opened from the command list or on a file ---
    Notes(ncrs_notes::Message),
    OpenNotes,
    // --- The filing tab, a source folder planned live into the write folder ---
    FilingView(ncrs_filing_view::Message),
    /// The note on the file under the cursor, made when there is none.
    NoteOnFile,
    // --- The check for a newer release ---
    Updates(ncrs_updates::Message),
    /// The version at the right of the status bar: the Updates page, or the
    /// About card where the build has no update check.
    VersionPressed,
    HitList(HitListMessage),
    Trash(TrashMessage),

    // --- The Hermes chat (F2) ---
    HermesChat(HermesChatMessage),
    /// Shift+F2 in the chat: the chat closes and the command line takes the focus.
    HermesChatToCommandLine,

    // --- Filing module: the index and its first-start questions ---
    Filing(FilingMessage),

    // --- Filing status page ---
    FilingStatus(FilingStatusMessage),

    // --- The Hermes column of the panels ---
    HermesColumn(HermesColumnMessage),

    /// The protection buttons of the Index page in the settings.
    HermesOverview(HermesOverviewMessage),
    Rules(RulesMessage),
    QuickFix(QuickFixMessage),
    HermesFile(HermesFileMessage),
    SavedQueries(SavedQueriesMessage),
    Tags(TagsMessage),
    /// Opens one of the single pages.
    HermesPage(HermesPage),
    /// Opens the settings on the AI page, where the Hermes switch is.
    ShowAiSettings,
    /// The filing job on its blocking thread is done.
    DocumentsFiled(Result<FilingSummary, String>),

    // --- Quick Open (Cmd/Ctrl+P) ---
    QuickOpen(QuickOpenMessage),

    // --- Command line below the panels (Shift+F2) ---
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

    // --- Folder comparison ---
    Compare(CompareMessage),

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

/// What the area under the title bar shows. Settings, the System page and the
/// Hermes chat stay open while the panels are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShownTab {
    #[default]
    Panels,
    Settings,
    Monitor,
    Viewer,
    Notes,
    Filing,
    Hermes,
}

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

pub use ncrs_settings::about::AboutMessage;

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

pub use ncrs_compare::Message as CompareMessage;
#[cfg(test)]
pub use ncrs_remote::ConnectField;
pub use ncrs_remote::Message as RemoteMessage;

pub use ncrs_ops::ClipboardKind;

pub use ncrs_core::TransferKind;
pub use ncrs_palette::{CommandLineMessage, CommandPaletteMessage, QuickOpenMessage};

impl From<ncrs_compare::Message> for Message {
    fn from(message: ncrs_compare::Message) -> Self {
        Self::Compare(message)
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
            Hermes::HermesOverview(inner) => Self::HermesOverview(inner),
            Hermes::Rules(inner) => Self::Rules(inner),
            Hermes::QuickFix(inner) => Self::QuickFix(inner),
            Hermes::SavedQueries(inner) => Self::SavedQueries(inner),
            Hermes::Tags(inner) => Self::Tags(inner),
            Hermes::DocumentsFiled(result) => Self::DocumentsFiled(result),
            Hermes::HermesColumn(inner) => Self::HermesColumn(inner),
            Hermes::HermesFile(inner) => Self::HermesFile(inner),
            Hermes::ToggleFileListTags => Self::Settings(SettingsMessage::ToggleFileListTags),
        }
    }
}

impl From<OpsMessage> for Message {
    fn from(ops: OpsMessage) -> Self {
        match ops {
            OpsMessage::TransferDialog(dialog) => Self::TransferDialog(dialog),
            OpsMessage::DeleteConfirm => Self::DeleteConfirm,
            OpsMessage::QuitConfirm => Self::QuitConfirm,
            OpsMessage::AbortJob => Self::AbortJob,
            OpsMessage::Renamed {
                request_id,
                side,
                from,
                result,
            } => Self::Renamed {
                request_id,
                side,
                from,
                result,
            },
            OpsMessage::Trash(trash) => Self::Trash(trash),
            handled_by_ops => Self::Ops(handled_by_ops),
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

impl From<ncrs_files::context_menu_view::ContextMenuMessage> for Message {
    fn from(menu: ncrs_files::context_menu_view::ContextMenuMessage) -> Self {
        use ncrs_files::context_menu_view::ContextMenuMessage as Menu;
        match menu {
            Menu::Click(index) => Self::ContextMenuClick(index),
            Menu::Hover(index) => Self::ContextMenuHover(index),
            Menu::SubClick(index) => Self::ContextMenuSubClick(index),
            Menu::SubHover(index) => Self::ContextMenuSubHover(index),
        }
    }
}

pub use ncrs_files::FavouritesBarMessage;
