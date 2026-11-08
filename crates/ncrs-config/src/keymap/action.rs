use super::*;

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
    NextTab => "next_tab",
    PreviousTab => "previous_tab",
    ActivateLeftPanel => "activate_left_panel",
    ActivateRightPanel => "activate_right_panel",
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
    OpenTerminal => "open_terminal",
    Copy => "copy",
    Move => "move",
    Rename => "rename",
    Delete => "delete",
    DeletePermanently => "delete_permanently",
    Trash => "trash",
    RestoreLastDeletion => "restore_last_deletion",
    OpenTrashInFinder => "open_trash_in_finder",
    Mkdir => "mkdir",
    NewFile => "new_file",
    Pack => "pack",
    Permissions => "permissions",
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
    HermesChat => "hermes_chat",
    CommandPalette => "command_palette",
    FilingStatus => "filing_status",
    Rules => "rules",
    HermesQuickFix => "hermes_quick_fix",
    HermesHandToInbox => "hermes_hand_to_inbox",
    HermesProcessAgain => "hermes_process_again",
    HermesSetInbox => "hermes_set_inbox",
    HermesSetFilingLocation => "hermes_set_filing_location",
    HermesSetArchive => "hermes_set_archive",
    HermesExclude => "hermes_exclude",
    HermesInspect => "hermes_inspect",
    SavedQueries => "saved_queries",
    SearchDocuments => "search_documents",
    FetchTheme => "fetch_theme",
    Settings => "settings",
    SystemMonitor => "system_monitor",
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
    OpenHitsLeft => "open_hits_left",
    OpenHitsRight => "open_hits_right",
    FileListTagsToggle => "file_list_tags_toggle",
    FileListTagsChoose => "file_list_tags_choose",
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
pub enum Reach {
    PanelOnly,
    SearchOnly,
    Both,
}

/// Two actions may share a key only when they have no area in common, so one
/// press never triggers two actions that are active at the same time.
pub fn shares_a_scope(first: KeyAction, second: KeyAction) -> bool {
    [KeyScope::Panel, KeyScope::Search]
        .into_iter()
        .any(|scope| first.works_in(scope) && second.works_in(scope))
}

