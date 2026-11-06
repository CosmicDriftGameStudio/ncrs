//! The menu bar as data: which menus there are, what they hold and which key
//! shows beside each entry. Built from the command list, so the menu and the
//! Lab cannot disagree; the native layer only turns it into system menus.

use std::collections::BTreeMap;

use iced::keyboard::{key, Key};

use crate::commands::{self, Command, CommandGroup, EntryKind, Submenu};
use crate::i18n::Language;
use crate::keymap::{Binding, KeyAction, KeyScope, Keymap};
use crate::messages::Message;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    /// The app menu holds the commands that belong there on a Mac instead of
    /// in their group: About first, Quit last.
    pub app: Vec<MenuItemSpec>,
    pub menus: Vec<MenuSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSpec {
    pub group: CommandGroup,
    pub title: String,
    pub items: Vec<MenuEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    Item(MenuItemSpec),
    /// Between two sections of a menu.
    Separator,
    Submenu(SubmenuSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmenuSpec {
    pub submenu: Submenu,
    pub title: String,
    pub items: Vec<MenuItemSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItemSpec {
    pub action: KeyAction,
    pub title: String,
    /// The key shown beside the entry, when it is safe to hand to the system.
    pub accelerator: Option<Binding>,
    /// `Some` for a toggle: whether it is on right now.
    pub checked: Option<bool>,
}

impl MenuEntry {
    /// The items of this entry: itself, or those of its submenu.
    pub fn items(&self) -> &[MenuItemSpec] {
        match self {
            Self::Item(item) => std::slice::from_ref(item),
            Self::Separator => &[],
            Self::Submenu(submenu) => &submenu.items,
        }
    }
}

const APP_MENU_ACTIONS: [KeyAction; 2] = [KeyAction::About, KeyAction::Quit];

const DIALOG_ELLIPSIS: char = '\u{2026}';

impl MenuModel {
    pub fn build(lang: Language, keymap: &Keymap, toggles: Toggles) -> Self {
        let item_of = |command: Command| {
            let title = lang.titled(command.label, command.theme_name);
            MenuItemSpec {
                action: command.action,
                title: match command.menu.kind {
                    EntryKind::Dialog => format!("{title}{DIALOG_ELLIPSIS}"),
                    EntryKind::Plain | EntryKind::Toggle => title,
                },
                accelerator: keymap
                    .bindings_of(command.action)
                    .find(|binding| is_safe_as_key_equivalent(binding))
                    .cloned(),
                checked: (command.menu.kind == EntryKind::Toggle)
                    .then(|| toggles.is_on(command.action)),
            }
        };
        let app = APP_MENU_ACTIONS
            .iter()
            .filter_map(|&action| commands::command_of(action))
            .map(item_of)
            .collect();
        let menus = CommandGroup::ALL
            .iter()
            .map(|&group| MenuSpec {
                group,
                title: lang.titled(group.label(), group.theme_name()),
                items: entries_of(
                    lang,
                    commands::all().filter(|command| {
                        command.group == group && !APP_MENU_ACTIONS.contains(&command.action)
                    }),
                    item_of,
                ),
            })
            .collect();
        Self { app, menus }
    }
}

/// The entries of one group's menu: its sections in order with a separator
/// between them, and in each section the plain items before the submenus.
fn entries_of(
    lang: Language,
    group_commands: impl Iterator<Item = Command>,
    item_of: impl Fn(Command) -> MenuItemSpec,
) -> Vec<MenuEntry> {
    let mut sections: BTreeMap<u8, Vec<Command>> = BTreeMap::new();
    for command in group_commands {
        sections
            .entry(command.menu.section)
            .or_default()
            .push(command);
    }
    let mut entries = Vec::new();
    for section in sections.into_values() {
        if !entries.is_empty() {
            entries.push(MenuEntry::Separator);
        }
        let mut submenus: BTreeMap<Submenu, Vec<MenuItemSpec>> = BTreeMap::new();
        for command in section {
            match command.menu.submenu {
                None => entries.push(MenuEntry::Item(item_of(command))),
                Some(submenu) => submenus.entry(submenu).or_default().push(item_of(command)),
            }
        }
        entries.extend(submenus.into_iter().map(|(submenu, items)| {
            MenuEntry::Submenu(SubmenuSpec {
                submenu,
                title: lang.text(submenu.label()).to_owned(),
                items,
            })
        }));
    }
    entries
}

/// The state of the toggles in the menu, as the app has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Toggles {
    pub search_case: bool,
    pub search_whole_word: bool,
    pub search_regex: bool,
    pub file_list_tags: bool,
}

impl Toggles {
    /// Whether the toggle of `action` is on; `false` for any other action.
    pub fn is_on(self, action: KeyAction) -> bool {
        match action {
            KeyAction::SearchCase => self.search_case,
            KeyAction::SearchWholeWord => self.search_whole_word,
            KeyAction::SearchRegex => self.search_regex,
            KeyAction::FileListTagsToggle => self.file_list_tags,
            _ => false,
        }
    }
}

/// Everything about the app the menu shows besides the commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuState {
    pub availability: Availability,
    pub toggles: Toggles,
}

/// The system checks a menu's key equivalents before the window sees the key.
/// A bare Enter, Tab, Space, arrow or letter there would stop text fields and
/// the panels from getting them, so only keys with Cmd or Ctrl, and the
/// function keys, are handed over.
pub fn is_safe_as_key_equivalent(binding: &Binding) -> bool {
    binding.modifiers.control() || binding.modifiers.logo() || is_function_key(&binding.key)
}

fn is_function_key(key: &Key) -> bool {
    use key::Named::*;
    matches!(
        key,
        Key::Named(
            F1 | F2
                | F3
                | F4
                | F5
                | F6
                | F7
                | F8
                | F9
                | F10
                | F11
                | F12
                | F13
                | F14
                | F15
                | F16
                | F17
                | F18
                | F19
                | F20
                | F21
                | F22
                | F23
                | F24
        )
    )
}

/// What the menu lets the user run right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// A dialog or box is open: its keys must reach it, and a disabled
    /// entry's key equivalent is not performed by the system.
    Blocked,
    Open(KeyScope),
}

