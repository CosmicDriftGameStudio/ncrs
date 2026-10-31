//! Keyboard bindings and the header hints, from one source.
//!
//! A `KeyAction` names what a key can do; the `Keymap` says which keys do it.
//! The key handler and the header hint list are both derived from the keymap,
//! so a binding cannot exist in one and be missing in the other. The built-in
//! keymap is the default; `[keys]` in the config file replaces the keys of the
//! actions it names.
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use iced::keyboard::{key, Key, Modifiers};

use crate::config::KeySection;
use crate::fs::{OpenKind, SortColumn};
use crate::hermes_column::FolderRole;
use crate::i18n::{Language, Msg};
use crate::messages::Message;
use crate::messages::{
    AboutMessage, AiModelMessage, ClipboardKind, CommandLineMessage, CommandPaletteMessage,
    FilingSetupMessage, FilingStatusMessage, GrantMessage, HermesFileMessage, KeyBindingsMessage,
    OcrModelsMessage, PanelSide, QuickFixMessage, QuickOpenMessage, RemoteMessage,
    ReorganisationPlanMessage, ReviewListMessage, RuleSuggestionsMessage, RulesMessage,
    SavedQueriesMessage, SearchMessage, SettingsMessage, ThemeFetchMessage, TransferKind,
    TrashMessage,
};

/// A key plus the modifier that must be held. A bare press has no modifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Binding {
    pub key: Key,
    pub modifiers: Modifiers,
}

impl Binding {
    /// A key that triggers on its own, e.g. F5.
    pub fn key(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers::default(),
        }
    }

    /// A key that only triggers together with a modifier, e.g. Alt+F1.
    pub fn with_modifiers(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }
}

/// Defines `KeyAction` and its config names from one list, so a name cannot be
/// spelt differently in the enum and in the parser.
macro_rules! key_actions {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        /// Something a key can do. The name is what `[keys]` calls it.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum KeyAction {
            $($variant),+
        }

        impl KeyAction {
            /// Every action, in the order of this list.
            pub const ALL: &'static [KeyAction] = &[$(Self::$variant),+];

            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),+
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

key_actions! {
    Quit => "quit",
    GoUp => "go_up",
    Open => "open",
    SwitchPanel => "switch_panel",
    SwitchLanguage => "switch_language",
    ToggleTag => "toggle_tag",
    TagDown => "tag_down",
    TagUp => "tag_up",
    TagAll => "tag_all",
    ClearTags => "clear_tags",
    AbortJob => "abort_job",
    CopyPath => "copy_path",
    CopyName => "copy_name",
    CopyFiles => "copy_files",
    Paste => "paste",
    View => "view",
    Edit => "edit",
    Copy => "copy",
    Move => "move",
    Delete => "delete",
    DeletePermanently => "delete_permanently",
    Trash => "trash",
    RestoreLastDeletion => "restore_last_deletion",
    OpenTrashInFinder => "open_trash_in_finder",
    Mkdir => "mkdir",
    NewFile => "new_file",
    Pack => "pack",
    DriveMenuLeft => "drive_menu_left",
    DriveMenuRight => "drive_menu_right",
    HistoryBack => "history_back",
    HistoryForward => "history_forward",
    AddFavourite => "add_favourite",
    RemoveFavourite => "remove_favourite",
    Favourites => "favourites",
    SortByName => "sort_by_name",
    SortByModified => "sort_by_modified",
    SortBySize => "sort_by_size",
    ComputeFolderSizes => "compute_folder_sizes",
    ContextMenu => "context_menu",
    CursorUp => "cursor_up",
    CursorDown => "cursor_down",
    PageUp => "page_up",
    PageDown => "page_down",
    First => "first",
    Last => "last",
    Search => "search",
    QuickOpen => "quick_open",
    CommandLine => "command_line",
    CommandPalette => "command_palette",
    FilingStatus => "filing_status",
    FilingSetup => "filing_setup",
    Rules => "rules",
    RuleSuggestions => "rule_suggestions",
    ReviewList => "review_list",
    HermesQuickFix => "hermes_quick_fix",
    HermesHandToInbox => "hermes_hand_to_inbox",
    HermesProcessAgain => "hermes_process_again",
    HermesSetInbox => "hermes_set_inbox",
    HermesSetFilingLocation => "hermes_set_filing_location",
    HermesSetArchive => "hermes_set_archive",
    HermesExclude => "hermes_exclude",
    HermesInspect => "hermes_inspect",
    SavedQueries => "saved_queries",
    ReorganisationPlan => "reorganisation_plan",
    UndoReorganisation => "undo_reorganisation",
    FetchTheme => "fetch_theme",
    Settings => "settings",
    KeyBindings => "key_bindings",
    About => "about",
    DownloadOcrModels => "download_ocr_models",
    DownloadAiModel => "download_ai_model",
    GrantFolderAccess => "grant_folder_access",
    ConnectServer => "connect_server",
    DisconnectServer => "disconnect_server",
    SearchCase => "search_case",
    SearchWholeWord => "search_whole_word",
    SearchRegex => "search_regex",
}

/// An area of the window with its own keys: the panels, or the search view
/// that hides them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyScope {
    Panel,
    Search,
}

/// Where an action can be triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    PanelOnly,
    SearchOnly,
    Both,
}

/// Two actions may share a key only when they have no area in common, so one
/// press never triggers two actions that are active at the same time.
fn shares_a_scope(first: KeyAction, second: KeyAction) -> bool {
    [KeyScope::Panel, KeyScope::Search]
        .into_iter()
        .any(|scope| first.works_in(scope) && second.works_in(scope))
}

/// Keys of the search view that no configured chord can take: the search
/// handles them itself, and a search-only action on one would steal it.
fn is_fixed_in_search(binding: &Binding) -> bool {
    use key::Named::*;
    let Key::Named(named) = &binding.key else {
        return false;
    };
    (binding.modifiers - Modifiers::SHIFT).is_empty()
        && matches!(
            named,
            Enter | ArrowUp | ArrowDown | ArrowLeft | ArrowRight | PageUp | PageDown | Home | End
        )
}

impl KeyAction {
    /// Whether the action can be triggered in `scope`. No catch-all arm: a new
    /// action has to say where it works.
    pub fn works_in(self, scope: KeyScope) -> bool {
        let reach = match self {
            Self::SearchCase | Self::SearchWholeWord | Self::SearchRegex => Reach::SearchOnly,
            Self::Quit
            | Self::Search
            | Self::CommandPalette
            | Self::View
            | Self::Edit
            | Self::Copy
            | Self::Move
            | Self::Delete
            | Self::DeletePermanently
            | Self::AbortJob
            | Self::SwitchLanguage
            | Self::SwitchPanel
            | Self::ToggleTag
            | Self::TagDown
            | Self::TagUp
            | Self::TagAll
            | Self::ClearTags => Reach::Both,
            Self::GoUp
            | Self::Open
            | Self::CopyPath
            | Self::CopyName
            | Self::CopyFiles
            | Self::Trash
            | Self::RestoreLastDeletion
            | Self::OpenTrashInFinder
            | Self::Paste
            | Self::Mkdir
            | Self::NewFile
            | Self::Pack
            | Self::QuickOpen
            | Self::CommandLine
            | Self::FilingStatus
            | Self::FilingSetup
            | Self::Rules
            | Self::RuleSuggestions
            | Self::ReviewList
            | Self::HermesQuickFix
            | Self::HermesHandToInbox
            | Self::HermesProcessAgain
            | Self::HermesSetInbox
            | Self::HermesSetFilingLocation
            | Self::HermesSetArchive
            | Self::HermesExclude
            | Self::HermesInspect
            | Self::SavedQueries
            | Self::ReorganisationPlan
            | Self::UndoReorganisation
            | Self::FetchTheme
            | Self::Settings
            | Self::KeyBindings
            | Self::About
            | Self::DownloadOcrModels
            | Self::DownloadAiModel
            | Self::GrantFolderAccess
            | Self::ConnectServer
            | Self::DisconnectServer
            | Self::DriveMenuLeft
            | Self::DriveMenuRight
            | Self::HistoryBack
            | Self::HistoryForward
            | Self::AddFavourite
            | Self::RemoveFavourite
            | Self::Favourites
            | Self::SortByName
            | Self::SortByModified
            | Self::SortBySize
            | Self::ComputeFolderSizes
            | Self::ContextMenu
            | Self::CursorUp
            | Self::CursorDown
            | Self::PageUp
            | Self::PageDown
            | Self::First
            | Self::Last => Reach::PanelOnly,
        };
        match (reach, scope) {
            (Reach::Both, _)
            | (Reach::PanelOnly, KeyScope::Panel)
            | (Reach::SearchOnly, KeyScope::Search) => true,
            (Reach::PanelOnly, KeyScope::Search) | (Reach::SearchOnly, KeyScope::Panel) => false,
        }
    }

    /// Keys that became defaults after a config could already have given them
    /// to another action: bare F2 in 0.6, Shift+F4, the history keys and
    /// primary+D in 0.7, primary+K in 0.8, primary+C (`copy_files`) and primary+Z
    /// (`restore_last_deletion`) after 1.0.
    /// Such a config keeps working: its binding wins over the built-in one,
    /// and the action keeps its other keys.
    pub fn yields_its_built_in_key(self) -> bool {
        matches!(
            self,
            Self::CommandLine
                | Self::NewFile
                | Self::HistoryBack
                | Self::HistoryForward
                | Self::AddFavourite
                | Self::KeyBindings
                | Self::CopyFiles
                | Self::RestoreLastDeletion
        )
    }

    /// The action works on the file under the cursor and only means something
    /// while the Hermes column is up.
    pub fn acts_on_hermes_file(self) -> bool {
        matches!(
            self,
            Self::HermesQuickFix
                | Self::HermesHandToInbox
                | Self::HermesProcessAgain
                | Self::HermesSetInbox
                | Self::HermesSetFilingLocation
                | Self::HermesSetArchive
                | Self::HermesExclude
                | Self::HermesInspect
        )
    }

    /// The action sets the role of a folder, which works before any location
    /// is configured.
    pub fn sets_hermes_role(self) -> bool {
        matches!(
            self,
            Self::HermesSetInbox | Self::HermesSetFilingLocation | Self::HermesSetArchive
        )
    }

