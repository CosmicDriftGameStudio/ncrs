//! The one list of commands: what the command list (the Lab) shows, and what
//! the native menu will show. Each command is a key action with a group, a
//! plain label and, for the playful ones, a theme name. Its shortcut is read
//! from the keymap, so a `[keys]` override shows up everywhere.

use crate::keymap::KeyAction;
use ncrs_i18n::{FuturamaName, Msg};

/// The headings of the Lab and of the menu, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommandGroup {
    File,
    Edit,
    View,
    Go,
    Search,
    Hermes,
    Window,
    Help,
}

impl CommandGroup {
    pub const ALL: [Self; 8] = [
        Self::File,
        Self::Edit,
        Self::View,
        Self::Go,
        Self::Search,
        Self::Hermes,
        Self::Window,
        Self::Help,
    ];

    pub fn label(self) -> Msg {
        match self {
            Self::File => Msg::GroupFile,
            Self::Edit => Msg::GroupEdit,
            Self::View => Msg::GroupView,
            Self::Go => Msg::GroupGo,
            Self::Search => Msg::GroupSearch,
            Self::Hermes => Msg::GroupHermes,
            Self::Window => Msg::GroupWindow,
            Self::Help => Msg::GroupHelp,
        }
    }

    pub fn theme_name(self) -> Option<FuturamaName> {
        match self {
            Self::Hermes => Some(FuturamaName::GroupHermes),
            Self::File
            | Self::Edit
            | Self::View
            | Self::Go
            | Self::Search
            | Self::Window
            | Self::Help => None,
        }
    }
}

/// A submenu of a group's menu, for the commands that are long or rarely
/// used. The order is the order the submenus have within a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Submenu {
    Bookmarks,
    Places,
    HermesReview,
    HermesSetup,
}

impl Submenu {
    pub fn label(self) -> Msg {
        match self {
            Self::Bookmarks => Msg::MenuSubmenuBookmarks,
            Self::Places => Msg::MenuSubmenuPlaces,
            Self::HermesReview => Msg::MenuSubmenuHermesReview,
            Self::HermesSetup => Msg::MenuSubmenuHermesSetup,
        }
    }
}

/// How the menu shows a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Plain,
    /// Opens a dialog or a list first, so the title ends with "…".
    Dialog,
    /// Switches something on or off and shows which.
    Toggle,
}

/// Where a command stands in the menu of its group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuPlace {
    /// The commands of a group with the same section stand together, with a
    /// separator between the sections, in ascending order.
    pub section: u8,
    pub submenu: Option<Submenu>,
    pub kind: EntryKind,
}

impl MenuPlace {
    const fn section(section: u8) -> Self {
        Self {
            section,
            submenu: None,
            kind: EntryKind::Plain,
        }
    }

    const fn dialog(self) -> Self {
        Self {
            kind: EntryKind::Dialog,
            ..self
        }
    }

    const fn toggle(self) -> Self {
        Self {
            kind: EntryKind::Toggle,
            ..self
        }
    }