impl Availability {
    pub fn enables(self, action: KeyAction) -> bool {
        if action == KeyAction::Quit {
            return true;
        }
        match self {
            Self::Blocked => false,
            Self::Open(scope) => action.works_in(scope),
        }
    }
}

/// The message of a menu entry, found by its id, which is the action's name.
/// `None` for ids that are not ours, like the system's own entries.
pub fn command_message(id: &str) -> Option<Message> {
    let action = KeyAction::from_name(id)?;
    commands::command_of(action).map(|command| Message::RunCommand(command.action))
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
    use iced::keyboard::Modifiers;

    fn items_in(menu: &MenuSpec) -> impl Iterator<Item = &MenuItemSpec> {
        menu.items.iter().flat_map(MenuEntry::items)
    }

    fn all_items(model: &MenuModel) -> Vec<&MenuItemSpec> {
        model
            .app
            .iter()
            .chain(model.menus.iter().flat_map(items_in))
            .collect()
    }

    fn menu_of(model: &MenuModel, group: CommandGroup) -> &MenuSpec {
        model.menus.iter().find(|menu| menu.group == group).unwrap()
    }

    fn submenu_of(menu: &MenuSpec, submenu: Submenu) -> &SubmenuSpec {
        menu.items
            .iter()
            .find_map(|entry| match entry {
                MenuEntry::Submenu(spec) if spec.submenu == submenu => Some(spec),
                _ => None,
            })
            .unwrap()
    }

    fn item(model: &MenuModel, action: KeyAction) -> &MenuItemSpec {
        all_items(model)
            .into_iter()
            .find(|item| item.action == action)
            .unwrap()
    }

    fn keymap_with(content: &str) -> Keymap {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, content).unwrap();
        (*crate::config::load(&path).unwrap().keymap).clone()
    }

    #[test]
    fn every_command_is_in_the_menu_exactly_once_in_its_group() {
        let model = MenuModel::build(Language::English, &Keymap::default(), Toggles::default());
        let mut seen: Vec<KeyAction> = Vec::new();
        for menu in &model.menus {
            for item in items_in(menu) {
                let command = commands::command_of(item.action).unwrap();
                assert_eq!(command.group, menu.group, "{}", item.action.name());
                assert!(
                    !APP_MENU_ACTIONS.contains(&item.action),
                    "{} is in the app menu",
                    item.action.name()
                );
                seen.push(item.action);
            }
        }
        assert_eq!(
            model.app.iter().map(|item| item.action).collect::<Vec<_>>(),
            [KeyAction::About, KeyAction::Quit]
        );
        seen.extend(APP_MENU_ACTIONS);
        let mut expected: Vec<KeyAction> = commands::all().map(|command| command.action).collect();
        seen.sort();
        expected.sort();
        assert_eq!(seen, expected);
    }

    #[test]
    fn about_leads_the_app_menu_and_is_a_help_command() {
        let model = MenuModel::build(
            Language::EnglishFuturama,
            &Keymap::default(),
            Toggles::default(),
        );
        assert_eq!(
            model.app.first().map(|item| item.action),
            Some(KeyAction::About)
        );
        assert_eq!(
            model.app.first().map(|item| item.title.as_str()),
            Some("About ncrs (Planet Express crew roster)")
        );
        assert_eq!(
            commands::command_of(KeyAction::About).map(|command| command.group),
            Some(CommandGroup::Help)
        );
    }

    #[test]
    fn the_menus_come_in_the_order_of_the_groups() {
        let model = MenuModel::build(Language::English, &Keymap::default(), Toggles::default());
        let groups: Vec<CommandGroup> = model.menus.iter().map(|menu| menu.group).collect();
        assert_eq!(groups, CommandGroup::ALL);
        assert!(model
            .menus
            .iter()
            .find(|menu| menu.group == CommandGroup::Window)
            .unwrap()
            .items
            .is_empty());
    }

    #[test]
    fn titles_are_plain_with_the_theme_name_after_them() {
        let english = MenuModel::build(
            Language::EnglishFuturama,
            &Keymap::default(),
            Toggles::default(),
        );
        assert_eq!(
            item(&english, KeyAction::Search).title,
            "Search in files (Smell-o-scope)"
        );
        assert_eq!(item(&english, KeyAction::Copy).title, "Copy to other panel");
        let hermes = english
            .menus
            .iter()
            .find(|menu| menu.group == CommandGroup::Hermes)
            .unwrap();
        assert_eq!(hermes.title, "Hermes (Hermes)");
        let german = MenuModel::build(
            Language::GermanFuturama,
            &Keymap::default(),
            Toggles::default(),
        );
        assert!(item(&german, KeyAction::Search).title.ends_with(')'));
        assert_ne!(
            item(&german, KeyAction::Copy).title,
            item(&english, KeyAction::Copy).title
        );
    }

    #[test]
    fn the_standard_languages_have_titles_without_a_theme_name() {
        for lang in [Language::English, Language::German] {
            let model = MenuModel::build(lang, &Keymap::default(), Toggles::default());
            assert!(!item(&model, KeyAction::Search).title.contains('('));
            assert!(model.menus.iter().all(|menu| !menu.title.contains('(')));
        }
        let english = MenuModel::build(Language::English, &Keymap::default(), Toggles::default());
        assert_eq!(item(&english, KeyAction::Search).title, "Search in files");
    }

    #[test]
    fn an_entry_shows_the_first_key_that_is_safe_for_the_system() {
        let model = MenuModel::build(Language::English, &Keymap::default(), Toggles::default());
        assert_eq!(
            item(&model, KeyAction::Quit).accelerator,
            Some(Binding::with_modifiers(
                Key::Character("q".into()),
                Modifiers::COMMAND
            ))
        );
        assert_eq!(
            item(&model, KeyAction::Copy).accelerator,
            Some(Binding::key(Key::Named(key::Named::F5)))
        );
        // Alt+F7 comes before Cmd+Shift+F in the keymap.
        assert_eq!(
            item(&model, KeyAction::Search).accelerator,
            Some(Binding::with_modifiers(
                Key::Named(key::Named::F7),
                Modifiers::ALT
            ))
        );
    }

    #[test]
    fn bare_keys_and_alt_letters_get_no_key() {
        let model = MenuModel::build(Language::English, &Keymap::default(), Toggles::default());
        for action in [
            KeyAction::Open,
            KeyAction::GoUp,
            KeyAction::SwitchPanel,
            KeyAction::ToggleTag,
            KeyAction::TagAll,
            KeyAction::SearchCase,
        ] {
            assert_eq!(item(&model, action).accelerator, None, "{}", action.name());
        }
    }

    #[test]
    fn a_keys_override_shows_in_the_menu() {
        let keymap = keymap_with("[keys]\ncopy = \"primary+shift+y\"\nmkdir = []\n");
        let model = MenuModel::build(Language::English, &keymap, Toggles::default());
        assert_eq!(
            item(&model, KeyAction::Copy).accelerator,
            Some(Binding::with_modifiers(
                Key::Character("y".into()),
                Modifiers::COMMAND | Modifiers::SHIFT
            ))
        );
        assert_eq!(item(&model, KeyAction::Mkdir).accelerator, None);
    }

    #[test]
    fn a_menu_id_leads_to_the_message_of_its_command() {
        for command in commands::all() {
            assert_eq!(
                command_message(command.action.name()),
                Some(Message::RunCommand(command.action))
            );
        }
        assert_eq!(command_message("cursor_up"), None);
        assert_eq!(command_message("1234"), None);
    }

    #[test]
    fn only_quit_is_enabled_while_a_dialog_is_open() {
        for command in commands::all() {
            assert_eq!(
                Availability::Blocked.enables(command.action),
                command.action == KeyAction::Quit,
                "{}",
                command.action.name()
            );
        }
    }

    #[test]
    fn the_search_enables_only_what_works_there() {
        let search = Availability::Open(KeyScope::Search);
        assert!(search.enables(KeyAction::SearchRegex));
        assert!(search.enables(KeyAction::Quit));
        assert!(!search.enables(KeyAction::Mkdir));
        let panels = Availability::Open(KeyScope::Panel);
        assert!(panels.enables(KeyAction::Mkdir));
        assert!(!panels.enables(KeyAction::SearchRegex));
    }

    fn english() -> MenuModel {
        MenuModel::build(Language::English, &Keymap::default(), Toggles::default())
    }

    #[test]
    fn no_menu_shows_more_than_seven_entries_without_a_separator() {
        for lang in [Language::English, Language::German] {
            let model = MenuModel::build(lang, &Keymap::default(), Toggles::default());
            for menu in &model.menus {
                for run in menu.items.split(|entry| *entry == MenuEntry::Separator) {
                    assert!(
                        run.len() <= 7,
                        "{}: {} entries in a run",
                        menu.title,
                        run.len()
                    );
                    assert!(
                        !run.is_empty(),
                        "{}: a separator without entries",
                        menu.title
                    );
                }
            }
        }
    }

    #[test]
    fn the_file_menu_has_its_sections_between_separators() {
        let model = english();
        let file = menu_of(&model, CommandGroup::File);
        let sections: Vec<Vec<KeyAction>> = file
            .items
            .split(|entry| *entry == MenuEntry::Separator)
            .map(|run| {
                run.iter()
                    .flat_map(MenuEntry::items)
                    .map(|i| i.action)
                    .collect()
            })
            .collect();
        assert!(sections.len() >= 4);
        assert_eq!(
            sections.first().map(Vec::as_slice),
            Some([KeyAction::Open, KeyAction::View, KeyAction::Edit].as_slice())
        );
        assert!(sections[1].contains(&KeyAction::Copy));
        assert!(sections[1].contains(&KeyAction::Move));
        assert!(sections.iter().any(|run| run.contains(&KeyAction::Trash)
            && run.contains(&KeyAction::Delete)
            && !run.contains(&KeyAction::Copy)));
    }

    #[test]
    fn hermes_keeps_review_and_setup_in_submenus_and_go_its_bookmarks_and_places() {
        let model = english();
        let hermes = menu_of(&model, CommandGroup::Hermes);
        let review = submenu_of(hermes, Submenu::HermesReview);
        assert_eq!(review.title, "Review");
        assert!(review
            .items
            .iter()
            .any(|i| i.action == KeyAction::FilingStatus));
        let setup = submenu_of(hermes, Submenu::HermesSetup);
        assert!(setup
            .items
            .iter()
            .any(|i| i.action == KeyAction::HermesSetInbox));
        assert!(hermes
            .items
            .iter()
            .any(|entry| matches!(entry, MenuEntry::Item(i) if i.action == KeyAction::Rules)));
        let go = menu_of(&model, CommandGroup::Go);
        let bookmarks = submenu_of(go, Submenu::Bookmarks);
        assert!(bookmarks
            .items
            .iter()
            .any(|i| i.action == KeyAction::Favourites));
        let places = submenu_of(go, Submenu::Places);
        assert!(places
            .items
            .iter()
            .any(|i| i.action == KeyAction::DriveMenuLeft));
        assert!(!go
            .items
            .iter()
            .any(|entry| matches!(entry, MenuEntry::Item(i) if i.action == KeyAction::Favourites)));
        let german = MenuModel::build(Language::German, &Keymap::default(), Toggles::default());
        assert_eq!(
            submenu_of(menu_of(&german, CommandGroup::Go), Submenu::Bookmarks).title,
            "Lesezeichen"
        );
    }

    #[test]
    fn an_entry_that_opens_a_dialog_ends_with_an_ellipsis() {
        let model = english();
        assert_eq!(item(&model, KeyAction::Rename).title, "Rename\u{2026}");
        assert_eq!(item(&model, KeyAction::Copy).title, "Copy to other panel");
        let german = MenuModel::build(Language::German, &Keymap::default(), Toggles::default());
        assert!(item(&german, KeyAction::Rename).title.ends_with('\u{2026}'));
    }

    #[test]
    fn a_toggle_is_a_check_item_that_follows_the_state_passed_in() {
        let off = english();
        assert_eq!(item(&off, KeyAction::SearchRegex).checked, Some(false));
        assert_eq!(
            item(&off, KeyAction::FileListTagsToggle).checked,
            Some(false)
        );
        assert_eq!(item(&off, KeyAction::Copy).checked, None);
        let on = MenuModel::build(
            Language::English,
            &Keymap::default(),
            Toggles {
                search_regex: true,
                file_list_tags: true,
                ..Toggles::default()
            },
        );
        assert_eq!(item(&on, KeyAction::SearchRegex).checked, Some(true));
        assert_eq!(item(&on, KeyAction::SearchCase).checked, Some(false));
        assert_eq!(item(&on, KeyAction::FileListTagsToggle).checked, Some(true));
    }
}