    /// The message the key sends. Derived rather than stored next to the key,
    /// so a rebound key cannot send anything but its action.
    pub fn message(self) -> Message {
        match self {
            Self::Quit => Message::Quit,
            Self::GoUp => Message::GoUp,
            Self::Open => Message::OpenSelected,
            Self::SwitchPanel => Message::SwitchPanel,
            Self::SwitchLanguage => Message::SwitchLanguage,
            Self::ToggleTag => Message::ToggleTag,
            Self::TagDown => Message::TagMove(1),
            Self::TagUp => Message::TagMove(-1),
            Self::TagAll => Message::TagAll,
            Self::ClearTags => Message::ClearTags,
            Self::AbortJob => Message::AbortJob,
            Self::CopyPath => Message::CopyToClipboard(ClipboardKind::Path),
            Self::CopyName => Message::CopyToClipboard(ClipboardKind::Name),
            Self::CopyFiles => Message::CopyFilesToClipboard,
            Self::Trash => Message::Trash(TrashMessage::Open),
            Self::RestoreLastDeletion => Message::Trash(TrashMessage::RestoreLast),
            Self::OpenTrashInFinder => Message::Trash(TrashMessage::OpenInFinder),
            Self::Paste => Message::Paste,
            Self::View => Message::OpenExternal(OpenKind::View),
            Self::Edit => Message::OpenExternal(OpenKind::Edit),
            Self::Copy => Message::Transfer(TransferKind::Copy),
            Self::Move => Message::Transfer(TransferKind::Move),
            Self::Delete => Message::Delete { permanent: false },
            Self::DeletePermanently => Message::Delete { permanent: true },
            Self::Mkdir => Message::CreateDirPrompt,
            Self::NewFile => Message::CreateFilePrompt,
            Self::Pack => Message::PackPrompt,
            Self::DriveMenuLeft => Message::VolumeMenu(PanelSide::Left),
            Self::DriveMenuRight => Message::VolumeMenu(PanelSide::Right),
            Self::HistoryBack => Message::HistoryBack,
            Self::HistoryForward => Message::HistoryForward,
            Self::AddFavourite => Message::AddFavourite,
            Self::RemoveFavourite => Message::RemoveFavourite,
            Self::Favourites => Message::OpenFavourites,
            Self::SortByName => Message::SortActive(SortColumn::Name),
            Self::SortByModified => Message::SortActive(SortColumn::Modified),
            Self::SortBySize => Message::SortActive(SortColumn::Size),
            Self::ComputeFolderSizes => Message::ComputeFolderSizes,
            Self::ContextMenu => Message::ContextMenuKey,
            Self::CursorUp => Message::MoveSelection(-1),
            Self::CursorDown => Message::MoveSelection(1),
            Self::PageUp => Message::PageUp,
            Self::PageDown => Message::PageDown,
            Self::First => Message::SelectFirst,
            Self::Last => Message::SelectLast,
            Self::Search => Message::OpenSearch,
            Self::QuickOpen => Message::QuickOpen(QuickOpenMessage::Open),
            Self::CommandLine => Message::CommandLine(CommandLineMessage::Focus),
            Self::CommandPalette => Message::CommandPalette(CommandPaletteMessage::Toggle),
            Self::FilingStatus => Message::FilingStatus(FilingStatusMessage::Toggle),
            Self::FilingSetup => Message::FilingSetup(FilingSetupMessage::Toggle),
            Self::Rules => Message::Rules(RulesMessage::Open),
            Self::RuleSuggestions => Message::RuleSuggestions(RuleSuggestionsMessage::Open),
            Self::ReviewList => Message::ReviewList(ReviewListMessage::Open),
            Self::HermesQuickFix => Message::QuickFix(QuickFixMessage::Open),
            Self::HermesHandToInbox => Message::HermesFile(HermesFileMessage::HandToInbox),
            Self::HermesProcessAgain => Message::HermesFile(HermesFileMessage::ProcessAgain),
            Self::HermesSetInbox => {
                Message::HermesFile(HermesFileMessage::SetRole(FolderRole::Inbox))
            }
            Self::HermesSetFilingLocation => {
                Message::HermesFile(HermesFileMessage::SetRole(FolderRole::FilingLocation))
            }
            Self::HermesSetArchive => {
                Message::HermesFile(HermesFileMessage::SetRole(FolderRole::Archive))
            }
            Self::HermesExclude => Message::HermesFile(HermesFileMessage::ToggleExclusion),
            Self::HermesInspect => Message::HermesFile(HermesFileMessage::ShowInInspection),
            Self::SavedQueries => Message::SavedQueries(SavedQueriesMessage::Open),
            Self::ReorganisationPlan => {
                Message::ReorganisationPlan(ReorganisationPlanMessage::Open)
            }
            Self::UndoReorganisation => Message::UndoReorganisation,
            Self::FetchTheme => Message::ThemeFetch(ThemeFetchMessage::Open),
            Self::Settings => Message::Settings(SettingsMessage::Toggle),
            Self::KeyBindings => Message::KeyBindings(KeyBindingsMessage::Open),
            Self::About => Message::About(AboutMessage::Open),
            Self::DownloadOcrModels => Message::OcrModels(OcrModelsMessage::Open),
            Self::DownloadAiModel => Message::AiModel(AiModelMessage::Open),
            Self::GrantFolderAccess => Message::Grant(GrantMessage::Open),
            Self::ConnectServer => Message::Remote(RemoteMessage::OpenConnect),
            Self::DisconnectServer => Message::Remote(RemoteMessage::DisconnectActive),
            Self::SearchCase => Message::Search(SearchMessage::ToggleCase),
            Self::SearchWholeWord => Message::Search(SearchMessage::ToggleWholeWord),
            Self::SearchRegex => Message::Search(SearchMessage::ToggleRegex),
        }
    }

    /// Whether this build offers the action. A build that can open every
    /// folder has nothing to grant, and macOS reads scans with its own text
    /// recognition, so the command list and the shortcuts dialog do not show
    /// either. The action itself stays, so a config that binds it still loads.
    pub fn is_offered(self) -> bool {
        self.is_offered_where(crate::ocr_models::system_reads_scans())
    }

    fn is_offered_where(self, system_reads_scans: bool) -> bool {
        match self {
            Self::GrantFolderAccess => crate::fs::sandbox_active(),
            Self::DownloadOcrModels => !system_reads_scans,
            Self::OpenTrashInFinder => cfg!(target_os = "macos"),
            _ => true,
        }
    }

    /// Whether a panel inside an archive may run the action. Only what takes
    /// neither the panel's path nor its selection as input for writing,
    /// launching, the config or Hermes; viewing is the exception, as it works
    /// on a copy, and so are copying in and deleting, which rewrite the
    /// archive. No catch-all arm: a new action has to be decided.
    #[cfg(test)]
    pub fn allowed_in_archive(self) -> bool {
        match self {
            Self::Quit
            | Self::GoUp
            | Self::Open
            | Self::SwitchPanel
            | Self::SwitchLanguage
            | Self::ToggleTag
            | Self::TagDown
            | Self::TagUp
            | Self::TagAll
            | Self::ClearTags
            | Self::AbortJob
            | Self::CopyPath
            | Self::CopyName
            | Self::CopyFiles
            | Self::Trash
            | Self::RestoreLastDeletion
            | Self::OpenTrashInFinder
            | Self::Paste
            | Self::Copy
            | Self::DriveMenuLeft
            | Self::DriveMenuRight
            | Self::HistoryBack
            | Self::HistoryForward
            | Self::Favourites
            | Self::SortByName
            | Self::SortByModified
            | Self::SortBySize
            | Self::ContextMenu
            | Self::CursorUp
            | Self::CursorDown
            | Self::PageUp
            | Self::PageDown
            | Self::First
            | Self::Last
            | Self::CommandLine
            | Self::CommandPalette
            | Self::FilingStatus
            | Self::FilingSetup
            | Self::Rules
            | Self::RuleSuggestions
            | Self::ReviewList
            | Self::SavedQueries
            | Self::ReorganisationPlan
            | Self::UndoReorganisation
            | Self::FetchTheme
            | Self::Settings
            | Self::KeyBindings
            | Self::About
            | Self::DownloadOcrModels
            | Self::DownloadAiModel
            | Self::GrantFolderAccess
            | Self::ConnectServer
            | Self::DisconnectServer
            | Self::SearchCase
            | Self::SearchWholeWord
            | Self::SearchRegex
            | Self::Delete
            | Self::DeletePermanently
            | Self::View => true,
            Self::Edit
            | Self::Move
            | Self::Mkdir
            | Self::NewFile
            | Self::Pack
            | Self::ComputeFolderSizes
            | Self::AddFavourite
            | Self::RemoveFavourite
            | Self::Search
            | Self::QuickOpen
            | Self::HermesQuickFix
            | Self::HermesHandToInbox
            | Self::HermesProcessAgain
            | Self::HermesSetInbox
            | Self::HermesSetFilingLocation
            | Self::HermesSetArchive
            | Self::HermesExclude
            | Self::HermesInspect => false,
        }
    }

    /// Whether a panel on an SFTP server may run the action. It is what an
    /// archive allows, less the command line, which runs on this computer, and
    /// plus the actions that connect and disconnect. No catch-all arm: a new
    /// action has to be decided.
    #[cfg(test)]
    pub fn allowed_on_server(self) -> bool {
        match self {
            Self::CommandLine => false,
            Self::Quit
            | Self::GoUp
            | Self::Open
            | Self::SwitchPanel
            | Self::SwitchLanguage
            | Self::ToggleTag
            | Self::TagDown
            | Self::TagUp
            | Self::TagAll
            | Self::ClearTags
            | Self::AbortJob
            | Self::CopyPath
            | Self::CopyName
            | Self::CopyFiles
            | Self::Trash
            | Self::RestoreLastDeletion
            | Self::OpenTrashInFinder
            | Self::Paste
            | Self::DriveMenuLeft
            | Self::DriveMenuRight
            | Self::HistoryBack
            | Self::HistoryForward
            | Self::Favourites
            | Self::SortByName
            | Self::SortByModified
            | Self::SortBySize
            | Self::ContextMenu
            | Self::CursorUp
            | Self::CursorDown
            | Self::PageUp
            | Self::PageDown
            | Self::First
            | Self::Last
            | Self::CommandPalette
            | Self::FilingStatus
            | Self::FilingSetup
            | Self::Rules
            | Self::RuleSuggestions
            | Self::ReviewList
            | Self::SavedQueries
            | Self::ReorganisationPlan
            | Self::UndoReorganisation
            | Self::FetchTheme
            | Self::Settings
            | Self::KeyBindings
            | Self::About
            | Self::DownloadOcrModels
            | Self::DownloadAiModel
            | Self::GrantFolderAccess
            | Self::ConnectServer
            | Self::DisconnectServer
            | Self::SearchCase
            | Self::SearchWholeWord
            | Self::SearchRegex
            | Self::View
            | Self::Copy => true,
            Self::Edit
            | Self::Move
            | Self::Delete
            | Self::DeletePermanently
            | Self::Mkdir
            | Self::NewFile
            | Self::Pack
            | Self::ComputeFolderSizes
            | Self::AddFavourite
            | Self::RemoveFavourite
            | Self::Search
            | Self::QuickOpen
            | Self::HermesQuickFix
            | Self::HermesHandToInbox
            | Self::HermesProcessAgain
            | Self::HermesSetInbox
            | Self::HermesSetFilingLocation
            | Self::HermesSetArchive
            | Self::HermesExclude
            | Self::HermesInspect => false,
        }
    }

