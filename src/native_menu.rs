//! The macOS menu bar, made of the menu model.
//!
//! winit installs its own default menu at launch and iced has no switch to
//! turn that off, so `install` replaces it once the app runs. The system's
//! menu objects belong to the main thread and are not `Send`; they live in a
//! thread-local of the thread that runs `update`, which is the main thread.

use std::cell::RefCell;
use std::sync::Arc;

use iced::Subscription;
use muda::accelerator::{Accelerator, Code, Modifiers as MenuModifiers};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

use crate::commands::CommandGroup;
use crate::i18n::{Language, Msg};
use crate::keymap::{Binding, KeyAction, Keymap};
use crate::menu::{self, Availability, MenuEntry, MenuItemSpec, MenuModel, MenuState};
use crate::messages::Message;
use iced::keyboard::Key;

/// An entry the state can change after the menu is built.
enum Entry {
    Plain(MenuItem),
    Check(CheckMenuItem),
}

impl Entry {
    fn set_enabled(&self, enabled: bool) {
        match self {
            Self::Plain(item) => item.set_enabled(enabled),
            Self::Check(item) => item.set_enabled(enabled),
        }
    }
}

struct Installed {
    /// Held so the system menu lives as long as this record.
    _menu: Menu,
    /// Every entry with an action, those in submenus too.
    items: Vec<(KeyAction, Entry)>,
    lang: Language,
    keymap: Arc<Keymap>,
    state: MenuState,
}

thread_local! {
    static INSTALLED: RefCell<Option<Installed>> = const { RefCell::new(None) };
}

/// Replaces the menu bar. Must run on the main thread: muda refuses to make
/// menu items anywhere else.
pub fn install(lang: Language, keymap: &Arc<Keymap>, state: MenuState) {
    match build(lang, keymap, state) {
        Ok(installed) => INSTALLED.set(Some(installed)),
        Err(error) => eprintln!("ncrs: the menu bar could not be built: {error}"),
    }
}

/// Brings an installed menu up to date: new titles after a language or keymap
/// change, and only the entries whose enabled or checked state changed
/// otherwise. Does nothing before `install`.
pub fn sync(lang: Language, keymap: &Arc<Keymap>, state: MenuState) {
    INSTALLED.with_borrow_mut(|slot| {
        let Some(installed) = slot.as_mut() else {
            return;
        };
        if installed.lang != lang || !Arc::ptr_eq(&installed.keymap, keymap) {
            match build(lang, keymap, state) {
                Ok(rebuilt) => *slot = Some(rebuilt),
                Err(error) => eprintln!("ncrs: the menu bar could not be rebuilt: {error}"),
            }
            return;
        }
        for (action, entry) in &installed.items {
            let enabled = state.availability.enables(*action);
            if enabled != installed.state.availability.enables(*action) {
                entry.set_enabled(enabled);
            }
            // Compared with the item itself, not with the last state: the
            // system flips a check item when it is chosen, even when the app
            // then leaves the toggle as it was.
            if let Entry::Check(item) = entry {
                let on = state.toggles.is_on(*action);
                if item.is_checked() != on {
                    item.set_checked(on);
                }
            }
        }
        installed.state = state;
    });
}

/// The messages of the menu's entries as they are chosen.
pub fn events() -> Subscription<Message> {
    Subscription::run(menu_event_ids).filter_map(|id| menu::command_message(&id))
}

fn menu_event_ids() -> impl iced::futures::Stream<Item = String> {
    use iced::futures::{channel::mpsc, SinkExt as _, StreamExt as _};
    iced::stream::channel(8, async |mut output| {
        // The system's receiver blocks, so it waits on a thread of its own
        // that hands the ids on.
        let (ids, mut received) = mpsc::unbounded();
        std::thread::spawn(move || {
            while let Ok(event) = MenuEvent::receiver().recv() {
                if ids.unbounded_send(event.id.0).is_err() {
                    break;
                }
            }
        });
        while let Some(id) = received.next().await {
            if output.send(id).await.is_err() {
                break;
            }
        }
    })
}

/// Makes the system's items of a menu and remembers those the state can
/// change.
struct Maker {
    availability: Availability,
    items: Vec<(KeyAction, Entry)>,
}

