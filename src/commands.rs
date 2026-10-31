//! The one list of commands: what the command list (the Lab) shows, and what
//! the native menu will show. Each command is a key action with a group, a
//! plain label and, for the playful ones, a theme name. Its shortcut is read
//! from the keymap, so a `[keys]` override shows up everywhere.

use crate::i18n::{FuturamaName, Msg};
use crate::keymap::KeyAction;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    pub action: KeyAction,
    pub group: CommandGroup,
    pub label: Msg,
    pub theme_name: Option<FuturamaName>,
}

impl Command {
    const fn plain(action: KeyAction, group: CommandGroup, label: Msg) -> Option<Self> {
        Some(Self {
            action,
            group,
            label,
            theme_name: None,
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
        })
    }
}

/// The command of `action`, or `None` for cursor and tag movement, which is
/// not something to look up by name. No catch-all arm: a new action has to
/// say whether it is a command and where it belongs.
pub fn command_of(action: KeyAction) -> Option<Command> {
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
        KeyAction::Copy => Command::plain(action, File, Msg::ActionCopy),
        KeyAction::Move => Command::plain(action, File, Msg::ActionMove),
        KeyAction::Mkdir => Command::plain(action, File, Msg::ActionMkdir),
        KeyAction::NewFile => Command::plain(action, File, Msg::ActionNewFile),
        KeyAction::Pack => Command::themed(action, File, Msg::ActionPack, FuturamaName::ActionPack),
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
        KeyAction::FilingStatus => Command::themed(
            action,
            Hermes,
            Msg::ActionFilingStatus,
            FuturamaName::ActionFilingStatus,
        ),
        KeyAction::FilingSetup => Command::themed(
            action,
            Hermes,
            Msg::ActionFilingSetup,
            FuturamaName::ActionFilingSetup,
        ),
        KeyAction::Rules => {
            Command::themed(action, Hermes, Msg::ActionRules, FuturamaName::ActionRules)
        }
        KeyAction::RuleSuggestions => Command::themed(
            action,
            Hermes,
            Msg::ActionRuleSuggestions,
            FuturamaName::ActionRuleSuggestions,
        ),
        KeyAction::ReviewList => Command::themed(
            action,
            Hermes,
            Msg::ActionReviewList,
            FuturamaName::ActionReviewList,
        ),
        KeyAction::HermesQuickFix => Command::plain(action, Hermes, Msg::ActionHermesQuickFix),
        KeyAction::HermesHandToInbox => {
            Command::plain(action, Hermes, Msg::ActionHermesHandToInbox)
        }
        KeyAction::HermesProcessAgain => {
            Command::plain(action, Hermes, Msg::ActionHermesProcessAgain)
        }
        KeyAction::HermesReprocessFolder => {
            Command::plain(action, Hermes, Msg::ActionHermesReprocessFolder)
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
        KeyAction::ReorganisationPlan => Command::themed(
            action,
            Hermes,
            Msg::ActionReorganisationPlan,
            FuturamaName::ActionReorganisationPlan,
        ),
        KeyAction::UndoReorganisation => Command::themed(
            action,
            Hermes,
            Msg::ActionUndoReorganisation,
            FuturamaName::ActionUndoReorganisation,
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
    use crate::i18n::Language;

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