    /// The translated hint the header shows. `None` for actions that are bound
    /// but not advertised, like the arrow keys: the header has room for five
    /// or six hints, not twenty.
    pub fn hint(self) -> Option<Msg> {
        match self {
            Self::CommandPalette => Some(Msg::ShortcutCommands),
            Self::Quit => Some(Msg::ShortcutQuit),
            Self::SwitchLanguage => Some(Msg::ShortcutSwitchLanguage),
            Self::View => Some(Msg::ShortcutView),
            Self::Edit => Some(Msg::ShortcutEdit),
            Self::Copy => Some(Msg::ShortcutCopy),
            Self::Move => Some(Msg::ShortcutMove),
            Self::Delete => Some(Msg::ShortcutDelete),
            Self::Mkdir => Some(Msg::ShortcutMkdir),
            Self::CommandLine => Some(Msg::ShortcutCommandLine),
            _ => None,
        }
    }
}

/// The keys this binary ships with, in the order that decides which one a
/// label shows. Adding a file operation means adding a `KeyAction` and its
/// keys here; the key handler and the hint list follow from it.
///
/// Built at runtime rather than as a `static` because `Key::Character` needs a
/// `SmolStr`, which is not a const constructor.
fn default_bindings() -> Vec<(KeyAction, Binding)> {
    use key::Named::*;
    use Key::{Character, Named as KeyNamed};
    use KeyAction as A;

    let mut bindings = vec![
        // --- Backspace, Enter, Tab and Cmd+Q are not on the function key bar ---
        // Not a bare Q: that letter belongs to type-ahead.
        (
            A::Quit,
            Binding::with_modifiers(Character("q".into()), Modifiers::COMMAND),
        ),
        (A::GoUp, Binding::key(KeyNamed(Backspace))),
        (A::Open, Binding::key(KeyNamed(Enter))),
        (A::SwitchPanel, Binding::key(KeyNamed(Tab))),
        (A::SwitchLanguage, Binding::key(KeyNamed(F9))),
        // --- selection: tagged rows, not advertised ---
        (A::ToggleTag, Binding::key(KeyNamed(Insert))),
        (A::ToggleTag, Binding::key(KeyNamed(Space))),
        (
            A::TagDown,
            Binding::with_modifiers(KeyNamed(ArrowDown), Modifiers::SHIFT),
        ),
        (
            A::TagUp,
            Binding::with_modifiers(KeyNamed(ArrowUp), Modifiers::SHIFT),
        ),
        (A::TagAll, Binding::key(Character("*".into()))),
        (
            A::ClearTags,
            Binding::with_modifiers(Character("*".into()), Modifiers::CTRL),
        ),
        // Cmd+C on a Mac, Ctrl+C elsewhere, as in the Finder and Explorer. A
        // Mac keeps Ctrl+C for stopping a job (see below); elsewhere Escape
        // stops it where free, else the cancel button in the progress line.
        (
            A::CopyFiles,
            Binding::with_modifiers(Character("c".into()), Modifiers::COMMAND),
        ),
        // Option+Cmd+C and Ctrl+Cmd+C on a Mac; Ctrl+Alt+C and Ctrl+Shift+C
        // elsewhere.
        (
            A::CopyPath,
            Binding::with_modifiers(Character("c".into()), Modifiers::ALT | Modifiers::COMMAND),
        ),
        (
            A::CopyName,
            Binding::with_modifiers(
                Character("c".into()),
                if cfg!(target_os = "macos") {
                    Modifiers::CTRL | Modifiers::COMMAND
                } else {
                    Modifiers::CTRL | Modifiers::SHIFT
                },
            ),
        ),
        // The files, picture or text of the system clipboard go into the active
        // panel.
        (
            A::Paste,
            Binding::with_modifiers(Character("v".into()), Modifiers::COMMAND),
        ),
        (A::View, Binding::key(KeyNamed(F3))),
        (A::Edit, Binding::key(KeyNamed(F4))),
        (A::Copy, Binding::key(KeyNamed(F5))),
        (A::Move, Binding::key(KeyNamed(F6))),
        (A::Delete, Binding::key(KeyNamed(F8))),
        // Not advertised: the header has no room, and a key that deletes for
        // good should be found in the help, not stumbled over.
        (
            A::DeletePermanently,
            Binding::with_modifiers(KeyNamed(F8), Modifiers::SHIFT),
        ),
        // Cmd+Z (Ctrl+Z elsewhere) puts back what F8 just moved to the trash,
        // as the Finder and Explorer undo a delete.
        (
            A::RestoreLastDeletion,
            Binding::with_modifiers(Character("z".into()), Modifiers::COMMAND),
        ),
        (A::Mkdir, Binding::key(KeyNamed(F7))),
        // Norton Commander's Shift+F4: edit a new file.
        (
            A::NewFile,
            Binding::with_modifiers(KeyNamed(F4), Modifiers::SHIFT),
        ),
        // Alt+F5 packs into an archive, as in Norton Commander.
        (
            A::Pack,
            Binding::with_modifiers(KeyNamed(F5), Modifiers::ALT),
        ),
        // Alt (Option on macOS) with F1 or F2 picks the drive of the left or
        // right panel, as in Norton Commander. Not advertised: the bar shows
        // bare keys only, and NC does not list these either.
        (
            A::DriveMenuLeft,
            Binding::with_modifiers(KeyNamed(F1), Modifiers::ALT),
        ),
        (
            A::DriveMenuRight,
            Binding::with_modifiers(KeyNamed(F2), Modifiers::ALT),
        ),
        // Alt+Left and Alt+Right walk the panel's folders back and forward, as
        // in a browser.
        (
            A::HistoryBack,
            Binding::with_modifiers(KeyNamed(ArrowLeft), Modifiers::ALT),
        ),
        (
            A::HistoryForward,
            Binding::with_modifiers(KeyNamed(ArrowRight), Modifiers::ALT),
        ),
        // Cmd+D (Ctrl+D elsewhere) bookmarks the folder, as in a browser.
        (
            A::AddFavourite,
            Binding::with_modifiers(Character("d".into()), Modifiers::COMMAND),
        ),
        (A::SortByName, sort_binding(SortColumn::Name)),
        (A::SortByModified, sort_binding(SortColumn::Modified)),
        (A::SortBySize, sort_binding(SortColumn::Size)),
        // Shift+F10 is the context menu key on Windows and Linux. Registered so
        // the exact chord wins over the fall-back to bare F10, which quits.
        (
            A::ContextMenu,
            Binding::with_modifiers(KeyNamed(F10), Modifiers::SHIFT),
        ),
        // Quit is also F10, the Norton Commander convention.
        (A::Quit, Binding::key(KeyNamed(F10))),
        // --- bound, not advertised: the header has no room for these ---
        (A::CursorUp, Binding::key(KeyNamed(ArrowUp))),
        (A::CursorDown, Binding::key(KeyNamed(ArrowDown))),
        (A::PageUp, Binding::key(KeyNamed(PageUp))),
        (A::PageDown, Binding::key(KeyNamed(PageDown))),
        // Cmd+Up/Down page on a Mac, where there is no PageUp key; `COMMAND`
        // is Ctrl elsewhere.
        (
            A::PageUp,
            Binding::with_modifiers(KeyNamed(ArrowUp), Modifiers::COMMAND),
        ),
        (
            A::PageDown,
            Binding::with_modifiers(KeyNamed(ArrowDown), Modifiers::COMMAND),
        ),
        (A::First, Binding::key(KeyNamed(Home))),
        (A::Last, Binding::key(KeyNamed(End))),
        // Alt+F7 is Norton Commander's file search. Cmd+Shift+F (Ctrl+Shift+F
        // elsewhere) is VS Code's search in files.
        (
            A::Search,
            Binding::with_modifiers(KeyNamed(F7), Modifiers::ALT),
        ),
        (
            A::Search,
            Binding::with_modifiers(Character("f".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // VS Code's Go to File.
        (
            A::QuickOpen,
            Binding::with_modifiers(Character("p".into()), Modifiers::COMMAND),
        ),
        // F2 was Norton Commander's user menu, the key that ran user commands;
        // Alt+F2 stays the right drive menu.
        (A::CommandLine, Binding::key(KeyNamed(F2))),
        // VS Code's Command Palette; F1 opens it there too.
        (
            A::CommandPalette,
            Binding::with_modifiers(Character("p".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        (A::CommandPalette, Binding::key(KeyNamed(F1))),
        // The settings tab; Cmd+, (Ctrl+, elsewhere) is the usual key for preferences.
        (
            A::Settings,
            Binding::with_modifiers(Character(",".into()), Modifiers::COMMAND),
        ),
        // The keyboard shortcuts, K for keys; Cmd+K, Ctrl+K elsewhere.
        (
            A::KeyBindings,
            Binding::with_modifiers(Character("k".into()), Modifiers::COMMAND),
        ),
        // The filing status page; Cmd+Shift+D, Ctrl+Shift+D elsewhere.
        (
            A::FilingStatus,
            Binding::with_modifiers(Character("d".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The filing setup, L for locations; Cmd+Shift+L, Ctrl+Shift+L elsewhere.
        (
            A::FilingSetup,
            Binding::with_modifiers(Character("l".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The rules, B for bureaucratic regulations; Cmd+Shift+B, Ctrl+Shift+B elsewhere.
        (
            A::Rules,
            Binding::with_modifiers(Character("b".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The rule suggestions, R for requisitions; Cmd+Shift+R, Ctrl+Shift+R elsewhere.
        (
            A::RuleSuggestions,
            Binding::with_modifiers(Character("r".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The review list, V for verify; Cmd+Shift+V, Ctrl+Shift+V elsewhere.
        (
            A::ReviewList,
            Binding::with_modifiers(Character("v".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The quick fixes of an inquiry, as in VS Code and Xcode; Cmd+. (Ctrl+. elsewhere).
        (
            A::HermesQuickFix,
            Binding::with_modifiers(Character(".".into()), Modifiers::COMMAND),
        ),
        // Process the file under the cursor again, H for Hermes; Cmd+Shift+H,
        // Ctrl+Shift+H elsewhere. Cmd+Shift+R already opens the rule suggestions.
        (
            A::HermesProcessAgain,
            Binding::with_modifiers(Character("h".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The saved queries, A for abfragen; Cmd+Shift+A, Ctrl+Shift+A elsewhere.
        (
            A::SavedQueries,
            Binding::with_modifiers(Character("a".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The reorganisation plan, I for inventory; Cmd+Shift+I, Ctrl+Shift+I elsewhere.
        (
            A::ReorganisationPlan,
            Binding::with_modifiers(Character("i".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // Undo of the last reorganisation, U for undo; Cmd+Shift+U, Ctrl+Shift+U elsewhere.
        (
            A::UndoReorganisation,
            Binding::with_modifiers(Character("u".into()), Modifiers::COMMAND | Modifiers::SHIFT),
        ),
        // The toggles of the search fields, as in VS Code. On a Mac Option types
        // other characters, so these are only found through the physical key.
        (
            A::SearchCase,
            Binding::with_modifiers(Character("c".into()), Modifiers::ALT),
        ),
        (
            A::SearchWholeWord,
            Binding::with_modifiers(Character("w".into()), Modifiers::ALT),
        ),
        (
            A::SearchRegex,
            Binding::with_modifiers(Character("r".into()), Modifiers::ALT),
        ),
    ];
    // Ctrl is not Cmd on a Mac, so Ctrl+C can keep stopping a job there;
    // elsewhere Ctrl+C is copy_files.
    if cfg!(target_os = "macos") {
        bindings.push((
            A::AbortJob,
            Binding::with_modifiers(Character("c".into()), Modifiers::CTRL),
        ));
    }
    // Safari's and Finder's back and forward; elsewhere Alt+arrows are enough.
    if cfg!(target_os = "macos") {
        bindings.push((
            A::HistoryBack,
            Binding::with_modifiers(Character("[".into()), Modifiers::COMMAND),
        ));
        bindings.push((
            A::HistoryForward,
            Binding::with_modifiers(Character("]".into()), Modifiers::COMMAND),
        ));
    }
    bindings
}

/// Ctrl+F3, Ctrl+F5 and Ctrl+F6 as in Norton Commander. A Mac takes Ctrl+F-keys
/// for itself, so there it is Cmd+1 (name), Cmd+2 (size) and Cmd+3 (modified).
pub fn sort_binding(column: SortColumn) -> Binding {
    use key::Named::{F3, F5, F6};
    if cfg!(target_os = "macos") {
        let digit = match column {
            SortColumn::Name => "1",
            SortColumn::Size => "2",
            SortColumn::Modified => "3",
        };
        Binding::with_modifiers(Key::Character(digit.into()), Modifiers::COMMAND)
    } else {
        let function_key = match column {
            SortColumn::Name => F3,
            SortColumn::Modified => F5,
            SortColumn::Size => F6,
        };
        Binding::with_modifiers(Key::Named(function_key), Modifiers::CTRL)
    }
}

/// More keys than anyone binds to one action; a bound on what a config file
/// can make the lookup walk through.
pub const MAX_KEYS_PER_ACTION: usize = 16;

/// Invisible formatting characters (Unicode category Cf): a key written with
/// one would look like another in the error line and the status bar.
fn is_format_character(c: char) -> bool {
    matches!(c,
        '\u{AD}'
        | '\u{600}'..='\u{605}'
        | '\u{61C}'
        | '\u{6DD}'
        | '\u{70F}'
        | '\u{180E}'
        | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{206F}'
        | '\u{FEFF}'
        | '\u{FFF9}'..='\u{FFFB}')
}

/// Why `[keys]` cannot be applied, and where in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapError {
    pub span: Range<usize>,
    pub reason: String,
}

/// Which keys do what: the built-in table, or that table with the `[keys]`
/// section of the config applied.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keymap {
    bindings: Vec<(KeyAction, Binding)>,
}

impl Default for Keymap {
    fn default() -> Self {
        (*Self::built_in()).clone()
    }
}

impl Keymap {
    /// The built-in table, built once on first use.
    ///
    /// `map_key` runs on every key press, and a binding owns a `Key`, which
    /// owns a `SmolStr`. Building the list per press meant an allocation per
    /// key repeat; holding a cursor key down made the UI visibly lag behind
    /// the keystroke.
    pub fn built_in() -> Arc<Self> {
        static BUILT_IN: OnceLock<Arc<Keymap>> = OnceLock::new();
        Arc::clone(BUILT_IN.get_or_init(|| {
            Arc::new(Keymap {
                bindings: default_bindings(),
            })
        }))
    }

    /// The built-in table with the actions named in `overrides` rebound. A
    /// named action loses all its built-in keys and gets exactly the listed
    /// ones; `[]` leaves it without a key. Two actions on one key are an
    /// error when both work in the same area (panel or search), also when the
    /// other one only has its built-in key: silently letting one win would
    /// hide the other action without a word. The exception is a built-in key
    /// that an older config may already use for something else
    /// ([`KeyAction::yields_its_built_in_key`]): the config wins and the
    /// action keeps the command list only.
    pub fn with_overrides(overrides: &KeySection) -> Result<Self, KeymapError> {
        let mut rebound: Vec<KeyAction> = Vec::new();
        for entry in &overrides.entries {
            if let Some(action) = KeyAction::from_name(entry.action.as_ref()) {
                rebound.push(action);
            }
        }

        let mut taken: Vec<(KeyAction, Binding)> = default_bindings()
            .into_iter()
            .filter(|(action, _)| !rebound.contains(action))
            .collect();
        let mut configured: Vec<(KeyAction, Vec<Binding>)> = Vec::new();
        let mut yielded: Vec<(KeyAction, Binding)> = Vec::new();
        for entry in &overrides.entries {
            let name = entry.action.as_ref();
            let action = KeyAction::from_name(name).ok_or_else(|| KeymapError {
                span: entry.action.span(),
                reason: format!("unknown action {name:?}"),
            })?;
            let mut bindings = Vec::new();
            for (index, chord) in entry.chords.iter().enumerate() {
                if index >= MAX_KEYS_PER_ACTION {
                    return Err(KeymapError {
                        span: chord.span(),
                        reason: format!("at most {MAX_KEYS_PER_ACTION} keys per action"),
                    });
                }
                let text = chord.as_ref().trim();
                let binding = parse_chord(text).map_err(|reason| KeymapError {
                    span: chord.span(),
                    reason,
                })?;
                if bindings.contains(&binding) {
                    return Err(KeymapError {
                        span: chord.span(),
                        reason: format!("{text:?} is listed twice for {name}"),
                    });
                }
                if !action.works_in(KeyScope::Panel) && is_fixed_in_search(&binding) {
                    return Err(KeymapError {
                        span: chord.span(),
                        reason: format!("{text:?} is fixed in the search view"),
                    });
                }
                let holder = taken
                    .iter()
                    .find(|(other, held)| *held == binding && shares_a_scope(*other, action))
                    .map(|(other, _)| *other);
                if let Some(other) = holder
                    .filter(|other| !rebound.contains(other) && other.yields_its_built_in_key())
                {
                    taken
                        .retain(|(held_action, held)| !(*held_action == other && *held == binding));
                    yielded.push((other, binding.clone()));
                } else if let Some(other) = holder {
                    let advice = if rebound.contains(&other) {
                        String::new()
                    } else {
                        format!("; give {} other keys too, or []", other.name())
                    };
                    return Err(KeymapError {
                        span: chord.span(),
                        reason: format!("{text:?} is already bound to {}{advice}", other.name()),
                    });
                }
                taken.push((action, binding.clone()));
                bindings.push(binding);
            }
            configured.push((action, bindings));
        }

        let mut bindings = Vec::new();
        let mut placed: Vec<KeyAction> = Vec::new();
        for (action, binding) in default_bindings() {
            if !rebound.contains(&action) {
                if !yielded.contains(&(action, binding.clone())) {
                    bindings.push((action, binding));
                }
            } else if !placed.contains(&action) {
                placed.push(action);
                for (configured_action, keys) in &configured {
                    if *configured_action == action {
                        bindings.extend(keys.iter().map(|key| (action, key.clone())));
                    }
                }
            }
        }
        // An action without a built-in key (on this platform) is not met in
        // the loop above; its configured keys go at the end.
        for (action, keys) in &configured {
            if !placed.contains(action) {
                bindings.extend(keys.iter().map(|key| (*action, key.clone())));
            }
        }
        Ok(Self { bindings })
    }

    /// The keys of `action`, the first-listed first.
    pub fn bindings_of(&self, action: KeyAction) -> impl Iterator<Item = &Binding> {
        self.bindings
            .iter()
            .filter(move |(bound, _)| *bound == action)
            .map(|(_, binding)| binding)
    }

    /// Maps a key press to a message, among the actions that work in `scope`.
    pub fn map_key(&self, key: &Key, modifiers: Modifiers, scope: KeyScope) -> Option<Message> {
        self.bindings
            .iter()
            .find(|(action, binding)| {
                action.works_in(scope) && binding.key == *key && binding.modifiers == modifiers
            })
            .map(|(action, _)| action.message())
    }

    /// The key that sends `message`, spelled for this platform: `F5`, `⇧F8`,
    /// `⌥⌘C` on a Mac, `Ctrl+Alt+C` elsewhere. `None` for a message no key
    /// sends.
    pub fn shortcut_label(&self, message: &Message) -> Option<String> {
        let (_, binding) = self
            .bindings
            .iter()
            .find(|(action, _)| action.message() == *message)?;
        binding_label(binding)
    }

    /// Whether `action` has exactly the keys it ships with, in that order.
    pub fn has_built_in_keys(&self, action: KeyAction) -> bool {
        self.bindings_of(action)
            .eq(built_in_bindings_of(action).iter())
    }

    /// The other action that `binding` would collide with if `action` took
    /// it: one that holds it and works in an area `action` works in too.
    pub fn holder_of(&self, action: KeyAction, binding: &Binding) -> Option<KeyAction> {
        self.bindings
            .iter()
            .find(|(other, held)| {
                *other != action && held == binding && shares_a_scope(*other, action)
            })
            .map(|(other, _)| *other)
    }

    /// The labels of the function key bar, derived from the bindings: slot
    /// `n - 1` holds the hint of the bare `Fn` binding, `None` for a free key.
    /// Only bare function keys are shown; Shift+F8 and friends are in the help.
    /// Actions that do not work in `scope` leave their slot free.
    pub fn function_keys(
        &self,
        lang: Language,
        scope: KeyScope,
    ) -> [Option<&'static str>; FUNCTION_KEY_COUNT] {
        let mut slots = [None; FUNCTION_KEY_COUNT];
        for (action, binding) in &self.bindings {
            if !action.works_in(scope) {
                continue;
            }
            let Some(hint) = action.hint() else {
                continue;
            };
            if binding.modifiers != Modifiers::default() {
                continue;
            }
            let Some(number) = function_key_number(&binding.key) else {
                continue;
            };
            if let Some(slot) = slots.get_mut(number - 1) {
                slot.get_or_insert(lang.text(hint));
            }
        }
        slots
    }
}

/// Slots in the function key bar: F1 to F10.
pub const FUNCTION_KEY_COUNT: usize = 10;

/// A key spelled for this platform: `F5`, `⇧F8`, `⌥⌘C` on a Mac,
/// `Ctrl+Alt+C` elsewhere. `None` for a key that has no printable name.
pub fn binding_label(binding: &Binding) -> Option<String> {
    let Binding { key, modifiers } = binding;
    let key_name = match key {
        Key::Named(named) => format!("{named:?}"),
        Key::Character(character) => character.to_uppercase(),
        Key::Unidentified => return None,
    };
    let mut label = String::new();
    if cfg!(target_os = "macos") {
        for (held, symbol) in [
            (modifiers.control(), "⌃"),
            (modifiers.alt(), "⌥"),
            (modifiers.shift(), "⇧"),
            (modifiers.logo(), "⌘"),
        ] {
            if held {
                label.push_str(symbol);
            }
        }
        label.push_str(&key_name);
    } else {
        for (held, name) in [
            (modifiers.control(), "Ctrl"),
            (modifiers.alt(), "Alt"),
            (modifiers.shift(), "Shift"),
            (modifiers.logo(), "Super"),
        ] {
            if held {
                label.push_str(name);
                label.push('+');
            }
        }
        label.push_str(&key_name);
    }
    Some(label)
}

/// The keys `action` ships with, the first-listed first.
pub fn built_in_bindings_of(action: KeyAction) -> Vec<Binding> {
    default_bindings()
        .into_iter()
        .filter(|(bound, _)| *bound == action)
        .map(|(_, binding)| binding)
        .collect()
}

/// The binding a key pressed to be recorded stands for, with the Shift rules
/// the lookup applies: a letter keeps Shift and is written in lower case, any
/// other character already carries Shift in itself (`*` is Shift+8), a named
/// key keeps it (Shift+F8 is not F8). `None` while only a modifier is down.
pub fn recorded_binding(key: &Key, modifiers: Modifiers) -> Option<Binding> {
    use key::Named;
    match key {
        Key::Named(
            Named::Shift
            | Named::Control
            | Named::Alt
            | Named::AltGraph
            | Named::Super
            | Named::Meta
            | Named::Hyper
            | Named::Fn
            | Named::FnLock
            | Named::CapsLock
            | Named::NumLock
            | Named::ScrollLock
            | Named::Symbol
            | Named::SymbolLock,
        )
        | Key::Unidentified => None,
        Key::Named(_) => Some(Binding::with_modifiers(key.clone(), modifiers)),
        Key::Character(typed) => {
            let held = if typed.chars().all(char::is_alphabetic) {
                modifiers
            } else {
                modifiers - Modifiers::SHIFT
            };
            Some(Binding::with_modifiers(
                Key::Character(typed.to_lowercase().into()),
                held,
            ))
        }
    }
}

/// Why a recorded key cannot be written to `[keys]` for an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRefusal {
    /// The key has no name in the config file, e.g. F13 or a media key.
    Unnamed,
    /// A letter or digit without Ctrl, Alt or Cmd would shadow type-ahead.
    NeedsModifier,
    /// Escape closes dialogs and menus and stops jobs.
    Escape,
    /// The search view handles the key itself.
    FixedInSearch,
}

/// The text `[keys]` stores for `binding` when `action` takes it, checked
/// against the rules [`Keymap::with_overrides`] applies to a single key: the
/// text reads back as exactly this binding.
pub fn configurable_chord(action: KeyAction, binding: &Binding) -> Result<String, KeyRefusal> {
    if binding.key == Key::Named(key::Named::Escape) {
        return Err(KeyRefusal::Escape);
    }
    let text = chord_text(binding).ok_or(KeyRefusal::Unnamed)?;
    match parse_chord(&text) {
        Ok(parsed) if parsed == *binding => {}
        Ok(_) => return Err(KeyRefusal::Unnamed),
        Err(_) => {
            let types_a_name = matches!(&binding.key, Key::Character(character)
                if character.chars().all(char::is_alphanumeric));
            return Err(if types_a_name {
                KeyRefusal::NeedsModifier
            } else {
                KeyRefusal::Unnamed
            });
        }
    }
    if !action.works_in(KeyScope::Panel) && is_fixed_in_search(binding) {
        return Err(KeyRefusal::FixedInSearch);
    }
    Ok(text)
}

/// `binding` as `[keys]` writes it, the inverse of [`parse_chord`]: the
/// platform's command modifier is `primary`, so the file means the same key
/// on a Mac and elsewhere.
fn chord_text(binding: &Binding) -> Option<String> {
    use key::Named;
    let mut parts: Vec<String> = Vec::new();
    let mut rest = binding.modifiers;
    if rest.contains(Modifiers::COMMAND) {
        parts.push("primary".to_owned());
        rest -= Modifiers::COMMAND;
    }
    for (modifier, name) in [
        (Modifiers::CTRL, "ctrl"),
        (Modifiers::ALT, "alt"),
        (Modifiers::SHIFT, "shift"),
        (Modifiers::LOGO, "super"),
    ] {
        if rest.contains(modifier) {
            parts.push(name.to_owned());
        }
    }
    let key_name = match &binding.key {
        Key::Character(character) if character.as_str() == "+" => "plus".to_owned(),
        Key::Character(character) => character.to_string(),
        Key::Named(named) => match named {
            Named::F1 => "f1",
            Named::F2 => "f2",
            Named::F3 => "f3",
            Named::F4 => "f4",
            Named::F5 => "f5",
            Named::F6 => "f6",
            Named::F7 => "f7",
            Named::F8 => "f8",
            Named::F9 => "f9",
            Named::F10 => "f10",
            Named::F11 => "f11",
            Named::F12 => "f12",
            Named::Enter => "enter",
            Named::Tab => "tab",
            Named::Backspace => "backspace",
            Named::Space => "space",
            Named::Insert => "insert",
            Named::Delete => "delete",
            Named::Home => "home",
            Named::End => "end",
            Named::PageUp => "pageup",
            Named::PageDown => "pagedown",
            Named::ArrowUp => "up",
            Named::ArrowDown => "down",
            Named::ArrowLeft => "left",
            Named::ArrowRight => "right",
            _ => return None,
        }
        .to_owned(),
        Key::Unidentified => return None,
    };
    parts.push(key_name);
    Some(parts.join("+"))
}

/// The number of a bare function key, 1 to 10.
fn function_key_number(key: &Key) -> Option<usize> {
    use key::Named::*;
    let Key::Named(named) = key else {
        return None;
    };
    [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10]
        .iter()
        .position(|candidate| candidate == named)
        .map(|index| index + 1)
}

/// Reads one key of the config file: `f5`, `ctrl+alt+p`, `primary+q`.
///
/// Parts are separated by `+`, case does not matter, the last one is the key
/// and the ones before it are modifiers. `primary` is Cmd on a Mac and Ctrl
/// elsewhere. A literal plus is written `plus`.
fn parse_chord(text: &str) -> Result<Binding, String> {
    if text.is_empty() {
        return Err("a key is empty".to_owned());
    }
    let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let key_part = parts.pop().unwrap_or_default();
    let mut modifiers = Modifiers::default();
    for part in parts {
        let modifier = match part.to_lowercase().as_str() {
            "primary" => Modifiers::COMMAND,
            "ctrl" | "control" => Modifiers::CTRL,
            "alt" | "option" => Modifiers::ALT,
            "shift" => Modifiers::SHIFT,
            "cmd" | "super" => Modifiers::LOGO,
            _ => return Err(format!("unknown modifier {part:?} in {text:?}")),
        };
        if modifiers.intersects(modifier) {
            return Err(format!("modifier {part:?} is given twice in {text:?}"));
        }
        modifiers |= modifier;
    }
    let key = parse_key_name(key_part)
        .ok_or_else(|| format!("unknown key {key_part:?} in {text:?}"))??;
    if let Key::Character(character) = &key {
        let is_letter = character.chars().all(char::is_alphabetic);
        if modifiers.shift() && !is_letter {
            return Err(
                "shift changes the character; write the character it types, e.g. \"*\"".to_owned(),
            );
        }
        let types_a_name = character.chars().all(char::is_alphanumeric);
        if types_a_name && !modifiers.intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::LOGO)
        {
            return Err("letters and digits need ctrl, alt or cmd: without them they jump to a file name (type-ahead)".to_owned());
        }
    }
    Ok(Binding::with_modifiers(key, modifiers))
}

/// The key a name stands for. `None` for a name that is not a key; `Some(Err)`
/// for one that is, but is not allowed.
fn parse_key_name(name: &str) -> Option<Result<Key, String>> {
    use key::Named;
    let lower = name.to_lowercase();
    let named = match lower.as_str() {
        "escape" | "esc" => {
            return Some(Err(
                "Escape is reserved: it closes dialogs and menus and stops jobs".to_owned(),
            ))
        }
        "f1" => Named::F1,
        "f2" => Named::F2,
        "f3" => Named::F3,
        "f4" => Named::F4,
        "f5" => Named::F5,
        "f6" => Named::F6,
        "f7" => Named::F7,
        "f8" => Named::F8,
        "f9" => Named::F9,
        "f10" => Named::F10,
        "f11" => Named::F11,
        "f12" => Named::F12,
        "enter" => Named::Enter,
        "tab" => Named::Tab,
        "backspace" => Named::Backspace,
        "space" => Named::Space,
        "insert" => Named::Insert,
        "delete" => Named::Delete,
        "home" => Named::Home,
        "end" => Named::End,
        "pageup" => Named::PageUp,
        "pagedown" => Named::PageDown,
        "up" => Named::ArrowUp,
        "down" => Named::ArrowDown,
        "left" => Named::ArrowLeft,
        "right" => Named::ArrowRight,
        "plus" => return Some(Ok(Key::Character("+".into()))),
        _ => {
            let mut chars = name.chars();
            return match (chars.next(), chars.next()) {
                (Some(only), None)
                    if !only.is_whitespace()
                        && !only.is_control()
                        && !is_format_character(only) =>
                {
                    Some(Ok(Key::Character(lower.as_str().into())))
                }
                _ => None,
            };
        }
    };
    Some(Ok(Key::Named(named)))
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    #[test]
    fn the_ocr_download_is_offered_only_where_the_system_does_not_read_scans() {
        assert!(KeyAction::DownloadOcrModels.is_offered_where(false));
        assert!(!KeyAction::DownloadOcrModels.is_offered_where(true));
        assert!(KeyAction::Quit.is_offered_where(true));
    }

    #[test]
    fn the_trash_actions_are_offered_only_where_they_work() {
        let on_a_mac = cfg!(target_os = "macos");
        assert_eq!(KeyAction::OpenTrashInFinder.is_offered(), on_a_mac);
        assert!(KeyAction::Trash.is_offered());
        assert!(KeyAction::RestoreLastDeletion.is_offered());
    }

    #[test]
    fn restoring_the_last_deletion_is_on_primary_z_and_yields_it_to_a_config() {
        let keys: Vec<_> = default_bindings()
            .into_iter()
            .filter(|(action, _)| *action == KeyAction::RestoreLastDeletion)
            .map(|(_, binding)| binding)
            .collect();
        assert_eq!(
            keys,
            [Binding::with_modifiers(
                Key::Character("z".into()),
                Modifiers::COMMAND
            )]
        );
        assert!(KeyAction::RestoreLastDeletion.yields_its_built_in_key());
        assert!(!KeyAction::Trash.yields_its_built_in_key());
        assert_eq!(
            KeyAction::from_name("restore_last_deletion"),
            Some(KeyAction::RestoreLastDeletion)
        );
    }

    use super::*;
    use crate::config;
    use iced::keyboard::key::Named;

    fn map_key(key: Key, modifiers: Modifiers) -> Option<Message> {
        Keymap::built_in().map_key(&key, modifiers, KeyScope::Panel)
    }

    fn function_keys(lang: Language) -> [Option<&'static str>; FUNCTION_KEY_COUNT] {
        Keymap::built_in().function_keys(lang, KeyScope::Panel)
    }

    fn shortcut_label(message: &Message) -> Option<String> {
        Keymap::built_in().shortcut_label(message)
    }

    fn written(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    /// The keymap `[keys]` text produces, or the line and reason it fails with.
    fn keymap_from(keys: &str) -> Result<Keymap, (Option<usize>, String)> {
        let (_dir, path) = written(keys);
        config::load(&path)
            .map(|config| (*config.keymap).clone())
            .map_err(|err| (err.line(), err.reason().to_owned()))
    }

    fn rebound(keys: &str) -> Keymap {
        keymap_from(keys).unwrap()
    }

    fn failure(keys: &str) -> (Option<usize>, String) {
        keymap_from(keys).unwrap_err()
    }

    fn press(keymap: &Keymap, key: Key, modifiers: Modifiers) -> Option<Message> {
        keymap.map_key(&key, modifiers, KeyScope::Panel)
    }

    fn named(key: Named) -> Key {
        Key::Named(key)
    }

    #[test]
    fn alt_f7_and_the_find_chord_open_the_search_and_f7_alone_still_makes_a_directory() {
        let find = Modifiers::COMMAND | Modifiers::SHIFT;
        assert_eq!(
            map_key(named(Named::F7), Modifiers::ALT),
            Some(Message::OpenSearch)
        );
        assert_eq!(
            map_key(Key::Character("f".into()), find),
            Some(Message::OpenSearch)
        );
        assert_eq!(
            map_key(named(Named::F7), Modifiers::default()),
            Some(Message::CreateDirPrompt)
        );
        assert!(shortcut_label(&Message::OpenSearch).is_some());
    }

    #[test]
    fn primary_p_opens_quick_open_in_the_panel_and_not_in_the_search() {
        let message = Some(Message::QuickOpen(QuickOpenMessage::Open));
        assert_eq!(
            map_key(Key::Character("p".into()), Modifiers::COMMAND),
            message
        );
        let keymap = Keymap::default();
        assert_eq!(
            keymap.map_key(
                &Key::Character("p".into()),
                Modifiers::COMMAND,
                KeyScope::Search
            ),
            None
        );
        assert!(shortcut_label(&Message::QuickOpen(QuickOpenMessage::Open)).is_some());
    }

    #[test]
    fn primary_shift_l_toggles_the_filing_setup_in_the_panel_and_can_be_rebound() {
        let message = Some(Message::FilingSetup(FilingSetupMessage::Toggle));
        assert_eq!(
            map_key(
                Key::Character("l".into()),
                Modifiers::COMMAND | Modifiers::SHIFT
            ),
            message
        );
        assert_eq!(
            Keymap::default().map_key(
                &Key::Character("l".into()),
                Modifiers::COMMAND | Modifiers::SHIFT,
                KeyScope::Search
            ),
            None
        );
        let keymap = rebound("[keys]\nfiling_setup = \"F11\"\n");
        assert_eq!(
            press(&keymap, named(Named::F11), Modifiers::default()),
            message
        );
        assert_eq!(
            press(
                &keymap,
                Key::Character("l".into()),
                Modifiers::COMMAND | Modifiers::SHIFT
            ),
            None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cmd_1_2_3_sort_by_name_size_modified() {
        for (digit, column) in [
            ("1", SortColumn::Name),
            ("2", SortColumn::Size),
            ("3", SortColumn::Modified),
        ] {
            assert_eq!(
                map_key(Key::Character(digit.into()), Modifiers::COMMAND),
                Some(Message::SortActive(column))
            );
            assert_eq!(
                map_key(Key::Character(digit.into()), Modifiers::default()),
                None
            );
        }
        assert_eq!(map_key(Key::Named(Named::F3), Modifiers::CTRL), None);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn ctrl_f3_f5_f6_sort_by_name_modified_size() {
        for (function_key, column) in [
            (Named::F3, SortColumn::Name),
            (Named::F5, SortColumn::Modified),
            (Named::F6, SortColumn::Size),
        ] {
            assert_eq!(
                map_key(Key::Named(function_key), Modifiers::CTRL),
                Some(Message::SortActive(column))
            );
        }
        assert_eq!(
            map_key(Key::Character("1".into()), Modifiers::COMMAND),
            None
        );
    }

    /// The bare function keys keep their meaning whatever the sort chords are.
    #[test]
    fn the_sort_chords_leave_the_bare_keys_alone() {
        assert_eq!(
            map_key(Key::Named(Named::F5), Modifiers::default()),
            Some(Message::Transfer(TransferKind::Copy))
        );
    }

    /// The point of this module: a label on the bar whose key does nothing, or
    /// a binding the bar never shows. Both used to be possible.
    #[test]
    fn every_labelled_slot_has_a_binding_that_responds() {
        for (index, label) in function_keys(Language::English).iter().enumerate() {
            let Some(text) = label else { continue };
            let number = index + 1;
            let bound = Keymap::built_in().bindings.iter().any(|(action, binding)| {
                action.hint().is_some()
                    && binding.modifiers == Modifiers::default()
                    && function_key_number(&binding.key) == Some(number)
            });
            assert!(bound, "F{number} is labelled {text:?} with no binding");
        }
    }

    #[test]
    fn the_bar_shows_the_function_keys_and_nothing_else() {
        let en = function_keys(Language::English);
        assert_eq!(
            en,
            [
                Some("Commands"),
                Some("Cmd line"),
                Some("View"),
                Some("Edit"),
                Some("Copy"),
                Some("Move"),
                Some("Mkdir"),
                Some("Delete"),
                Some("Lang"),
                Some("Quit"),
            ]
        );
    }

    #[test]
    fn shift_f8_is_bound_but_has_no_slot() {
        assert_eq!(
            map_key(Key::Named(Named::F8), Modifiers::SHIFT),
            Some(Message::Delete { permanent: true })
        );
        assert_eq!(function_keys(Language::English)[7], Some("Delete"));
    }

    #[test]
    fn the_labels_are_translated_and_fit_a_slot() {
        let en = function_keys(Language::English);
        let de = function_keys(Language::German);
        assert_eq!(de[7], Some("Löschen"));
        for (en_label, de_label) in en.iter().zip(de.iter()) {
            assert_eq!(en_label.is_some(), de_label.is_some());
            for label in [en_label, de_label].into_iter().flatten() {
                assert!(
                    label.chars().count() <= 8,
                    "{label:?} is too long for a slot"
                );
            }
        }
    }

    #[test]
    fn bindings_produce_the_expected_messages() {
        assert_eq!(
            map_key(Key::Named(Named::Enter), Modifiers::default()),
            Some(Message::OpenSelected)
        );
        assert_eq!(
            map_key(Key::Named(Named::F10), Modifiers::default()),
            Some(Message::Quit)
        );
        assert_eq!(
            map_key(Key::Character("q".into()), Modifiers::COMMAND),
            Some(Message::Quit)
        );
        assert_eq!(
            map_key(Key::Character("q".into()), Modifiers::default()),
            None,
            "a bare q is type-ahead now"
        );
        assert_eq!(
            map_key(Key::Character("x".into()), Modifiers::default()),
            None
        );
    }

    #[test]
    fn alt_f1_and_alt_f2_open_the_drive_menu_of_their_panel() {
        assert_eq!(
            map_key(Key::Named(Named::F1), Modifiers::ALT),
            Some(Message::VolumeMenu(PanelSide::Left))
        );
        assert_eq!(
            map_key(Key::Named(Named::F2), Modifiers::ALT),
            Some(Message::VolumeMenu(PanelSide::Right))
        );
        assert_eq!(map_key(Key::Named(Named::F2), Modifiers::SHIFT), None);
    }

    #[test]
    fn bare_f2_focuses_the_command_line_in_the_panel_and_not_in_the_search() {
        assert_eq!(
            map_key(Key::Named(Named::F2), Modifiers::default()),
            Some(Message::CommandLine(CommandLineMessage::Focus))
        );
        assert_eq!(
            Keymap::default().map_key(
                &Key::Named(Named::F2),
                Modifiers::default(),
                KeyScope::Search
            ),
            None
        );
        assert_eq!(function_keys(Language::English)[1], Some("Cmd line"));
        assert_eq!(function_keys(Language::German)[1], Some("Kommando"));
    }

    #[test]
    fn an_older_config_that_puts_copy_on_f2_keeps_it() {
        let keymap = rebound("[keys]\ncopy = \"F2\"\n");
        assert_eq!(
            keymap.map_key(
                &Key::Named(Named::F2),
                Modifiers::default(),
                KeyScope::Panel
            ),
            Some(Message::Transfer(TransferKind::Copy))
        );
        assert_eq!(keymap.bindings_of(KeyAction::CommandLine).count(), 0);
        assert_eq!(
            keymap.shortcut_label(&KeyAction::CommandLine.message()),
            None
        );
        assert_eq!(
            keymap.function_keys(Language::English, KeyScope::Panel)[1],
            Some("Copy")
        );
    }

    #[test]
    fn an_older_config_keeps_the_keys_that_became_defaults_in_0_7() {
        let keymap = rebound(
            "[keys]\nmkdir = [\"F7\", \"primary+d\"]\ncopy = [\"F5\", \"shift+F4\", \"primary+[\"]\nmove = [\"F6\", \"alt+Left\", \"alt+Right\"]\n",
        );
        let in_panel = |key: Key, modifiers| keymap.map_key(&key, modifiers, KeyScope::Panel);

        assert_eq!(
            in_panel(Key::Character("d".into()), Modifiers::COMMAND),
            Some(Message::CreateDirPrompt)
        );
        assert_eq!(
            in_panel(Key::Named(Named::F4), Modifiers::SHIFT),
            Some(Message::Transfer(TransferKind::Copy))
        );
        assert_eq!(
            in_panel(Key::Character("[".into()), Modifiers::COMMAND),
            Some(Message::Transfer(TransferKind::Copy))
        );
        assert_eq!(
            in_panel(Key::Named(Named::ArrowLeft), Modifiers::ALT),
            Some(Message::Transfer(TransferKind::Move))
        );
        assert_eq!(keymap.bindings_of(KeyAction::AddFavourite).count(), 0);
        assert_eq!(keymap.bindings_of(KeyAction::NewFile).count(), 0);
        let built_in = Keymap::built_in();
        assert_eq!(
            keymap.bindings_of(KeyAction::HistoryForward).count(),
            built_in.bindings_of(KeyAction::HistoryForward).count() - 1
        );

        // History loses only the keys taken from it and keeps the rest of its
        // defaults, whichever the platform has.
        let taken = [
            Binding::with_modifiers(Key::Named(Named::ArrowLeft), Modifiers::ALT),
            Binding::with_modifiers(Key::Character("[".into()), Modifiers::COMMAND),
        ];
        let kept: Vec<&Binding> = built_in
            .bindings_of(KeyAction::HistoryBack)
            .filter(|binding| !taken.contains(binding))
            .collect();
        assert_eq!(
            keymap
                .bindings_of(KeyAction::HistoryBack)
                .collect::<Vec<_>>(),
            kept
        );
    }

    #[test]
    fn history_keeps_alt_left_when_its_other_key_is_taken() {
        let keymap = rebound("[keys]\ncopy = [\"F5\", \"primary+[\"]\n");
        assert_eq!(
            keymap.map_key(
                &Key::Named(Named::ArrowLeft),
                Modifiers::ALT,
                KeyScope::Panel
            ),
            Some(Message::HistoryBack)
        );
        assert_eq!(
            keymap.map_key(
                &Key::Character("[".into()),
                Modifiers::COMMAND,
                KeyScope::Panel
            ),
            Some(Message::Transfer(TransferKind::Copy))
        );
    }

    #[test]
    fn only_the_command_line_gives_its_built_in_key_away() {
        let (_, clash_with_view) = failure("[keys]\ncopy = \"F3\"\n");
        assert!(
            clash_with_view.contains("already bound to view"),
            "{clash_with_view}"
        );
        let (_, clash_on_f2) = failure("[keys]\ncopy = \"F2\"\nmove = \"F2\"\n");
        assert!(
            clash_on_f2.contains("already bound to copy"),
            "{clash_on_f2}"
        );
    }

    #[test]
    fn f1_opens_the_command_palette_and_labels_the_first_slot() {
        assert_eq!(
            map_key(Key::Named(Named::F1), Modifiers::default()),
            Some(Message::CommandPalette(CommandPaletteMessage::Toggle))
        );
        assert_eq!(
            map_key(
                Key::Character("p".into()),
                Modifiers::COMMAND | Modifiers::SHIFT
            ),
            Some(Message::CommandPalette(CommandPaletteMessage::Toggle))
        );
        assert_eq!(function_keys(Language::English)[0], Some("Commands"));
        assert_eq!(function_keys(Language::German)[0], Some("Befehle"));
    }

    #[test]
    fn a_key_section_that_used_the_free_f1_is_told_how_to_keep_it() {
        let (_, reason) = failure("[keys]\ncopy = \"F1\"\n");
        assert!(
            reason
                .contains("already bound to command_palette; give command_palette other keys too"),
            "{reason}"
        );
    }

    #[test]
    fn a_key_section_can_take_f1_from_the_command_palette() {
        let keymap = rebound("[keys]\ncommand_palette = \"primary+shift+p\"\ncopy = \"F1\"\n");
        assert_eq!(
            press(&keymap, named(Named::F1), Modifiers::default()),
            Some(Message::Transfer(TransferKind::Copy))
        );
        assert_eq!(
            keymap.function_keys(Language::English, KeyScope::Panel)[0],
            Some("Copy")
        );
    }

    #[test]
    fn shift_f10_opens_the_context_menu_and_does_not_quit() {
        assert_eq!(
            map_key(Key::Named(Named::F10), Modifiers::SHIFT),
            Some(Message::ContextMenuKey)
        );
        assert_eq!(
            map_key(Key::Named(Named::F10), Modifiers::default()),
            Some(Message::Quit)
        );
    }

    #[test]
    fn shortcuts_are_spelled_for_the_platform() {
        let label = |message: Message| shortcut_label(&message).unwrap();
        assert_eq!(label(Message::OpenSelected), "Enter");
        assert_eq!(label(Message::Transfer(TransferKind::Copy)), "F5");
        if cfg!(target_os = "macos") {
            assert_eq!(label(Message::Delete { permanent: true }), "⇧F8");
            assert_eq!(label(Message::CopyToClipboard(ClipboardKind::Path)), "⌥⌘C");
            assert_eq!(label(Message::CopyToClipboard(ClipboardKind::Name)), "⌃⌘C");
        } else {
            assert_eq!(label(Message::Delete { permanent: true }), "Shift+F8");
            assert_eq!(
                label(Message::CopyToClipboard(ClipboardKind::Path)),
                "Ctrl+Alt+C"
            );
            assert_eq!(
                label(Message::CopyToClipboard(ClipboardKind::Name)),
                "Ctrl+Shift+C"
            );
        }
        assert_eq!(shortcut_label(&Message::SwitchLanguage), Some("F9".into()));
        assert_eq!(shortcut_label(&Message::ContextMenuClose), None);
    }

    /// A modifier must actually gate the binding: Alt+Enter is not Enter.
    #[test]
    fn modifiers_gate_the_lookup() {
        assert_eq!(map_key(Key::Named(Named::Enter), Modifiers::ALT), None);
    }

    #[test]
    fn action_names_round_trip_and_are_unique() {
        for action in KeyAction::ALL {
            assert_eq!(KeyAction::from_name(action.name()), Some(*action));
        }
        let mut names: Vec<_> = KeyAction::ALL.iter().map(|action| action.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), KeyAction::ALL.len());
        assert_eq!(KeyAction::from_name("nope"), None);
    }

    #[test]
    fn paste_and_copy_files_are_on_the_command_chord() {
        let bindings = default_bindings();
        let paste: Vec<_> = bindings
            .iter()
            .filter(|(action, _)| *action == KeyAction::Paste)
            .map(|(_, binding)| binding)
            .collect();
        assert_eq!(
            paste,
            [&Binding::with_modifiers(
                Key::Character("v".into()),
                Modifiers::COMMAND
            )]
        );
        let copy_files: Vec<_> = bindings
            .iter()
            .filter(|(action, _)| *action == KeyAction::CopyFiles)
            .map(|(_, binding)| binding)
            .collect();
        assert_eq!(
            copy_files,
            [&Binding::with_modifiers(
                Key::Character("c".into()),
                Modifiers::COMMAND
            )]
        );
        assert_eq!(KeyAction::from_name("paste"), Some(KeyAction::Paste));
        assert_eq!(
            KeyAction::from_name("copy_files"),
            Some(KeyAction::CopyFiles)
        );
    }

    #[test]
    fn every_action_has_a_default_key_and_no_key_is_twice_bound() {
        // Reached through the command palette (or a menu) only.
        let mut without_key = vec![
            KeyAction::RemoveFavourite,
            KeyAction::Favourites,
            KeyAction::FetchTheme,
            KeyAction::About,
            KeyAction::DownloadOcrModels,
            KeyAction::DownloadAiModel,
            KeyAction::GrantFolderAccess,
            KeyAction::ConnectServer,
            KeyAction::DisconnectServer,
            KeyAction::Trash,
            KeyAction::OpenTrashInFinder,
            KeyAction::ComputeFolderSizes,
            KeyAction::HermesHandToInbox,
            KeyAction::HermesSetInbox,
            KeyAction::HermesSetFilingLocation,
            KeyAction::HermesSetArchive,
            KeyAction::HermesExclude,
            KeyAction::HermesInspect,
        ];
        // Ctrl+C is copy_files there; Escape and the cancel button stop a job.
        if !cfg!(target_os = "macos") {
            without_key.push(KeyAction::AbortJob);
        }
        let bindings = default_bindings();
        for action in KeyAction::ALL
            .iter()
            .filter(|action| !without_key.contains(action))
        {
            assert!(
                bindings.iter().any(|(bound, _)| bound == action),
                "{} has no default key",
                action.name()
            );
        }
        for (index, (_, binding)) in bindings.iter().enumerate() {
            assert!(
                !bindings[index + 1..]
                    .iter()
                    .any(|(_, other)| other == binding),
                "{binding:?} is bound twice"
            );
        }
    }

    #[test]
    fn a_config_can_give_primary_c_to_abort_job_and_copy_files_loses_its_key() {
        let keymap = rebound("[keys]\nabort_job = \"primary+c\"\n");
        assert_eq!(
            press(&keymap, Key::Character("c".into()), Modifiers::COMMAND),
            Some(Message::AbortJob)
        );
        assert_ne!(
            map_key(Key::Character("c".into()), Modifiers::COMMAND),
            Some(Message::AbortJob)
        );
        assert!(keymap
            .shortcut_label(&Message::CopyFilesToClipboard)
            .is_none());
    }

    #[test]
    fn an_action_without_a_built_in_key_takes_its_configured_one() {
        let keymap = rebound("[keys]\nabout = \"ctrl+alt+shift+b\"\n");
        assert_eq!(
            press(
                &keymap,
                Key::Character("b".into()),
                Modifiers::CTRL | Modifiers::ALT | Modifiers::SHIFT
            ),
            Some(KeyAction::About.message())
        );
    }

    /// A default the user could not write would make `[keys]` unable to say
    /// what the app does by itself, and a bare letter would shadow type-ahead.
    #[test]
    fn the_defaults_obey_the_rules_for_configured_keys() {
        for (action, binding) in default_bindings() {
            if let Key::Character(character) = &binding.key {
                let types_a_name = character.chars().all(char::is_alphanumeric);
                let has_chord_modifier = binding
                    .modifiers
                    .intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::LOGO);
                assert!(
                    !types_a_name || has_chord_modifier,
                    "{} would shadow type-ahead",
                    action.name()
                );
                assert!(
                    !binding.modifiers.shift() || character.chars().all(char::is_alphabetic),
                    "{} uses shift with a character it changes",
                    action.name()
                );
            }
            assert_ne!(binding.key, Key::Named(Named::Escape));
        }
    }

    #[test]
    fn a_rebound_action_answers_the_new_key_and_not_the_old_one() {
        let keymap = rebound("[keys]\ncommand_line = []\ncopy = \"F2\"\n");
        let copy = Some(Message::Transfer(TransferKind::Copy));
        assert_eq!(press(&keymap, named(Named::F2), Modifiers::default()), copy);
        assert_eq!(press(&keymap, named(Named::F5), Modifiers::default()), None);
        assert_eq!(
            keymap.shortcut_label(&Message::Transfer(TransferKind::Copy)),
            Some("F2".into())
        );
        assert_eq!(
            keymap.function_keys(Language::English, KeyScope::Panel)[..5],
            [
                Some("Commands"),
                Some("Copy"),
                Some("View"),
                Some("Edit"),
                None
            ]
        );
    }

    #[test]
    fn a_file_without_keys_leaves_the_built_in_keymap() {
        assert_eq!(
            rebound("[open]\nview = [\"viewer\"]\n"),
            *Keymap::built_in()
        );
        assert_eq!(rebound(""), *Keymap::built_in());
    }

    #[test]
    fn a_key_taken_from_an_unnamed_action_is_refused_at_its_line() {
        let (line, reason) = failure("[keys]\n\nview = \"F4\"\n");
        assert_eq!(line, Some(3));
        assert!(reason.contains("edit"), "{reason}");
    }

    #[test]
    fn swapping_two_keys_works_when_both_are_named() {
        let keymap = rebound("[keys]\nview = \"F4\"\nedit = \"F3\"\n");
        assert_eq!(
            press(&keymap, named(Named::F3), Modifiers::default()),
            Some(Message::OpenExternal(OpenKind::Edit))
        );
        assert_eq!(
            press(&keymap, named(Named::F4), Modifiers::default()),
            Some(Message::OpenExternal(OpenKind::View))
        );
        assert_eq!(
            keymap.function_keys(Language::English, KeyScope::Panel)[2..4],
            [Some("Edit"), Some("View")]
        );
    }

    /// `move` comes after `copy` in the enum but first in the file: the later
    /// line is the one that is reported, not the later action.
    #[test]
    fn a_clash_of_two_named_actions_is_reported_at_the_later_line() {
        let (line, reason) = failure("[keys]\nmove = \"F12\"\n\ncopy = \"F12\"\n");
        assert_eq!(line, Some(4));
        assert!(reason.contains("move"), "{reason}");
        let (reversed_line, _) = failure("[keys]\ncopy = \"F12\"\nmove = \"F12\"\n");
        assert_eq!(reversed_line, Some(3));
    }

    #[test]
    fn the_same_key_twice_for_one_action_is_refused() {
        let (line, reason) = failure("[keys]\nquit = [\"F10\",\n  \"f10\"]\n");
        assert_eq!(line, Some(3));
        assert!(reason.contains("listed twice"), "{reason}");
    }

    #[test]
    fn an_unknown_action_is_reported_at_its_own_line() {
        let (line, reason) = failure("[keys]\ncopy = \"F12\"\n\nfoo = \"F7\"\n");
        assert_eq!(line, Some(4));
        assert_eq!(reason, "unknown action \"foo\"");
    }

    #[test]
    fn a_bad_key_is_reported_at_its_own_line_also_inside_a_list() {
        let (line, reason) = failure("[keys]\nquit = [\n  \"F10\",\n  \"ctrl+xyz\",\n]\n");
        assert_eq!(line, Some(4));
        assert_eq!(reason, "unknown key \"xyz\" in \"ctrl+xyz\"");
        let (second_line, _) = failure("[keys]\ncopy = \"F12\"\nquit = \"hyper+q\"\n");
        assert_eq!(second_line, Some(3));
    }

    #[test]
    fn keys_that_would_break_the_app_are_refused() {
        for bad in [
            "escape",
            "esc",
            "Ctrl+Esc",
            "a",
            "shift+a",
            "5",
            "shift+5",
            "shift+8",
            "shift+*",
            "",
            "  ",
            "ctrl+",
            "ctrl+ctrl+c",
            "ctrl+control+c",
            "ab",
        ] {
            let result = keymap_from(&format!("[keys]\ncopy = \"{bad}\"\n"));
            assert!(result.is_err(), "{bad:?} was accepted");
        }
        let (_, reason) = failure("[keys]\ncopy = \"escape\"\n");
        assert!(reason.starts_with("Escape is reserved"), "{reason}");
        let (_, letter_reason) = failure("[keys]\ncopy = \"a\"\n");
        assert!(letter_reason.contains("type-ahead"), "{letter_reason}");
    }

    #[test]
    fn a_broken_keymap_drops_the_whole_file_including_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[open]\nview = [\"viewer\"]\n[keys]\ncopy = \"escape\"\n",
        )
        .unwrap();
        config::load(&path).unwrap_err();
    }

    #[test]
    fn an_empty_list_unbinds_the_action() {
        let keymap = rebound("[keys]\ndelete_permanently = []\n");
        assert_eq!(press(&keymap, named(Named::F8), Modifiers::SHIFT), None);
        assert_eq!(
            keymap.shortcut_label(&Message::Delete { permanent: true }),
            None
        );
        assert_eq!(
            press(&keymap, named(Named::F8), Modifiers::default()),
            Some(Message::Delete { permanent: false })
        );
    }

    #[test]
    fn keys_are_case_blind_and_lists_keep_their_order() {
        let keymap =
            rebound("[keys]\nquit = [\"PRIMARY+Q\", \"f10\"]\ncopy_path = \" Ctrl + Alt + P \"\n");
        assert_eq!(
            press(&keymap, Key::Character("q".into()), Modifiers::COMMAND),
            Some(Message::Quit)
        );
        assert_eq!(
            press(&keymap, named(Named::F10), Modifiers::default()),
            Some(Message::Quit)
        );
        assert_eq!(
            press(
                &keymap,
                Key::Character("p".into()),
                Modifiers::CTRL | Modifiers::ALT
            ),
            Some(Message::CopyToClipboard(ClipboardKind::Path))
        );
        let only_first = rebound("[keys]\nquit = [\"primary+q\", \"f10\"]\n");
        assert_eq!(
            only_first.shortcut_label(&Message::Quit),
            Keymap::built_in().shortcut_label(&Message::Quit)
        );
        let reversed = rebound("[keys]\nquit = [\"f10\", \"primary+q\"]\n");
        assert_eq!(reversed.shortcut_label(&Message::Quit), Some("F10".into()));
    }

    #[test]
    fn the_readme_example_loads_as_written() {
        let keymap = rebound(
            "[keys]\n# F12 copies, F5 does nothing any more.\ncopy = \"F12\"\n# Several keys for one action.\nquit = [\"F10\", \"primary+q\"]\n# No key at all; the context menu still offers it.\ndelete_permanently = []\n# A key that belongs to another action has to be freed there too:\n# F3 and F4 swapped.\nview = \"F4\"\nedit = \"F3\"\n",
        );
        let bare = Modifiers::default();
        assert_eq!(
            press(&keymap, named(Named::F12), bare),
            Some(Message::Transfer(TransferKind::Copy))
        );
        assert_eq!(press(&keymap, named(Named::F5), bare), None);
        assert_eq!(
            press(&keymap, named(Named::F3), bare),
            Some(Message::OpenExternal(OpenKind::Edit))
        );
        assert_eq!(
            press(&keymap, named(Named::F4), bare),
            Some(Message::OpenExternal(OpenKind::View))
        );
        assert_eq!(press(&keymap, named(Named::F8), Modifiers::SHIFT), None);
    }

    #[test]
    fn too_many_keys_for_one_action_are_refused_at_the_first_excess_one() {
        let keys: Vec<String> = (0..=MAX_KEYS_PER_ACTION)
            .filter_map(|index| char::from_u32(u32::from(b'a') + index as u32))
            .map(|letter| format!("alt+shift+{letter}"))
            .collect();
        let list = keys
            .iter()
            .map(|key| format!("  \"{key}\",\n"))
            .collect::<String>();
        let (line, reason) = failure(&format!("[keys]\ncopy = [\n{list}]\n"));
        assert_eq!(reason, "at most 16 keys per action");
        assert_eq!(line, Some(3 + MAX_KEYS_PER_ACTION));
    }

    #[test]
    fn invisible_format_characters_are_not_keys() {
        for invisible in ["\u{202E}", "\u{200B}", "\u{FEFF}", "ctrl+\u{202E}"] {
            let (_, reason) = failure(&format!("[keys]\ncopy = \"{invisible}\"\n"));
            assert!(reason.contains("unknown key"), "{invisible:?}: {reason}");
        }
    }

    #[test]
    fn plus_names_the_plus_character() {
        let keymap = rebound("[keys]\ncopy = \"ctrl+plus\"\nmove = \"PLUS\"\n");
        assert_eq!(
            press(&keymap, Key::Character("+".into()), Modifiers::CTRL),
            Some(Message::Transfer(TransferKind::Copy))
        );
        assert_eq!(
            press(&keymap, Key::Character("+".into()), Modifiers::default()),
            Some(Message::Transfer(TransferKind::Move))
        );
    }

    /// The dialog writes what it records; every default key has to survive
    /// the trip through the file unchanged.
    #[test]
    fn every_default_key_is_written_as_text_that_reads_back_as_itself() {
        for (action, binding) in default_bindings() {
            let text = configurable_chord(action, &binding)
                .unwrap_or_else(|refusal| panic!("{}: {refusal:?}", action.name()));
            assert_eq!(parse_chord(&text), Ok(binding), "{}", action.name());
        }
        let primary_k = Binding::with_modifiers(Key::Character("k".into()), Modifiers::COMMAND);
        assert_eq!(
            configurable_chord(KeyAction::KeyBindings, &primary_k),
            Ok("primary+k".to_owned())
        );
    }

    #[test]
    fn a_recorded_key_keeps_shift_only_where_the_lookup_does() {
        let bare = |key: Key| Binding::with_modifiers(key, Modifiers::default());
        let shift_ctrl = Modifiers::SHIFT | Modifiers::CTRL;
        assert_eq!(
            recorded_binding(&Key::Character("A".into()), shift_ctrl),
            Some(Binding::with_modifiers(
                Key::Character("a".into()),
                shift_ctrl
            ))
        );
        assert_eq!(
            recorded_binding(&Key::Character("*".into()), shift_ctrl),
            Some(Binding::with_modifiers(
                Key::Character("*".into()),
                Modifiers::CTRL
            ))
        );
        assert_eq!(
            recorded_binding(&named(Named::F8), Modifiers::SHIFT),
            Some(Binding::with_modifiers(named(Named::F8), Modifiers::SHIFT))
        );
        assert_eq!(
            recorded_binding(&named(Named::Shift), Modifiers::SHIFT),
            None
        );
        assert_eq!(
            configurable_chord(KeyAction::Copy, &bare(Key::Character("x".into()))),
            Err(KeyRefusal::NeedsModifier)
        );
        assert_eq!(
            configurable_chord(KeyAction::Copy, &bare(named(Named::F13))),
            Err(KeyRefusal::Unnamed)
        );
        assert_eq!(
            configurable_chord(KeyAction::Copy, &bare(named(Named::Escape))),
            Err(KeyRefusal::Escape)
        );
    }

    #[test]
    fn a_panel_action_may_take_a_key_the_search_toggle_has() {
        let keymap = rebound("[keys]\nmkdir = \"alt+c\"\n");
        let c = Key::Character("c".into());
        assert_eq!(
            keymap.map_key(&c, Modifiers::ALT, KeyScope::Panel),
            Some(Message::CreateDirPrompt)
        );
        assert_eq!(
            keymap.map_key(&c, Modifiers::ALT, KeyScope::Search),
            Some(Message::Search(SearchMessage::ToggleCase))
        );
    }

    #[test]
    fn two_panel_actions_cannot_share_a_key() {
        let (_, reason) = failure("[keys]\nmkdir = \"alt+x\"\ngo_up = \"alt+x\"\n");
        assert!(reason.contains("already bound"), "{reason}");
    }

    #[test]
    fn an_action_that_works_in_both_areas_conflicts_with_a_search_toggle() {
        let (_, reason) = failure("[keys]\nview = \"alt+c\"\n");
        assert!(reason.contains("search_case"), "{reason}");
    }

    #[test]
    fn a_search_toggle_cannot_take_the_fixed_keys_of_the_search_view() {
        for keys in [
            "search_case = \"enter\"",
            "search_regex = \"shift+up\"",
            "search_whole_word = \"pagedown\"",
        ] {
            let (_, reason) = failure(&format!("[keys]\n{keys}\n"));
            assert!(
                reason.contains("fixed in the search view"),
                "{keys}: {reason}"
            );
        }
        let keymap = rebound("[keys]\nsearch_case = \"alt+enter\"\n");
        assert_eq!(
            keymap.map_key(&named(Named::Enter), Modifiers::ALT, KeyScope::Search),
            Some(Message::Search(SearchMessage::ToggleCase))
        );
    }
}