impl Maker {
    fn append_item(&mut self, parent: &Submenu, spec: &MenuItemSpec) -> muda::Result<()> {
        let enabled = self.availability.enables(spec.action);
        let accelerator = spec.accelerator.as_ref().and_then(accelerator_of);
        let entry = match spec.checked {
            Some(checked) => {
                let item = CheckMenuItem::with_id(
                    spec.action.name(),
                    &spec.title,
                    enabled,
                    checked,
                    accelerator,
                );
                parent.append(&item)?;
                Entry::Check(item)
            }
            None => {
                let item = MenuItem::with_id(spec.action.name(), &spec.title, enabled, accelerator);
                parent.append(&item)?;
                Entry::Plain(item)
            }
        };
        self.items.push((spec.action, entry));
        Ok(())
    }

    fn append_entry(&mut self, parent: &Submenu, entry: &MenuEntry) -> muda::Result<()> {
        match entry {
            MenuEntry::Item(spec) => self.append_item(parent, spec),
            MenuEntry::Separator => parent.append(&PredefinedMenuItem::separator()),
            MenuEntry::Submenu(spec) => {
                let submenu = Submenu::new(&spec.title, true);
                for item in &spec.items {
                    self.append_item(&submenu, item)?;
                }
                parent.append(&submenu)
            }
        }
    }

    fn app_menu(&mut self, lang: Language, specs: &[MenuItemSpec]) -> muda::Result<Submenu> {
        let text = |msg: Msg| Some(lang.text(msg));
        let app_menu = Submenu::new(lang.text(Msg::MenuApp), true);
        let (about, quit) = specs.split_at(specs.len().min(1));
        for spec in about {
            self.append_item(&app_menu, spec)?;
        }
        app_menu.append(&PredefinedMenuItem::separator())?;
        app_menu.append(&PredefinedMenuItem::services(text(Msg::MenuServices)))?;
        app_menu.append(&PredefinedMenuItem::separator())?;
        app_menu.append(&PredefinedMenuItem::hide(text(Msg::MenuHide)))?;
        app_menu.append(&PredefinedMenuItem::hide_others(text(Msg::MenuHideOthers)))?;
        app_menu.append(&PredefinedMenuItem::show_all(text(Msg::MenuShowAll)))?;
        app_menu.append(&PredefinedMenuItem::separator())?;
        for spec in quit {
            self.append_item(&app_menu, spec)?;
        }
        Ok(app_menu)
    }

    fn group_menu(&mut self, lang: Language, spec: &menu::MenuSpec) -> muda::Result<Submenu> {
        let text = |msg: Msg| Some(lang.text(msg));
        let submenu = Submenu::new(&spec.title, true);
        for entry in &spec.items {
            self.append_entry(&submenu, entry)?;
        }
        match spec.group {
            CommandGroup::Window => {
                submenu.append(&PredefinedMenuItem::minimize(text(Msg::MenuMinimize)))?;
                submenu.append(&PredefinedMenuItem::maximize(text(Msg::MenuZoom)))?;
                submenu.append(&PredefinedMenuItem::fullscreen(text(Msg::MenuFullscreen)))?;
                submenu.append(&PredefinedMenuItem::separator())?;
                submenu.append(&PredefinedMenuItem::bring_all_to_front(text(
                    Msg::MenuBringAllToFront,
                )))?;
                submenu.set_as_windows_menu_for_nsapp();
            }
            CommandGroup::Help => submenu.set_as_help_menu_for_nsapp(),
            CommandGroup::File
            | CommandGroup::Edit
            | CommandGroup::View
            | CommandGroup::Go
            | CommandGroup::Search
            | CommandGroup::Hermes => {}
        }
        Ok(submenu)
    }
}

fn build(lang: Language, keymap: &Arc<Keymap>, state: MenuState) -> muda::Result<Installed> {
    let model = MenuModel::build(lang, keymap, state.toggles);
    let mut maker = Maker {
        availability: state.availability,
        items: Vec::new(),
    };
    let menu = Menu::new();
    menu.append(&maker.app_menu(lang, &model.app)?)?;
    for spec in &model.menus {
        menu.append(&maker.group_menu(lang, spec)?)?;
    }
    menu.init_for_nsapp();
    Ok(Installed {
        _menu: menu,
        items: maker.items,
        lang,
        keymap: Arc::clone(keymap),
        state,
    })
}

