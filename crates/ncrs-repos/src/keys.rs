//! The keys of the Repos tab once `[keys.repos]` is applied, and what a press
//! does while the tab is up. The app builds [`ReposKeys`] from the config and
//! hands them to the tab; the key handler, the menus and the tooltips read them.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_config::keymap::{ChordTable, ChordTableError, TabActions};

pub use ncrs_config::keymap::{Chord, KeyOverride};

use crate::message::Message;
use crate::shortcuts::Shortcut;

/// Why `[keys.repos]` cannot be applied: the place of the line in the
/// overrides, and what is wrong with it.
pub type ReposKeysError = ChordTableError;

/// The keys of every action of the tab, the first one of an action is the one shown.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReposKeys {
    /// By [`Shortcut::index`].
    table: ChordTable,
}

impl Default for ReposKeys {
    fn default() -> Self {
        Self::defaults_for(cfg!(target_os = "macos"))
    }
}

impl ReposKeys {
    /// The keys the actions ship with on a Mac (`mac`) or elsewhere, whatever
    /// this binary runs on.
    pub fn defaults_for(mac: bool) -> Self {
        let command = if mac {
            Modifiers::LOGO
        } else {
            Modifiers::CTRL
        };
        Self {
            table: ChordTable::new(
                Shortcut::ALL
                    .iter()
                    .map(|shortcut| vec![shortcut.default_chord(command)])
                    .collect(),
            ),
        }
    }

    /// The default keys with the actions of `overrides` rebound: a named
    /// action loses its default key and gets exactly the listed ones, `[]`
    /// leaves it without a key. Two actions on one key are refused.
    pub fn with_overrides(overrides: &[KeyOverride], mac: bool) -> Result<Self, ReposKeysError> {
        let place_of = |name: &str| Shortcut::from_name(name).map(Shortcut::index);
        let name_of = |index: usize| Shortcut::ALL.get(index).map_or("", |s| s.name());
        let table = Self::defaults_for(mac).table.with_overrides(
            overrides,
            &TabActions {
                table: "repos",
                place_of,
                name_of,
            },
        )?;
        Ok(Self { table })
    }

    /// The keys of one action, the one shown first.
    pub fn chords(&self, shortcut: Shortcut) -> &[Chord] {
        self.table.chords(shortcut.index())
    }

    /// The action a press stands for. A symbol that needs Shift on some
    /// layouts (`*`) counts with or without it.
    pub fn pressed(&self, key: &Key, modifiers: Modifiers) -> Option<Shortcut> {
        let find = |held: Modifiers| {
            let index = self.table.pressed(key, held)?;
            Shortcut::ALL.get(index).copied()
        };
        find(modifiers).or_else(|| {
            let is_symbol =
                matches!(key, Key::Character(text) if text.chars().all(|c| !c.is_alphanumeric()));
            (is_symbol && modifiers == Modifiers::SHIFT)
                .then(|| find(Modifiers::empty()))
                .flatten()
        })
    }

    /// The first key of the action as this platform spells it: `⇧⌘E` on a
    /// Mac, `Ctrl+Shift+E` elsewhere; empty for an action without a key.
    pub fn label(&self, shortcut: Shortcut) -> String {
        self.table.label(shortcut.index())
    }
}

