//! Keyboard bindings and the header hints, from one source.
//!
//! An action registers a binding here; the key handler and the header hint list
//! are both derived from it, so a binding cannot exist in one and be missing in
//! the other. This is the extension point a plugin would use.
use iced::keyboard::{key, Key, Modifiers};

use crate::i18n::{Language, Msg};
use crate::messages::Message;
use crate::messages::TransferKind;

/// A key plus the modifier that must be held. A bare press has no modifier.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    ///
    /// Unused until the target-panel bindings land (F6-T7 in the roadmap); the
    /// modifier is carried through `label()` and the lookup already.
    #[allow(dead_code)]
    pub fn with_modifiers(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }
}

/// One action: the key that triggers it, the message it produces, and the
/// translated hint the header shows.
struct Action {
    binding: Binding,
    message: Message,
    /// The translated hint. `None` for actions that are bound but not
    /// advertised, like the arrow keys: the header has room for five or six
    /// hints, not twenty.
    hint: Option<Msg>,
}

/// The bindings this binary knows. Adding a file operation means adding one
/// entry here; the key handler and the header hint list follow from it.
///
/// Built at runtime rather than as a `static` because `Key::Character` needs a
/// `SmolStr`, which is not a const constructor.
fn build_actions() -> Vec<Action> {
    use key::Named::*;
    use Key::{Character, Named as KeyNamed};

    vec![
        // --- Backspace, Enter, Tab and Q are not on the function key bar ---
        Action {
            binding: Binding::key(Character("q".into())),
            message: Message::Quit,
            hint: None,
        },
        Action {
            binding: Binding::key(KeyNamed(Backspace)),
            message: Message::GoUp,
            hint: None,
        },
        Action {
            binding: Binding::key(KeyNamed(Enter)),
            message: Message::OpenSelected,
            hint: None,
        },
        Action {
            binding: Binding::key(KeyNamed(Tab)),
            message: Message::SwitchPanel,
            hint: None,
        },
        Action {
            binding: Binding::key(KeyNamed(F9)),
            message: Message::SwitchLanguage,
            hint: Some(Msg::ShortcutSwitchLanguage),
        },
        // --- selection: tagged rows, not advertised ---
        Action {
            binding: Binding::key(KeyNamed(Insert)),
            message: Message::ToggleTag,
            hint: None,
        },
        Action {
            binding: Binding::key(KeyNamed(Space)),
            message: Message::ToggleTag,
            hint: None,
        },
        Action {
            binding: Binding::with_modifiers(KeyNamed(ArrowDown), Modifiers::SHIFT),
            message: Message::TagMove(1),
            hint: None,
        },
        Action {
            binding: Binding::with_modifiers(KeyNamed(ArrowUp), Modifiers::SHIFT),
            message: Message::TagMove(-1),
            hint: None,
        },
        Action {
            binding: Binding::key(Character("*".into())),
            message: Message::TagAll,
            hint: None,
        },
        Action {
            binding: Binding::with_modifiers(Character("*".into()), Modifiers::CTRL),
            message: Message::ClearTags,
            hint: None,
        },
        // Ctrl+C stops a running job. Registered here rather than handled
        // ad hoc so it shows up in the bindings like every other key.
        Action {
            binding: Binding::with_modifiers(Key::Character("c".into()), Modifiers::CTRL),
            message: Message::AbortJob,
            hint: None,
        },
        Action {
            binding: Binding::key(KeyNamed(F5)),
            message: Message::Transfer(TransferKind::Copy),
            hint: Some(Msg::ShortcutCopy),
        },
        Action {
            binding: Binding::key(KeyNamed(F6)),
            message: Message::Transfer(TransferKind::Move),
            hint: Some(Msg::ShortcutMove),
        },
        Action {
            binding: Binding::key(KeyNamed(F8)),
            message: Message::Delete { permanent: false },
            hint: Some(Msg::ShortcutDelete),
        },
        // Not advertised: the header has no room, and a key that deletes for
        // good should be found in the help, not stumbled over.
        Action {
            binding: Binding::with_modifiers(KeyNamed(F8), Modifiers::SHIFT),
            message: Message::Delete { permanent: true },
            hint: None,
        },
        Action {
            binding: Binding::key(KeyNamed(F7)),
            message: Message::CreateDirPrompt,
            hint: Some(Msg::ShortcutMkdir),
        },
        // Quit is also F10, the Norton Commander convention.
        Action {
            binding: Binding::key(KeyNamed(F10)),
            message: Message::Quit,
            hint: Some(Msg::ShortcutQuit),
        },
        // --- bound, not advertised: the header has no room for these ---
        Action {
            binding: Binding::key(KeyNamed(ArrowUp)),
            message: Message::MoveSelection(-1),
            hint: None,
        },
        Action {
            binding: Binding::key(KeyNamed(ArrowDown)),
            message: Message::MoveSelection(1),
            hint: None,
        },
        Action {
            binding: Binding::key(KeyNamed(PageUp)),
            message: Message::PageUp,
            hint: None,
        },
        Action {
            binding: Binding::key(KeyNamed(PageDown)),
            message: Message::PageDown,
            hint: None,
        },
        Action {
            binding: Binding::key(KeyNamed(Home)),
            message: Message::SelectFirst,
            hint: None,
        },
        Action {
            binding: Binding::key(KeyNamed(End)),
            message: Message::SelectLast,
            hint: None,
        },
    ]
}

