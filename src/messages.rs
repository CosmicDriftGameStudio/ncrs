//! Central message type. Every state change in the app is triggered by one
//! of these messages and handled in [`crate::app::App::update`].

use std::path::PathBuf;

use iced::Size;

use crate::clipboard_paste::PastedContent;
use crate::filing::FilingMessage;
#[cfg(any(target_os = "macos", test))]
use crate::keymap::KeyAction;
pub use ncrs_core::PanelSide;
pub use ncrs_files::hit_list::HitListMessage;
pub use ncrs_files::message::PanelMessage;
pub use ncrs_files::MenuMessage;
pub use ncrs_hermes_chat::Message as HermesChatMessage;
#[cfg(test)]
pub use ncrs_hermes_ui::message::RuleFormMessage;
pub use ncrs_hermes_ui::message::{
    ActivityMessage, FilingStatusMessage, HermesColumnMessage, HermesFileMessage,
    HermesOverviewMessage, HermesPage, RulesMessage,
    SortingMessage, SuggestMessage, TargetsMessage,
    SavedQueriesMessage, TagsMessage,
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
    /// Escape with the config error up: take it off the status line.
    DismissConfigError,
    /// One tick of a running transfer, sent from the blocking thread.
    JobProgress(crate::fs::transfer::Tick),
    /// F5 or F6, decided by the action rather than a message each.
    Transfer(TransferKind),
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
    /// A message of the file operations: `ncrs_ops::update` handles it, or the
    /// app where it needs more than the operations' own state.
    Ops(OpsMessage),

    // --- drive menu (Alt+F1, Alt+F2) ---
    /// The drive menu and the context menu of a panel.
    Files(MenuMessage),
    /// Adds the active panel's folder to the favourites.
    AddFavourite,
    /// Takes the active panel's folder out of the favourites.
    RemoveFavourite,
    /// Opens the drive menu on the first favourite.
    OpenFavourites,
    /// Opens favourite N (counted from 0) in the active panel: a key, the
    /// command list or the menu bar, only while the panels have the keys.
    OpenFavourite(usize),
    /// The bar under the path tabs and the menu of its slots.
    FavouritesBar(FavouritesBarMessage),

    // --- path field in the status bar ---
    /// A mouse press that no widget took.
    ClickedOutside,

    // --- Modal prompt (see crate::dialog) ---
    /// F7: open the create-directory prompt.
    CreateDirPrompt,
    /// Shift+F4: open the new-file prompt.
    CreateFilePrompt,
    /// `+` or `-`: ask for the pattern of the names to tag or untag.
    TagPatternPrompt {
        untag: bool,
    },
    /// Cmd/Ctrl+G: edit the path of the active panel in the status bar.
    GoToPath,
    /// Alt+F5: ask for the name of a new archive in the other panel's folder.
    PackPrompt,
    /// Unpack the archives under the cursor or tagged next to themselves.
    ExtractHere,
    /// Option+Cmd+R (Ctrl+Alt+R elsewhere): open the permissions dialog for the tagged entries.
    PermissionsPrompt,

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
    // --- The Git tab, opened from the command list or the folder context menu ---
    GitView(GitTabId, ncrs_git_view::Message),
    OpenGitView,
    OpenGitFile(GitFileView),
    /// A running git job asks for a login.
    GitAsked(ncrs_git::askpass::AskRequest),
    /// The clock of the background fetch.
    GitFetchDue,
    // --- The Repos tab, every repository of the machine in one list ---
    Repos(ncrs_repos::Message),
    OpenRepos,
    CloseRepos,
    // --- The filing tab, a source folder planned live into the write folder ---
    FilingView(ncrs_filing_view::Message),
    /// The note on the file under the cursor, made when there is none.
    NoteOnFile,
    // --- Paths other programs hand to ncrs ---
    /// What was found in the inbox of the second starts and, on macOS, the
    /// files the system opened ncrs with, oldest first.
    HandedOver(Vec<ncrs_handoff::Request>),
    // --- The check for a newer release ---
    Updates(ncrs_updates::Message),
    /// The version at the right of the status bar: the Updates page, or the
    /// About card where the build has no update check.
    VersionPressed,
    HitList(HitListMessage),
    // --- The trial and the purchase of the App Store build ---
    Store(StoreMessage),

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
    Sorting(SortingMessage),
    Targets(TargetsMessage),
    Suggest(SuggestMessage),
    HermesFile(HermesFileMessage),
    SavedQueries(SavedQueriesMessage),
    Tags(TagsMessage),
    Activity(ActivityMessage),
    /// Opens one of the single pages.
    HermesPage(HermesPage),
    /// The background filing of the watched sources is done.
    ContinuousFiled(Result<Vec<ncrs_hermes_ui::SourceRun>, String>),

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
    Quit,
    /// The local model is freed after a quit: the process may end now.
    #[cfg(feature = "local-ai")]
    QuitReady,
}

pub use ncrs_config::SettingsFeature;

/// The App Store build asks the store and reacts to what it says.
#[derive(Debug, Clone, PartialEq)]
pub enum StoreMessage {
    /// The minute clock: asks again when the store reported a change or an
    /// hour has passed.
    Tick,
    /// What the store allows now.
    Fetched(ncrs_store::Entitlement),
    /// The hint of a refused action or of the status bar: opens the page.
    OpenPage,
    /// The trial or the purchase has been answered by the store.
    PurchaseDone(ncrs_store::PurchaseOutcome),
    /// "Restore purchase" has been answered.
    Restored(ncrs_store::Entitlement),
}

/// Which of the open Git tabs a message or a tab stop belongs to; stays the
/// same while other Git tabs open and close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct GitTabId(pub u32);

/// What the Git tab shows of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitFileView {
    History,
    Blame,
}

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
    Git(GitTabId),
    Repos,
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
    /// An arrow of the tab strip: the first tab that shows, while not all fit.
    TabsFrom(usize),
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
            Hermes::Sorting(inner) => Self::Sorting(inner),
            Hermes::Targets(inner) => Self::Targets(inner),
            Hermes::Suggest(inner) => Self::Suggest(inner),
            Hermes::SavedQueries(inner) => Self::SavedQueries(inner),
            Hermes::Tags(inner) => Self::Tags(inner),
            Hermes::Activity(inner) => Self::Activity(inner),
            Hermes::ContinuousFiled(result) => Self::ContinuousFiled(result),
            Hermes::HermesColumn(inner) => Self::HermesColumn(inner),
            Hermes::HermesFile(inner) => Self::HermesFile(inner),
            Hermes::ToggleFileListTags => Self::Settings(SettingsMessage::ToggleFileListTags),
        }
    }
}

impl From<OpsMessage> for Message {
    fn from(ops: OpsMessage) -> Self {
        Self::Ops(ops)
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
            Menu::Click(index) => Self::Files(MenuMessage::ContextMenuClick(index)),
            Menu::Hover(index) => Self::Files(MenuMessage::ContextMenuHover(index)),
            Menu::SubClick(index) => Self::Files(MenuMessage::ContextMenuSubClick(index)),
            Menu::SubHover(index) => Self::Files(MenuMessage::ContextMenuSubHover(index)),
        }
    }
}

pub use ncrs_files::FavouritesBarMessage;