/// The message of a press. Escape is not here: the app sends
/// [`Message::Escape`] whatever field took the key. While the filter has the
/// keyboard only the keys that leave it count.
pub fn repos_key(
    keys: &ReposKeys,
    key: &Key,
    modifiers: Modifiers,
    filter_focused: bool,
    menu_open: bool,
) -> Option<Message> {
    if menu_open {
        return None;
    }
    if filter_focused {
        return match (modifiers.is_empty(), key.as_ref()) {
            (true, Key::Named(Named::ArrowDown)) => Some(Message::Down),
            (true, Key::Named(Named::ArrowUp)) => Some(Message::Up),
            _ => None,
        };
    }
    if let Some(shortcut) = keys.pressed(key, modifiers) {
        return Some(shortcut.message());
    }
    // "/" is a shifted key on some layouts.
    let slash_modifiers = modifiers.is_empty() || modifiers == Modifiers::SHIFT;
    if slash_modifiers && key.as_ref() == Key::Character("/") {
        return Some(Message::FocusFilter);
    }
    if modifiers == Modifiers::SHIFT {
        return match key.as_ref() {
            Key::Named(Named::ArrowUp) => Some(Message::ExtendUp),
            Key::Named(Named::ArrowDown) => Some(Message::ExtendDown),
            Key::Named(Named::Tab) => Some(Message::SwitchFocus),
            _ => None,
        };
    }
    if !modifiers.is_empty() {
        return None;
    }
    match key.as_ref() {
        Key::Named(Named::ArrowUp) => Some(Message::Up),
        Key::Named(Named::ArrowDown) => Some(Message::Down),
        Key::Named(Named::ArrowLeft) => Some(Message::Left),
        Key::Named(Named::Tab) => Some(Message::SwitchFocus),
        Key::Named(Named::ArrowRight) => Some(Message::Right),
        Key::Named(Named::PageUp) => Some(Message::PageUp),
        Key::Named(Named::PageDown) => Some(Message::PageDown),
        Key::Named(Named::Home) => Some(Message::First),
        Key::Named(Named::End) => Some(Message::Last),
        Key::Named(Named::Space) => Some(Message::Mark),
        Key::Named(Named::Enter) => Some(Message::Enter),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(held: Modifiers, letter: &str) -> Chord {
        (held, Key::Character(letter.into()))
    }

    #[test]
    fn every_default_key_is_its_own_on_both_platforms() {
        for mac in [true, false] {
            let keys = ReposKeys::defaults_for(mac);
            for (index, left) in Shortcut::ALL.iter().enumerate() {
                for right in Shortcut::ALL.iter().skip(index + 1) {
                    assert_ne!(
                        keys.chords(*left),
                        keys.chords(*right),
                        "{left:?} and {right:?}, mac: {mac}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_key_finds_its_shortcut() {
        for mac in [true, false] {
            let keys = ReposKeys::defaults_for(mac);
            for shortcut in Shortcut::ALL {
                for (held, key) in keys.chords(*shortcut) {
                    assert_eq!(keys.pressed(key, *held), Some(*shortcut), "mac: {mac}");
                }
            }
        }
    }

    #[test]
    fn the_names_find_their_shortcut() {
        for shortcut in Shortcut::ALL {
            assert_eq!(Shortcut::from_name(shortcut.name()), Some(*shortcut));
        }
    }

    #[test]
    fn the_star_key_works_with_and_without_shift() {
        let keys = ReposKeys::defaults_for(false);
        let star = Key::Character("*".into());
        assert_eq!(
            keys.pressed(&star, Modifiers::empty()),
            Some(Shortcut::Favourite)
        );
        assert_eq!(
            keys.pressed(&star, Modifiers::SHIFT),
            Some(Shortcut::Favourite)
        );
        assert_eq!(keys.pressed(&star, Modifiers::CTRL), None);
    }

    #[test]
    fn an_override_replaces_the_default_key() {
        let held = Modifiers::CTRL | Modifiers::ALT;
        let keys = ReposKeys::with_overrides(
            &[KeyOverride {
                action: "fetch".to_owned(),
                chords: vec![chord(held, "f")],
            }],
            false,
        )
        .unwrap();
        assert_eq!(
            keys.pressed(&Key::Character("f".into()), held),
            Some(Shortcut::Fetch)
        );
        let (default_held, default_key) = Shortcut::Fetch.default_chord(Modifiers::CTRL);
        assert_eq!(keys.pressed(&default_key, default_held), None);
        let expected = if cfg!(target_os = "macos") {
            "\u{2303}\u{2325}F"
        } else {
            "Ctrl+Alt+F"
        };
        assert_eq!(Shortcut::Fetch.label(&keys), expected);
    }

    #[test]
    fn an_override_on_the_key_of_another_action_is_refused() {
        let chord = Shortcut::PullFastForward.default_chord(Modifiers::CTRL);
        let refused = ReposKeys::with_overrides(
            &[KeyOverride {
                action: "fetch".to_owned(),
                chords: vec![chord],
            }],
            false,
        )
        .unwrap_err();
        assert_eq!(refused.place, 0);
        assert!(refused.reason.contains("pull"), "{}", refused.reason);
    }

    #[test]
    fn an_unknown_action_is_refused() {
        let refused = ReposKeys::with_overrides(
            &[KeyOverride {
                action: "teleport".to_owned(),
                chords: Vec::new(),
            }],
            false,
        )
        .unwrap_err();
        assert!(refused.reason.contains("teleport"));
    }

    #[test]
    fn the_arrows_move_and_shift_extends() {
        let keys = ReposKeys::defaults_for(false);
        let down = Key::Named(Named::ArrowDown);
        assert!(matches!(
            repos_key(&keys, &down, Modifiers::empty(), false, false),
            Some(Message::Down)
        ));
        assert!(matches!(
            repos_key(&keys, &down, Modifiers::SHIFT, false, false),
            Some(Message::ExtendDown)
        ));
        assert!(repos_key(&keys, &down, Modifiers::empty(), false, true).is_none());
    }

    #[test]
    fn the_filter_takes_the_letters() {
        let keys = ReposKeys::defaults_for(false);
        let space = Key::Named(Named::Space);
        assert!(repos_key(&keys, &space, Modifiers::empty(), true, false).is_none());
        assert!(matches!(
            repos_key(&keys, &space, Modifiers::empty(), false, false),
            Some(Message::Mark)
        ));
    }
}
