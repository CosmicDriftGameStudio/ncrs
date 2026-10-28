//! The menu bar as data: which menus there are, what they hold and which key
//! shows beside each entry. Built from the command list, so the menu and the
//! Lab cannot disagree; the native layer only turns it into system menus.

use iced::keyboard::{key, Key};

use crate::commands::{self, Command, CommandGroup};
use crate::i18n::{Language, Msg};
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
    pub items: Vec<MenuItemSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItemSpec {
    pub action: KeyAction,
    pub title: String,
    /// The key shown beside the entry, when it is safe to hand to the system.
    pub accelerator: Option<Binding>,
}

const APP_MENU_ACTIONS: [KeyAction; 2] = [KeyAction::About, KeyAction::Quit];

impl MenuModel {
    pub fn build(lang: Language, keymap: &Keymap) -> Self {
        let item_of = |command: Command| MenuItemSpec {
            action: command.action,
            title: titled(lang, command.label, command.theme_name),
            accelerator: keymap
                .bindings_of(command.action)
                .find(|binding| is_safe_as_key_equivalent(binding))
                .cloned(),
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
                title: titled(lang, group.label(), group.theme_name()),
                items: commands::all()
                    .filter(|command| {
                        command.group == group && !APP_MENU_ACTIONS.contains(&command.action)
                    })
                    .map(item_of)
                    .collect(),
            })
            .collect();
        Self { app, menus }
    }
}

/// The plain name, then the theme name in parentheses where there is one.
fn titled(lang: Language, label: Msg, theme_name: Option<Msg>) -> String {
    match theme_name {
        Some(theme) => format!("{} ({})", lang.text(label), lang.text(theme)),
        None => lang.text(label).to_owned(),
    }
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

    fn all_items(model: &MenuModel) -> Vec<&MenuItemSpec> {
        model
            .app
            .iter()
            .chain(model.menus.iter().flat_map(|menu| &menu.items))
            .collect()
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
        let model = MenuModel::build(Language::English, &Keymap::default());
        let mut seen: Vec<KeyAction> = Vec::new();
        for menu in &model.menus {
            for item in &menu.items {
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
        let model = MenuModel::build(Language::English, &Keymap::default());
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
        let model = MenuModel::build(Language::English, &Keymap::default());
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
        let english = MenuModel::build(Language::English, &Keymap::default());
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
        assert_eq!(hermes.title, "Filing (Hermes)");
        let german = MenuModel::build(Language::German, &Keymap::default());
        assert!(item(&german, KeyAction::Search).title.ends_with(')'));
        assert_ne!(
            item(&german, KeyAction::Copy).title,
            item(&english, KeyAction::Copy).title
        );
    }

    #[test]
    fn an_entry_shows_the_first_key_that_is_safe_for_the_system() {
        let model = MenuModel::build(Language::English, &Keymap::default());
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
        let model = MenuModel::build(Language::English, &Keymap::default());
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
        let model = MenuModel::build(Language::English, &keymap);
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
}