/// The binding table, built once on first use.
///
/// `map_key` runs on every key press, and `Action` owns a `Key`, which owns a
/// `SmolStr`. Building the list per press meant an allocation per key repeat;
/// holding a cursor key down made the UI visibly lag behind the keystroke.
fn actions() -> &'static [Action] {
    static ACTIONS: std::sync::OnceLock<Vec<Action>> = std::sync::OnceLock::new();
    ACTIONS.get_or_init(build_actions)
}

/// Maps a key press to a message. Must be a plain `fn` for
/// `iced::keyboard::on_key_press`.
pub fn map_key(key: Key, modifiers: Modifiers) -> Option<Message> {
    actions()
        .iter()
        .find(|action| action.binding.key == key && action.binding.modifiers == modifiers)
        .map(|action| action.message.clone())
}

/// Slots in the function key bar: F1 to F10.
pub const FUNCTION_KEY_COUNT: usize = 10;

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

/// The labels of the function key bar, derived from the bindings above: slot
/// `n - 1` holds the hint of the bare `Fn` binding, `None` for a free key.
/// Only bare function keys are shown; Shift+F8 and friends are in the help.
pub fn function_keys(lang: Language) -> [Option<&'static str>; FUNCTION_KEY_COUNT] {
    let mut slots = [None; FUNCTION_KEY_COUNT];
    for action in actions() {
        let Some(hint) = action.hint else {
            continue;
        };
        if action.binding.modifiers != Modifiers::default() {
            continue;
        }
        let Some(number) = function_key_number(&action.binding.key) else {
            continue;
        };
        if let Some(slot) = slots.get_mut(number - 1) {
            slot.get_or_insert(lang.text(hint));
        }
    }
    slots
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
    use iced::keyboard::key::Named;

    /// The point of this module: a label on the bar whose key does nothing, or
    /// a binding the bar never shows. Both used to be possible.
    #[test]
    fn every_labelled_slot_has_a_binding_that_responds() {
        for (index, label) in function_keys(Language::English).iter().enumerate() {
            let Some(text) = label else { continue };
            let number = index + 1;
            let bound = actions().iter().any(|action| {
                action.hint.is_some()
                    && action.binding.modifiers == Modifiers::default()
                    && function_key_number(&action.binding.key) == Some(number)
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
                None,
                None,
                None,
                None,
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
            map_key(Key::Character("q".into()), Modifiers::default()),
            Some(Message::Quit)
        );
        assert_eq!(
            map_key(Key::Character("x".into()), Modifiers::default()),
            None
        );
    }

    /// A modifier must actually gate the binding: Alt+Enter is not Enter.
    #[test]
    fn modifiers_gate_the_lookup() {
        assert_eq!(map_key(Key::Named(Named::Enter), Modifiers::ALT), None);
    }
}
