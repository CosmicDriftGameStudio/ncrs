//! What a key does while the System page is up.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::message::MonitorMessage;
use crate::state::{State, KILL_OFFERED};
use crate::tasks::TaskColumn;

#[derive(Debug, Clone)]
pub enum MonitorKey {
    Message(MonitorMessage),
    /// The key is bound to something that is not the page's: the app sends
    /// that binding on.
    Bound,
}

/// What the page and the app know about the press.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyFacts {
    pub filter_focused: bool,
    pub dialog_open: bool,
    pub menu_open: bool,
    /// The press is the keymap's key for the context menu (Shift+F10).
    pub context_menu_pressed: bool,
    /// The press is bound to something and is not a key a text field types.
    pub bound_untyped: bool,
}

impl KeyFacts {
    pub fn of(state: &State, context_menu_pressed: bool, bound_untyped: bool) -> Self {
        Self {
            filter_focused: state.filter_focused,
            dialog_open: state.kill_dialog.is_some(),
            menu_open: state.menu.is_some(),
            context_menu_pressed,
            bound_untyped,
        }
    }
}

fn is_letter(key: &Key, letter: &str) -> bool {
    matches!(key.as_ref(), Key::Character(typed) if typed.eq_ignore_ascii_case(letter))
}

/// Cmd+F on a Mac, Ctrl+F elsewhere.
fn is_find(key: &Key, modifiers: Modifiers) -> bool {
    modifiers == Modifiers::COMMAND && is_letter(key, "f")
}