/// Keys of the search view that no configured chord can take: the search
/// handles them itself, and a search-only action on one would steal it.
pub fn is_fixed_in_search(binding: &Binding) -> bool {
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
            Self::SearchCase
            | Self::SearchWholeWord
            | Self::SearchRegex
            | Self::OpenHitsLeft
            | Self::OpenHitsRight => Reach::SearchOnly,
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
            | Self::SystemMonitor
            | Self::SwitchPanel
            | Self::NextTab
            | Self::PreviousTab
            | Self::ToggleTag
            | Self::TagDown
            | Self::TagUp
            | Self::TagAll
            | Self::ClearTags => Reach::Both,
            Self::GoUp
            | Self::Open
            | Self::OpenTerminal
            | Self::CopyPath
            | Self::CopyName
            | Self::CopyFiles
            | Self::Trash
            | Self::RestoreLastDeletion
            | Self::OpenTrashInFinder
            | Self::Paste
            | Self::Mkdir
            | Self::NewFile
            | Self::Rename
            | Self::Pack
            | Self::Permissions
            | Self::QuickOpen
            | Self::CommandLine
            | Self::HermesChat
            | Self::FilingStatus
            | Self::Rules
            | Self::HermesQuickFix
            | Self::HermesHandToInbox
            | Self::HermesProcessAgain
            | Self::HermesSetInbox
            | Self::HermesSetFilingLocation
            | Self::HermesSetArchive
            | Self::HermesExclude
            | Self::HermesInspect
            | Self::SavedQueries
            | Self::SearchDocuments
            | Self::FileListTagsToggle
            | Self::FileListTagsChoose
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
            | Self::ActivateLeftPanel
            | Self::ActivateRightPanel
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
    /// (`restore_last_deletion`) after 1.0, Shift+F6 (`rename`) after 1.2, F12 (`system_monitor`) after the 0.7.0 example config, Ctrl+Tab and the left and right arrows (`next_tab`, `previous_tab`, `activate_left_panel`, `activate_right_panel`) after 1.2.
    /// Such a config keeps working: its binding wins over the built-in one,
    /// and the action keeps its other keys.
    pub fn yields_its_built_in_key(self) -> bool {
        matches!(
            self,
            Self::CommandLine
                | Self::CommandPalette
                | Self::HermesChat
                | Self::NewFile
                | Self::Rename
                | Self::HistoryBack
                | Self::HistoryForward
                | Self::AddFavourite
                | Self::KeyBindings
                | Self::CopyFiles
                | Self::RestoreLastDeletion
                | Self::SystemMonitor
                | Self::NextTab
                | Self::PreviousTab
                | Self::ActivateLeftPanel
                | Self::ActivateRightPanel
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

    /// Whether this build offers the action. A build that can open every
    /// folder has nothing to grant, and macOS reads scans with its own text
    /// recognition, so the command list and the shortcuts dialog do not show
    /// either. The action itself stays, so a config that binds it still loads.
    pub fn is_offered(self) -> bool {
        self.is_offered_where(system_reads_scans())
    }

    fn is_offered_where(self, system_reads_scans: bool) -> bool {
        match self {
            Self::GrantFolderAccess => ncrs_fs::sandbox_active(),
            Self::DownloadOcrModels => !system_reads_scans,
            Self::OpenTrashInFinder => cfg!(target_os = "macos"),
            Self::SearchDocuments => false,
            _ => true,
        }
    }

    /// Whether a panel inside an archive may run the action. Only what takes
    /// neither the panel's path nor its selection as input for writing,
    /// launching, the config or Hermes; viewing is the exception, as it works
    /// on a copy, and so are copying in and deleting, which rewrite the
    /// archive. No catch-all arm: a new action has to be decided.
    #[cfg(any(test, feature = "test-support"))]
    pub fn allowed_in_archive(self) -> bool {
        match self {
            Self::Quit
            | Self::GoUp
            | Self::Open
            | Self::SwitchPanel
            | Self::NextTab
            | Self::PreviousTab
            | Self::ActivateLeftPanel
            | Self::ActivateRightPanel
            | Self::SwitchLanguage
            | Self::SystemMonitor
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
            | Self::HermesChat
            | Self::CommandPalette
            | Self::FilingStatus
            | Self::Rules
            | Self::SavedQueries
            | Self::FileListTagsToggle
            | Self::FileListTagsChoose
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
            | Self::OpenHitsLeft
            | Self::OpenHitsRight
            | Self::Delete
            | Self::DeletePermanently
            | Self::View => true,
            Self::Edit
            | Self::OpenTerminal
            | Self::Move
            | Self::Rename
            | Self::Mkdir
            | Self::NewFile
            | Self::Pack
            | Self::Permissions
            | Self::ComputeFolderSizes
            | Self::AddFavourite
            | Self::RemoveFavourite
            | Self::Search
            | Self::SearchDocuments
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
    /// plus the actions that connect and disconnect and renaming, which SFTP does itself. No catch-all arm: a new
    /// action has to be decided.
    #[cfg(any(test, feature = "test-support"))]
    pub fn allowed_on_server(self) -> bool {
        match self {
            Self::CommandLine => false,
            Self::Quit
            | Self::HermesChat
            | Self::SystemMonitor
            | Self::GoUp
            | Self::Open
            | Self::SwitchPanel
            | Self::NextTab
            | Self::PreviousTab
            | Self::ActivateLeftPanel
            | Self::ActivateRightPanel
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
            | Self::Rules
            | Self::SavedQueries
            | Self::FileListTagsToggle
            | Self::FileListTagsChoose
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
            | Self::OpenHitsLeft
            | Self::OpenHitsRight
            | Self::Rename
            | Self::View
            | Self::Copy => true,
            Self::Edit
            | Self::OpenTerminal
            | Self::Move
            | Self::Delete
            | Self::DeletePermanently
            | Self::Mkdir
            | Self::NewFile
            | Self::Pack
            | Self::Permissions
            | Self::ComputeFolderSizes
            | Self::AddFavourite
            | Self::RemoveFavourite
            | Self::Search
            | Self::SearchDocuments
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
            Self::HermesChat => Some(Msg::ShortcutHermesChat),
            _ => None,
        }
    }
}

/// macOS reads scans with its own text recognition, so there is nothing to
/// download there. Tests behave like the other systems, so that snapshots do
/// not depend on the machine they run on.
pub const fn system_reads_scans() -> bool {
    cfg!(target_os = "macos") && !cfg!(any(test, feature = "test-support"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ocr_download_is_offered_only_where_the_system_does_not_read_scans() {
        assert!(KeyAction::DownloadOcrModels.is_offered_where(false));
        assert!(!KeyAction::DownloadOcrModels.is_offered_where(true));
        assert!(KeyAction::Quit.is_offered_where(true));
    }
}
