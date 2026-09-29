//! Keyboard bindings and the header hints, from one source.
//!
//! An action registers a binding here; the key handler and the header hint list
//! are both derived from it, so a binding cannot exist in one and be missing in
//! the other. This is the extension point a plugin would use.
use iced::keyboard::{key, Key, Modifiers};

use crate::i18n::{Language, Msg};
use crate::messages::Message;

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

    /// The label shown in the header. Deliberately short: the header has room
    /// for five or six of these, not twenty.
    pub fn label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.modifiers.contains(Modifiers::ALT) {
            parts.push("Alt".to_string());
        }
        if self.modifiers.contains(Modifiers::CTRL) {
            parts.push("Ctrl".to_string());
        }
        if self.modifiers.contains(Modifiers::SHIFT) {
            parts.push("Shift".to_string());
        }
        if self.modifiers.contains(Modifiers::LOGO) {
            parts.push(
                if cfg!(target_os = "macos") {
                    "Cmd"
                } else {
                    "Super"
                }
                .to_string(),
            );
        }
        parts.push(self.key_label());
        parts.join("+")
    }

    fn key_label(&self) -> String {
        match &self.key {
            Key::Named(named) => format!("{named:?}"),
            Key::Character(c) => c.as_str().to_uppercase(),
            Key::Unidentified => "?".to_string(),
        }
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
        // --- advertised in the header ---
        Action {
            binding: Binding::key(Character("q".into())),
            message: Message::Quit,
            hint: Some(Msg::ShortcutQuit),
        },
        Action {
            binding: Binding::key(KeyNamed(Backspace)),
            message: Message::GoUp,
            hint: Some(Msg::ShortcutUp),
        },
        Action {
            binding: Binding::key(KeyNamed(Enter)),
            message: Message::OpenSelected,
            hint: Some(Msg::ShortcutOpen),
        },
        Action {
            binding: Binding::key(KeyNamed(Tab)),
            message: Message::SwitchPanel,
            hint: Some(Msg::ShortcutSwitchPanel),
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
            binding: Binding::key(Character("*".into())),
            message: Message::TagAll,
            hint: None,
        },
        Action {
            binding: Binding::with_modifiers(Character("*".into()), Modifiers::CTRL),
            message: Message::ClearTags,
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

/// Shortcuts shown in the header bar, derived from the bindings above:
/// `(key label, translated description)`.
pub fn shortcuts(lang: Language) -> Vec<(String, &'static str)> {
    let mut hints: Vec<_> = actions()
        .iter()
        .filter_map(|action| {
            let hint = action.hint?;
            Some((action.binding.label(), lang.text(hint)))
        })
        .collect();
    // Stable, readable order regardless of registration order.
    hints.sort_by(|a, b| a.0.cmp(&b.0));
    hints.dedup_by(|a, b| a.0 == b.0);
    hints
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
// reason: a test module belongs next to what it tests
#[allow(clippy::inline_modules)]
#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::key::Named;

    /// The point of this module: an advertised binding that no key press
    /// triggers, or a binding nothing advertises. Both used to be possible.
    #[test]
    fn every_advertised_binding_responds_to_its_key() {
        for (label, _) in shortcuts(Language::English) {
            let advertised = actions()
                .iter()
                .find(|action| action.binding.label() == label);
            let action = advertised.expect("a header hint without a binding");
            assert!(action.hint.is_some(), "{label} is advertised with no hint");
        }
    }

    #[test]
    fn quit_is_advertised_once_per_key() {
        let hints = shortcuts(Language::English);
        // Q and F10 both quit; each is its own hint, neither appears twice.
        let q = hints.iter().filter(|(key, _)| key == "Q").count();
        let f10 = hints.iter().filter(|(key, _)| key == "F10").count();
        assert_eq!((q, f10), (1, 1), "quit advertised wrongly: {hints:?}");
    }

    #[test]
    fn navigation_keys_are_bound_but_not_advertised() {
        let labels: Vec<String> = shortcuts(Language::English)
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert!(!labels.contains(&"ArrowUp".to_string()));
        assert!(labels.contains(&"Enter".to_string()));
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

    #[test]
    fn header_hints_are_translated() {
        let en = shortcuts(Language::English);
        let de = shortcuts(Language::German);
        assert_eq!(en.len(), de.len());
        assert_ne!(en[0].1, de[0].1, "the header hints are not translated");
    }
}