/// The system's description of a key. `None` for a key it has no name for;
/// the entry then shows no shortcut.
fn accelerator_of(binding: &Binding) -> Option<Accelerator> {
    let code = match &binding.key {
        Key::Named(named) => format!("{named:?}").parse::<Code>().ok()?,
        Key::Character(character) => {
            let mut chars = character.chars();
            let (Some(letter), None) = (chars.next(), chars.next()) else {
                return None;
            };
            let name = if letter.is_ascii_alphabetic() {
                format!("Key{}", letter.to_ascii_uppercase())
            } else if letter.is_ascii_digit() {
                format!("Digit{letter}")
            } else {
                return None;
            };
            name.parse::<Code>().ok()?
        }
        Key::Unidentified => return None,
    };
    let mut modifiers = MenuModifiers::empty();
    for (held, mapped) in [
        (binding.modifiers.logo(), MenuModifiers::META),
        (binding.modifiers.control(), MenuModifiers::CONTROL),
        (binding.modifiers.alt(), MenuModifiers::ALT),
        (binding.modifiers.shift(), MenuModifiers::SHIFT),
    ] {
        if held {
            modifiers |= mapped;
        }
    }
    Some(Accelerator::new(modifiers, code))
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
    use crate::menu::Toggles;
    use iced::keyboard::{key, Modifiers};

    #[test]
    fn cmd_ctrl_alt_and_shift_map_to_the_systems_modifiers() {
        let binding = Binding::with_modifiers(
            Key::Character("f".into()),
            Modifiers::LOGO | Modifiers::CTRL | Modifiers::ALT | Modifiers::SHIFT,
        );
        assert_eq!(
            accelerator_of(&binding),
            Some(Accelerator::new(
                MenuModifiers::META
                    | MenuModifiers::CONTROL
                    | MenuModifiers::ALT
                    | MenuModifiers::SHIFT,
                Code::KeyF
            ))
        );
    }

    #[test]
    fn letters_digits_and_named_keys_map_to_codes() {
        let cmd = |key: Key| Binding::with_modifiers(key, Modifiers::LOGO);
        assert_eq!(
            accelerator_of(&cmd(Key::Character("q".into()))),
            Some(Accelerator::new(MenuModifiers::META, Code::KeyQ))
        );
        assert_eq!(
            accelerator_of(&cmd(Key::Character("1".into()))),
            Some(Accelerator::new(MenuModifiers::META, Code::Digit1))
        );
        assert_eq!(
            accelerator_of(&cmd(Key::Named(key::Named::ArrowUp))),
            Some(Accelerator::new(MenuModifiers::META, Code::ArrowUp))
        );
        assert_eq!(
            accelerator_of(&Binding::key(Key::Named(key::Named::F10))),
            Some(Accelerator::new(MenuModifiers::empty(), Code::F10))
        );
    }

    #[test]
    fn a_key_the_system_has_no_name_for_gets_no_shortcut() {
        assert_eq!(
            accelerator_of(&Binding::with_modifiers(
                Key::Character("*".into()),
                Modifiers::CTRL
            )),
            None
        );
    }

    #[test]
    fn keys_that_would_break_typing_are_not_handed_over_at_all() {
        let keymap = Keymap::default();
        let model = MenuModel::build(Language::English, &keymap, Toggles::default());
        for spec in model.app.iter().chain(
            model
                .menus
                .iter()
                .flat_map(|menu| menu.items.iter().flat_map(MenuEntry::items)),
        ) {
            if let Some(binding) = &spec.accelerator {
                assert!(menu::is_safe_as_key_equivalent(binding));
            }
        }
        for bare in [
            Binding::key(Key::Named(key::Named::Enter)),
            Binding::key(Key::Named(key::Named::Space)),
            Binding::with_modifiers(Key::Character("c".into()), Modifiers::ALT),
        ] {
            assert!(!menu::is_safe_as_key_equivalent(&bare));
        }
    }
}