pub fn monitor_key(key: &Key, modifiers: Modifiers, facts: KeyFacts) -> Option<MonitorKey> {
    use MonitorMessage as M;
    let message = |message| Some(MonitorKey::Message(message));
    let plain = (modifiers - Modifiers::SHIFT).is_empty();
    let named = |wanted: Named| matches!(key.as_ref(), Key::Named(pressed) if pressed == wanted);

    if named(Named::Escape) {
        return message(M::Escape);
    }
    if facts.dialog_open {
        return if named(Named::Enter) {
            message(M::DialogEnter)
        } else if named(Named::Tab) || named(Named::ArrowLeft) || named(Named::ArrowRight) {
            message(M::KillFocus)
        } else if facts.bound_untyped {
            Some(MonitorKey::Bound)
        } else {
            None
        };
    }
    if facts.menu_open {
        return if facts.context_menu_pressed {
            message(M::MenuClose)
        } else if named(Named::ArrowUp) {
            message(M::MenuMove(-1))
        } else if named(Named::ArrowDown) {
            message(M::MenuMove(1))
        } else if named(Named::Enter) {
            message(M::MenuActivate)
        } else if facts.bound_untyped {
            Some(MonitorKey::Bound)
        } else {
            None
        };
    }
    if is_find(key, modifiers) {
        return message(M::FocusFilter);
    }
    if facts.context_menu_pressed {
        return message(M::OpenMenuAtCursor);
    }
    if plain {
        let moved = match key.as_ref() {
            Key::Named(Named::ArrowUp) => Some(M::Move(-1)),
            Key::Named(Named::ArrowDown) => Some(M::Move(1)),
            Key::Named(Named::PageUp) => Some(M::PageMove(-1)),
            Key::Named(Named::PageDown) => Some(M::PageMove(1)),
            Key::Named(Named::Home) => Some(M::First),
            Key::Named(Named::End) => Some(M::Last),
            Key::Named(Named::Enter) if facts.filter_focused => Some(M::FilterDone),
            _ => None,
        };
        if moved.is_some() {
            return moved.map(MonitorKey::Message);
        }
    }
    // A field that has the focus types the letters itself; what reaches here
    // from it is not text.
    if !facts.filter_focused {
        if plain && named(Named::ArrowLeft) {
            return message(M::ShiftCommands(-1));
        }
        if plain && named(Named::ArrowRight) {
            return message(M::ShiftCommands(1));
        }
        if modifiers.is_empty() {
            if is_letter(key, "c") {
                return message(M::SortBy(TaskColumn::Cpu));
            }
            if is_letter(key, "m") {
                return message(M::SortBy(TaskColumn::Memory));
            }
            if is_letter(key, "t") {
                return message(M::SortBy(TaskColumn::Time));
            }
            if is_letter(key, "g") {
                return message(M::ToggleGroups);
            }
            if KILL_OFFERED && is_letter(key, "k") {
                return message(M::AskKill);
            }
        }
    }
    facts.bound_untyped.then_some(MonitorKey::Bound)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character(letter: &str) -> Key {
        Key::Character(letter.into())
    }

    fn pressed(key: &Key, modifiers: Modifiers, facts: KeyFacts) -> Option<MonitorMessage> {
        match monitor_key(key, modifiers, facts)? {
            MonitorKey::Message(message) => Some(message),
            MonitorKey::Bound => None,
        }
    }

    #[test]
    fn letters_sort_and_ask_to_kill_while_the_list_has_the_keys() {
        let none = Modifiers::empty();
        let facts = KeyFacts::default();
        assert!(matches!(
            pressed(&character("c"), none, facts),
            Some(MonitorMessage::SortBy(TaskColumn::Cpu))
        ));
        assert!(matches!(
            pressed(&character("t"), none, facts),
            Some(MonitorMessage::SortBy(TaskColumn::Time))
        ));
        assert_eq!(
            matches!(
                pressed(&character("k"), none, facts),
                Some(MonitorMessage::AskKill)
            ),
            KILL_OFFERED
        );
    }

    #[test]
    fn letters_are_only_text_while_the_filter_field_has_the_focus() {
        let facts = KeyFacts {
            filter_focused: true,
            ..KeyFacts::default()
        };
        for letter in ["c", "m", "t", "k"] {
            assert!(pressed(&character(letter), Modifiers::empty(), facts).is_none());
        }
        assert!(matches!(
            pressed(&Key::Named(Named::ArrowDown), Modifiers::empty(), facts),
            Some(MonitorMessage::Move(1))
        ));
    }

    #[test]
    fn a_letter_with_a_modifier_is_not_a_sort_key() {
        assert!(pressed(&character("c"), Modifiers::COMMAND, KeyFacts::default()).is_none());
    }

    #[test]
    fn find_focuses_the_filter() {
        assert!(matches!(
            pressed(&character("f"), Modifiers::COMMAND, KeyFacts::default()),
            Some(MonitorMessage::FocusFilter)
        ));
    }

    #[test]
    fn escape_always_goes_to_the_page_which_closes_the_top_layer_first() {
        for facts in [
            KeyFacts::default(),
            KeyFacts {
                dialog_open: true,
                ..KeyFacts::default()
            },
            KeyFacts {
                menu_open: true,
                ..KeyFacts::default()
            },
            KeyFacts {
                filter_focused: true,
                ..KeyFacts::default()
            },
        ] {
            assert!(matches!(
                pressed(&Key::Named(Named::Escape), Modifiers::empty(), facts),
                Some(MonitorMessage::Escape)
            ));
        }
    }

    #[test]
    fn the_dialog_keeps_the_other_keys_from_the_list() {
        let facts = KeyFacts {
            dialog_open: true,
            ..KeyFacts::default()
        };
        assert!(pressed(&character("c"), Modifiers::empty(), facts).is_none());
        assert!(pressed(&Key::Named(Named::ArrowDown), Modifiers::empty(), facts).is_none());
        assert!(matches!(
            pressed(&Key::Named(Named::Enter), Modifiers::empty(), facts),
            Some(MonitorMessage::DialogEnter)
        ));
    }

    #[test]
    fn a_bound_key_is_handed_back_to_the_app() {
        let facts = KeyFacts {
            bound_untyped: true,
            ..KeyFacts::default()
        };
        assert!(matches!(
            monitor_key(&Key::Named(Named::F12), Modifiers::empty(), facts),
            Some(MonitorKey::Bound)
        ));
    }
}
