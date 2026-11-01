//! Keyboard bindings and the header hints. The actions, the keymap and the
//! parsing of `[keys]` live in `ncrs-config`; what a key sends is the app's
//! `Message`, so that mapping stays here as extension traits with the method
//! names the call sites already use.

use iced::keyboard::{Key, Modifiers};

pub use ncrs_config::keymap::*;

use crate::fs::OpenKind;
use crate::hermes_column::FolderRole;
use crate::messages::Message;
use crate::messages::PanelMessage;
use crate::messages::{
    AboutMessage, AiModelMessage, ClipboardKind, CommandLineMessage, CommandPaletteMessage,
    FilingSetupMessage, FilingStatusMessage, GrantMessage, HermesFileMessage, KeyBindingsMessage,
    OcrModelsMessage, PanelSide, QuickFixMessage, QuickOpenMessage, RemoteMessage,
    ReorganisationPlanMessage, ReviewListMessage, RuleSuggestionsMessage, RulesMessage,
    SavedQueriesMessage, SearchMessage, SettingsMessage, ThemeFetchMessage, TransferKind,
    TrashMessage,
};
use ncrs_fs::SortColumn;

/// The message a key action sends.
pub trait KeyActionMessage {
    /// The message the key sends. Derived rather than stored next to the key,
    /// so a rebound key cannot send anything but its action.
    fn message(self) -> Message;
}

impl KeyActionMessage for KeyAction {
    fn message(self) -> Message {
        match self {
            Self::Quit => Message::Quit,
            Self::GoUp => Message::Panel(PanelMessage::GoUp),
            Self::Open => Message::Panel(PanelMessage::OpenSelected),
            Self::SwitchPanel => Message::Panel(PanelMessage::SwitchPanel),
            Self::SwitchLanguage => Message::SwitchLanguage,
            Self::ToggleTag => Message::Panel(PanelMessage::ToggleTag),
            Self::TagDown => Message::Panel(PanelMessage::TagMove(1)),
            Self::TagUp => Message::Panel(PanelMessage::TagMove(-1)),
            Self::TagAll => Message::Panel(PanelMessage::TagAll),
            Self::ClearTags => Message::Panel(PanelMessage::ClearTags),
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
            Self::HistoryBack => Message::Panel(PanelMessage::HistoryBack),
            Self::HistoryForward => Message::Panel(PanelMessage::HistoryForward),
            Self::AddFavourite => Message::AddFavourite,
            Self::RemoveFavourite => Message::RemoveFavourite,
            Self::Favourites => Message::OpenFavourites,
            Self::SortByName => Message::Panel(PanelMessage::SortActive(SortColumn::Name)),
            Self::SortByModified => Message::Panel(PanelMessage::SortActive(SortColumn::Modified)),
            Self::SortBySize => Message::Panel(PanelMessage::SortActive(SortColumn::Size)),
            Self::ComputeFolderSizes => Message::ComputeFolderSizes,
            Self::ContextMenu => Message::ContextMenuKey,
            Self::CursorUp => Message::Panel(PanelMessage::MoveSelection(-1)),
            Self::CursorDown => Message::Panel(PanelMessage::MoveSelection(1)),
            Self::PageUp => Message::Panel(PanelMessage::PageUp),
            Self::PageDown => Message::Panel(PanelMessage::PageDown),
            Self::First => Message::Panel(PanelMessage::SelectFirst),
            Self::Last => Message::Panel(PanelMessage::SelectLast),
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
            Self::HermesReprocessFolder => Message::HermesFile(HermesFileMessage::ReprocessFolder),
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
            Self::SearchDocuments => Message::OpenSearch,
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
            Self::OpenHitsLeft => Message::Search(SearchMessage::OpenHitList(PanelSide::Left)),
            Self::OpenHitsRight => Message::Search(SearchMessage::OpenHitList(PanelSide::Right)),
            Self::FileListTagsToggle => Message::Settings(SettingsMessage::ToggleFileListTags),
            Self::FileListTagsChoose => Message::Settings(SettingsMessage::ChooseFileListTags),
        }
    }
}

/// Looking a key press up as a message.
pub trait KeymapMessages {
    fn map_key(&self, key: &Key, modifiers: Modifiers, scope: KeyScope) -> Option<Message>;
    fn shortcut_label(&self, message: &Message) -> Option<String>;
}

impl KeymapMessages for Keymap {
    /// Maps a key press to a message, among the actions that work in `scope`.
    fn map_key(&self, key: &Key, modifiers: Modifiers, scope: KeyScope) -> Option<Message> {
        self.action_for(key, modifiers, scope)
            .map(KeyAction::message)
    }

    /// The key that sends `message`, spelled for this platform: `F5`, `⇧F8`,
    /// `⌥⌘C` on a Mac, `Ctrl+Alt+C` elsewhere. `None` for a message no key
    /// sends.
    fn shortcut_label(&self, message: &Message) -> Option<String> {
        let (_, binding) = self
            .bindings()
            .iter()
            .find(|(action, _)| action.message() == *message)?;
        binding_label(binding)
    }
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::i18n::Language;
    use iced::keyboard::key::Named;

    fn function_keys(lang: Language) -> [Option<&'static str>; FUNCTION_KEY_COUNT] {
        Keymap::built_in().function_keys(lang, KeyScope::Panel)
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

    fn named(key: Named) -> Key {
        Key::Named(key)
    }

    fn map_key(key: Key, modifiers: Modifiers) -> Option<Message> {
        Keymap::built_in().map_key(&key, modifiers, KeyScope::Panel)
    }

    fn shortcut_label(message: &Message) -> Option<String> {
        Keymap::built_in().shortcut_label(message)
    }

    fn press(keymap: &Keymap, key: Key, modifiers: Modifiers) -> Option<Message> {
        keymap.map_key(&key, modifiers, KeyScope::Panel)
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
                Some(Message::Panel(PanelMessage::SortActive(column)))
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
                Some(Message::Panel(PanelMessage::SortActive(column)))
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

    #[test]
    fn shift_f8_is_bound_but_has_no_slot() {
        assert_eq!(
            map_key(Key::Named(Named::F8), Modifiers::SHIFT),
            Some(Message::Delete { permanent: true })
        );
        assert_eq!(function_keys(Language::English)[7], Some("Delete"));
    }

    #[test]
    fn bindings_produce_the_expected_messages() {
        assert_eq!(
            map_key(Key::Named(Named::Enter), Modifiers::default()),
            Some(Message::Panel(PanelMessage::OpenSelected))
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
            Some(Message::Panel(PanelMessage::HistoryBack))
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
        assert_eq!(label(Message::Panel(PanelMessage::OpenSelected)), "Enter");
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
