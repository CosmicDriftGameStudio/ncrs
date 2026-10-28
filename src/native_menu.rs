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
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

use crate::commands::CommandGroup;
use crate::i18n::{Language, Msg};
use crate::keymap::{Binding, KeyAction, Keymap};
use crate::menu::{self, Availability, MenuItemSpec, MenuModel};
use crate::messages::Message;
use iced::keyboard::Key;

struct Installed {
    /// Held so the system menu lives as long as this record.
    _menu: Menu,
    items: Vec<(KeyAction, MenuItem)>,
    lang: Language,
    keymap: Arc<Keymap>,
    availability: Availability,
}

thread_local! {
    static INSTALLED: RefCell<Option<Installed>> = const { RefCell::new(None) };
}

/// Replaces the menu bar. Must run on the main thread: muda refuses to make
/// menu items anywhere else.
pub fn install(lang: Language, keymap: &Arc<Keymap>, availability: Availability) {
    match build(lang, keymap, availability) {
        Ok(installed) => INSTALLED.set(Some(installed)),
        Err(error) => eprintln!("ncrs: the menu bar could not be built: {error}"),
    }
}

/// Brings an installed menu up to date: new titles after a language or keymap
/// change, and only the entries whose enabled state changed otherwise.
/// Does nothing before `install`.
pub fn sync(lang: Language, keymap: &Arc<Keymap>, availability: Availability) {
    INSTALLED.with_borrow_mut(|slot| {
        let Some(installed) = slot.as_mut() else {
            return;
        };
        if installed.lang != lang || !Arc::ptr_eq(&installed.keymap, keymap) {
            match build(lang, keymap, availability) {
                Ok(rebuilt) => *slot = Some(rebuilt),
                Err(error) => eprintln!("ncrs: the menu bar could not be rebuilt: {error}"),
            }
            return;
        }
        if installed.availability == availability {
            return;
        }
        for (action, item) in &installed.items {
            let enabled = availability.enables(*action);
            if enabled != installed.availability.enables(*action) {
                item.set_enabled(enabled);
            }
        }
        installed.availability = availability;
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

fn build(
    lang: Language,
    keymap: &Arc<Keymap>,
    availability: Availability,
) -> muda::Result<Installed> {
    let model = MenuModel::build(lang, keymap);
    let mut items: Vec<(KeyAction, MenuItem)> = Vec::new();
    let mut item_of = |spec: &MenuItemSpec| {
        let item = MenuItem::with_id(
            spec.action.name(),
            &spec.title,
            availability.enables(spec.action),
            spec.accelerator.as_ref().and_then(accelerator_of),
        );
        items.push((spec.action, item.clone()));
        item
    };
    let text = |msg: Msg| Some(lang.text(msg));

    let menu = Menu::new();

    let app_menu = Submenu::new(lang.text(Msg::MenuApp), true);
    let (about, quit) = model.app.split_at(model.app.len().min(1));
    for spec in about {
        app_menu.append(&item_of(spec))?;
    }
    app_menu.append(&PredefinedMenuItem::separator())?;
    app_menu.append(&PredefinedMenuItem::services(text(Msg::MenuServices)))?;
    app_menu.append(&PredefinedMenuItem::separator())?;
    app_menu.append(&PredefinedMenuItem::hide(text(Msg::MenuHide)))?;
    app_menu.append(&PredefinedMenuItem::hide_others(text(Msg::MenuHideOthers)))?;
    app_menu.append(&PredefinedMenuItem::show_all(text(Msg::MenuShowAll)))?;
    app_menu.append(&PredefinedMenuItem::separator())?;
    for spec in quit {
        app_menu.append(&item_of(spec))?;
    }
    menu.append(&app_menu)?;

    for spec in &model.menus {
        let submenu = Submenu::new(&spec.title, true);
        for entry in &spec.items {
            submenu.append(&item_of(entry))?;
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
        menu.append(&submenu)?;
    }

    menu.init_for_nsapp();
    Ok(Installed {
        _menu: menu,
        items,
        lang,
        keymap: Arc::clone(keymap),
        availability,
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
        let model = MenuModel::build(Language::English, &keymap);
        for spec in model
            .app
            .iter()
            .chain(model.menus.iter().flat_map(|menu| &menu.items))
        {
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