    const fn within(self, submenu: Submenu) -> Self {
        Self {
            submenu: Some(submenu),
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    pub action: KeyAction,
    pub group: CommandGroup,
    pub label: Msg,
    pub theme_name: Option<FuturamaName>,
    pub menu: MenuPlace,
}

impl Command {
    const fn plain(action: KeyAction, group: CommandGroup, label: Msg) -> Option<Self> {
        Some(Self {
            action,
            group,
            label,
            theme_name: None,
            menu: MenuPlace::section(0),
        })
    }

    const fn themed(
        action: KeyAction,
        group: CommandGroup,
        label: Msg,
        theme_name: FuturamaName,
    ) -> Option<Self> {
        Some(Self {
            action,
            group,
            label,
            theme_name: Some(theme_name),
            menu: MenuPlace::section(0),
        })
    }
}

/// The command of `action`, or `None` for cursor and tag movement, which is
/// not something to look up by name.
pub fn command_of(action: KeyAction) -> Option<Command> {
    command_without_place(action).map(|command| Command {
        menu: menu_place_of(action),
        ..command
    })
}

/// The place in the menu. No catch-all arm: a new action has to say where it
/// belongs. The sections are in the order the menu shows them.
fn menu_place_of(action: KeyAction) -> MenuPlace {
    use Submenu::{Bookmarks, HermesReview, HermesSetup, Places};
    let at = MenuPlace::section;
    match action {
        KeyAction::CursorUp
        | KeyAction::CursorDown
        | KeyAction::PageUp
        | KeyAction::PageDown
        | KeyAction::First
        | KeyAction::Last
        | KeyAction::TagUp
        | KeyAction::TagDown
        | KeyAction::Open
        | KeyAction::View
        | KeyAction::Edit
        | KeyAction::OpenTerminal => at(0),
        KeyAction::Copy | KeyAction::Move => at(1),
        KeyAction::Rename | KeyAction::Mkdir | KeyAction::NewFile => at(1).dialog(),
        KeyAction::Pack | KeyAction::Permissions => at(2).dialog(),
        KeyAction::Trash
        | KeyAction::Delete
        | KeyAction::DeletePermanently
        | KeyAction::OpenTrashInFinder => at(3),
        KeyAction::AbortJob => at(4),
        KeyAction::GrantFolderAccess => at(4).dialog(),
        KeyAction::Quit => at(5),
        KeyAction::ToggleTag | KeyAction::TagAll | KeyAction::ClearTags => at(0),
        KeyAction::CopyPath | KeyAction::CopyName | KeyAction::CopyFiles | KeyAction::Paste => {
            at(1)
        }
        KeyAction::RestoreLastDeletion | KeyAction::ContextMenu => at(2),
        KeyAction::SortByName
        | KeyAction::SortByModified
        | KeyAction::SortBySize
        | KeyAction::ComputeFolderSizes => at(0),
        KeyAction::FileListTagsToggle => at(1).toggle(),
        KeyAction::FileListTagsChoose => at(1).dialog(),
        KeyAction::FetchTheme => at(2).dialog(),
        KeyAction::SwitchLanguage => at(2),
        KeyAction::Settings | KeyAction::SystemMonitor => at(3),
        KeyAction::KeyBindings => at(3).dialog(),
        KeyAction::GoUp
        | KeyAction::SwitchPanel
        | KeyAction::HistoryBack
        | KeyAction::HistoryForward => at(0),
        KeyAction::CommandLine | KeyAction::HermesChat | KeyAction::QuickOpen => at(1),
        KeyAction::DriveMenuLeft | KeyAction::DriveMenuRight | KeyAction::DisconnectServer => {
            at(2).within(Places)
        }
        KeyAction::ConnectServer => at(2).dialog().within(Places),
        KeyAction::AddFavourite | KeyAction::RemoveFavourite => at(2).within(Bookmarks),
        KeyAction::Favourites => at(2).dialog().within(Bookmarks),
        KeyAction::Search | KeyAction::SearchDocuments => at(0),
        KeyAction::SearchCase | KeyAction::SearchWholeWord | KeyAction::SearchRegex => {
            at(1).toggle()
        }
        KeyAction::OpenHitsLeft | KeyAction::OpenHitsRight => at(2),
        KeyAction::HermesHandToInbox
        | KeyAction::HermesProcessAgain
        | KeyAction::HermesQuickFix
        | KeyAction::HermesExclude => at(0),
        KeyAction::Rules => at(1),
        KeyAction::FilingStatus | KeyAction::SavedQueries | KeyAction::HermesInspect => {
            at(1).within(HermesReview)
        }
        KeyAction::HermesSetInbox
        | KeyAction::HermesSetFilingLocation
        | KeyAction::HermesSetArchive => at(1).within(HermesSetup),
        KeyAction::DownloadOcrModels | KeyAction::DownloadAiModel => {
            at(1).dialog().within(HermesSetup)
        }
        KeyAction::About => at(0),
        KeyAction::CommandPalette => at(1).dialog(),
    }
}

fn command_without_place(action: KeyAction) -> Option<Command> {
    use CommandGroup::{Edit, File, Go, Help, Hermes, Search, View};
    match action {
        KeyAction::CursorUp
        | KeyAction::CursorDown
        | KeyAction::PageUp
        | KeyAction::PageDown
        | KeyAction::First
        | KeyAction::Last
        | KeyAction::TagUp
        | KeyAction::TagDown => None,
        KeyAction::Open => Command::plain(action, File, Msg::ActionOpen),
        KeyAction::View => Command::plain(action, File, Msg::ActionView),
        KeyAction::Edit => Command::plain(action, File, Msg::ActionEdit),
        KeyAction::OpenTerminal => Command::plain(action, File, Msg::ActionOpenTerminal),
        KeyAction::Copy => Command::plain(action, File, Msg::ActionCopy),
        KeyAction::Move => Command::plain(action, File, Msg::ActionMove),
        KeyAction::Rename => Command::plain(action, File, Msg::ActionRename),
        KeyAction::Mkdir => Command::plain(action, File, Msg::ActionMkdir),
        KeyAction::NewFile => Command::plain(action, File, Msg::ActionNewFile),
        KeyAction::Pack => Command::themed(action, File, Msg::ActionPack, FuturamaName::ActionPack),
        KeyAction::Permissions => Command::plain(action, File, Msg::ActionPermissions),
        KeyAction::Delete => {
            Command::themed(action, File, Msg::ActionDelete, FuturamaName::ActionDelete)
        }
        KeyAction::DeletePermanently => Command::plain(action, File, Msg::ActionDeletePermanently),
        KeyAction::Trash => {
            Command::themed(action, File, Msg::ActionTrash, FuturamaName::ActionTrash)
        }
        KeyAction::OpenTrashInFinder => Command::plain(action, File, Msg::ActionOpenTrashInFinder),
        KeyAction::AbortJob => Command::plain(action, File, Msg::ActionAbortJob),
        KeyAction::Quit => Command::plain(action, File, Msg::ActionQuit),
        KeyAction::ToggleTag => Command::themed(
            action,
            Edit,
            Msg::ActionToggleTag,
            FuturamaName::ActionToggleTag,
        ),
        KeyAction::TagAll => {
            Command::themed(action, Edit, Msg::ActionTagAll, FuturamaName::ActionTagAll)
        }
        KeyAction::ClearTags => Command::themed(
            action,
            Edit,
            Msg::ActionClearTags,
            FuturamaName::ActionClearTags,
        ),
        KeyAction::CopyPath => Command::plain(action, Edit, Msg::ActionCopyPath),
        KeyAction::CopyName => Command::plain(action, Edit, Msg::ActionCopyName),
        KeyAction::CopyFiles => Command::plain(action, Edit, Msg::ActionCopyFiles),
        KeyAction::Paste => Command::plain(action, Edit, Msg::ActionPaste),
        KeyAction::RestoreLastDeletion => Command::themed(
            action,
            Edit,
            Msg::ActionRestoreLastDeletion,
            FuturamaName::ActionRestoreLastDeletion,
        ),
        KeyAction::ContextMenu => Command::plain(action, Edit, Msg::ActionContextMenu),
        KeyAction::SortByName => Command::plain(action, View, Msg::ActionSortByName),
        KeyAction::SortByModified => Command::plain(action, View, Msg::ActionSortByModified),
        KeyAction::SortBySize => Command::plain(action, View, Msg::ActionSortBySize),
        KeyAction::ComputeFolderSizes => {
            Command::plain(action, View, Msg::ActionComputeFolderSizes)
        }
        KeyAction::FetchTheme => Command::plain(action, View, Msg::ActionFetchTheme),
        KeyAction::SwitchLanguage => Command::plain(action, View, Msg::ActionSwitchLanguage),
        KeyAction::Settings => Command::plain(action, View, Msg::ActionSettings),
        KeyAction::SystemMonitor => Command::plain(action, View, Msg::ActionSystemMonitor),
        KeyAction::FileListTagsToggle => {
            Command::plain(action, View, Msg::ActionFileListTagsToggle)
        }
        KeyAction::FileListTagsChoose => {
            Command::plain(action, View, Msg::ActionFileListTagsChoose)
        }
        KeyAction::KeyBindings => Command::plain(action, View, Msg::ActionKeyBindings),
        KeyAction::GoUp => Command::plain(action, Go, Msg::ActionGoUp),
        KeyAction::SwitchPanel => Command::plain(action, Go, Msg::ActionSwitchPanel),
        KeyAction::DriveMenuLeft => Command::themed(
            action,
            Go,
            Msg::ActionDriveMenuLeft,
            FuturamaName::ActionDriveMenuLeft,
        ),
        KeyAction::DriveMenuRight => Command::themed(
            action,
            Go,
            Msg::ActionDriveMenuRight,
            FuturamaName::ActionDriveMenuRight,
        ),
        KeyAction::HistoryBack => Command::plain(action, Go, Msg::ActionHistoryBack),
        KeyAction::HistoryForward => Command::plain(action, Go, Msg::ActionHistoryForward),
        KeyAction::AddFavourite => Command::plain(action, Go, Msg::ActionAddFavourite),
        KeyAction::RemoveFavourite => Command::plain(action, Go, Msg::ActionRemoveFavourite),
        KeyAction::Favourites => Command::plain(action, Go, Msg::ActionFavourites),
        KeyAction::CommandLine => Command::plain(action, Go, Msg::ActionCommandLine),
        KeyAction::HermesChat => Command::plain(action, Go, Msg::ActionHermesChat),
        KeyAction::QuickOpen => Command::themed(
            action,
            Go,
            Msg::ActionQuickOpen,
            FuturamaName::ActionQuickOpen,
        ),
        KeyAction::Search => Command::themed(
            action,
            Search,
            Msg::ActionSearch,
            FuturamaName::ActionSearch,
        ),
        KeyAction::SearchCase => Command::plain(action, Search, Msg::ActionSearchCase),
        KeyAction::SearchWholeWord => Command::plain(action, Search, Msg::ActionSearchWholeWord),
        KeyAction::SearchRegex => Command::plain(action, Search, Msg::ActionSearchRegex),
        KeyAction::OpenHitsLeft => Command::plain(action, Search, Msg::ActionOpenHitsLeft),
        KeyAction::OpenHitsRight => Command::plain(action, Search, Msg::ActionOpenHitsRight),
        KeyAction::FilingStatus => Command::themed(
            action,
            Hermes,
            Msg::ActionFilingStatus,
            FuturamaName::ActionFilingStatus,
        ),
        KeyAction::Rules => {
            Command::themed(action, Hermes, Msg::ActionRules, FuturamaName::ActionRules)
        }
        KeyAction::HermesQuickFix => Command::plain(action, Hermes, Msg::ActionHermesQuickFix),
        KeyAction::HermesHandToInbox => {
            Command::plain(action, Hermes, Msg::ActionHermesHandToInbox)
        }
        KeyAction::HermesProcessAgain => {
            Command::plain(action, Hermes, Msg::ActionHermesProcessAgain)
        }
        KeyAction::HermesSetInbox => Command::plain(action, Hermes, Msg::ActionHermesSetInbox),
        KeyAction::HermesSetFilingLocation => {
            Command::plain(action, Hermes, Msg::ActionHermesSetFilingLocation)
        }
        KeyAction::HermesSetArchive => Command::plain(action, Hermes, Msg::ActionHermesSetArchive),
        KeyAction::HermesExclude => Command::plain(action, Hermes, Msg::ActionHermesExclude),
        KeyAction::HermesInspect => Command::plain(action, Hermes, Msg::ActionHermesInspect),
        KeyAction::SavedQueries => Command::themed(
            action,
            Hermes,
            Msg::ActionSavedQueries,
            FuturamaName::ActionSavedQueries,
        ),
        KeyAction::SearchDocuments => Command::themed(
            action,
            Hermes,
            Msg::ActionSearchDocuments,
            FuturamaName::ActionSearchDocuments,
        ),
        KeyAction::About => {
            Command::themed(action, Help, Msg::ActionAbout, FuturamaName::ActionAbout)
        }
        KeyAction::DownloadOcrModels => {
            Command::plain(action, Hermes, Msg::ActionDownloadOcrModels)
        }
        KeyAction::DownloadAiModel => Command::themed(
            action,
            Hermes,
            Msg::ActionDownloadAiModel,
            FuturamaName::ActionDownloadAiModel,
        ),
        KeyAction::GrantFolderAccess => Command::plain(action, File, Msg::ActionGrantFolderAccess),
        KeyAction::ConnectServer => Command::themed(
            action,
            Go,
            Msg::ActionConnectServer,
            FuturamaName::ActionConnectServer,
        ),
        KeyAction::DisconnectServer => Command::themed(
            action,
            Go,
            Msg::ActionDisconnectServer,
            FuturamaName::ActionDisconnectServer,
        ),
        KeyAction::CommandPalette => Command::themed(
            action,
            Help,
            Msg::ActionCommandPalette,
            FuturamaName::ActionCommandPalette,
        ),
    }
}

/// Every command, in the order of the key actions. Granting folders is
/// offered only where the sandbox makes it necessary: the App Store build.
pub fn all() -> impl Iterator<Item = Command> {
    KeyAction::ALL
        .iter()
        .filter(|action| action.is_offered())
        .filter_map(|&action| command_of(action))
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
    use super::*;
    use ncrs_i18n::Language;

    #[test]
    fn only_movement_is_not_a_command() {
        let mut others: Vec<&str> = KeyAction::ALL
            .iter()
            .filter(|action| command_of(**action).is_none())
            .map(|action| action.name())
            .collect();
        others.sort_unstable();
        assert_eq!(
            others,
            [
                "cursor_down",
                "cursor_up",
                "first",
                "last",
                "page_down",
                "page_up",
                "tag_down",
                "tag_up"
            ]
        );
    }

    #[test]
    fn every_command_has_its_texts_in_both_languages() {
        // Not `all()`: that leaves out the commands of the App Store build,
        // and their texts are needed there.
        for command in KeyAction::ALL
            .iter()
            .filter_map(|&action| command_of(action))
        {
            let name = command.action.name();
            assert_eq!(
                command.label.key(),
                format!("action.{name}"),
                "the label key follows the config name"
            );
            if let Some(theme_name) = command.theme_name {
                assert_eq!(theme_name.key(), format!("action.{name}.theme"));
            }
            for lang in [Language::English, Language::German] {
                assert!(!lang.text(command.label).trim().is_empty(), "{name}");
                assert!(!lang.text(command.group.label()).trim().is_empty());
            }
            if let Some(theme_name) = command.theme_name {
                for lang in [Language::EnglishFuturama, Language::GermanFuturama] {
                    let shown = lang.futurama_name(theme_name).unwrap();
                    assert!(!shown.trim().is_empty(), "{name}");
                }
            }
        }
    }

    #[test]
    fn the_groups_come_in_their_fixed_order() {
        let mut sorted = CommandGroup::ALL;
        sorted.sort();
        assert_eq!(sorted, CommandGroup::ALL);
        assert_eq!(CommandGroup::ALL.first(), Some(&CommandGroup::File));
        assert_eq!(CommandGroup::ALL.last(), Some(&CommandGroup::Help));
    }
}
