//! The keys of the Git actions once `[keys.git]` is applied. The app builds
//! them from the config and hands them to the tab; the key handler, the menus
//! and the tooltips read them.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::shortcuts::Shortcut;

/// The modifiers held and the key pressed.
pub type Chord = (Modifiers, Key);

/// One line of `[keys.git]`: the name of an action and the keys written for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyOverride {
    pub action: String,
    pub chords: Vec<Chord>,
}

/// Why `[keys.git]` cannot be applied: the place of the line in the
/// overrides, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitKeysError {
    pub place: usize,
    pub reason: String,
}

/// The keys of every Git action, the first one of an action is the one shown.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GitKeys {
    /// By [`Shortcut::index`].
    chords: Vec<Vec<Chord>>,
}

impl Default for GitKeys {
    fn default() -> Self {
        Self::defaults_for(cfg!(target_os = "macos"))
    }
}

impl GitKeys {
    /// The keys the actions ship with on a Mac (`mac`) or elsewhere, whatever
    /// this binary runs on.
    pub fn defaults_for(mac: bool) -> Self {
        let command = if mac {
            Modifiers::LOGO
        } else {
            Modifiers::CTRL
        };
        Self {
            chords: Shortcut::ALL
                .iter()
                .map(|shortcut| vec![shortcut.default_chord(command)])
                .collect(),
        }
    }

    /// The default keys with the actions of `overrides` rebound: a named
    /// action loses its default key and gets exactly the listed ones, `[]`
    /// leaves it without a key. Two actions on one key are refused.
    pub fn with_overrides(overrides: &[KeyOverride], mac: bool) -> Result<Self, GitKeysError> {
        let mut keys = Self::defaults_for(mac);
        let mut rebound: Vec<Shortcut> = Vec::new();
        for (place, entry) in overrides.iter().enumerate() {
            let shortcut = Shortcut::from_name(&entry.action).ok_or_else(|| GitKeysError {
                place,
                reason: format!("unknown action {:?}", entry.action),
            })?;
            if let Some(slot) = keys.chords.get_mut(shortcut.index()) {
                slot.clone_from(&entry.chords);
            }
            rebound.push(shortcut);
        }
        for (place, entry) in overrides.iter().enumerate() {
            let Some(shortcut) = Shortcut::from_name(&entry.action) else {
                continue;
            };
            for chord in &entry.chords {
                let holder = Shortcut::ALL
                    .iter()
                    .copied()
                    .find(|other| *other != shortcut && keys.holds(*other, chord));
                if let Some(other) = holder {
                    return Err(GitKeysError {
                        place,
                        reason: format!("already bound to {} in [keys.git]", other.name()),
                    });
                }
            }
        }
        Ok(keys)
    }

    fn holds(&self, shortcut: Shortcut, chord: &Chord) -> bool {
        self.chords(shortcut)
            .iter()
            .any(|(held, key)| *held == chord.0 && same_key(key, &chord.1))
    }

    /// The keys of one action, the one shown first.
    pub fn chords(&self, shortcut: Shortcut) -> &[Chord] {
        self.chords.get(shortcut.index()).map_or(&[], Vec::as_slice)
    }

    /// The action a press stands for.
    pub fn pressed(&self, key: &Key, modifiers: Modifiers) -> Option<Shortcut> {
        Shortcut::ALL
            .iter()
            .copied()
            .find(|shortcut| self.holds(*shortcut, &(modifiers, key.clone())))
    }

    /// The first key of the action as this platform spells it: `⇧⌘E` on a
    /// Mac, `Ctrl+Shift+E` elsewhere; empty for an action without a key.
    pub fn label(&self, shortcut: Shortcut) -> String {
        self.chords(shortcut)
            .first()
            .map(|(held, key)| chord_label(*held, key))
            .unwrap_or_default()
    }
}

/// Letters ignore case: a press with Shift arrives as the capital.
fn same_key(left: &Key, right: &Key) -> bool {
    match (left, right) {
        (Key::Character(a), Key::Character(b)) => a.eq_ignore_ascii_case(b),
        _ => left == right,
    }
}

fn chord_label(held: Modifiers, key: &Key) -> String {
    let key_name = match key {
        Key::Character(character) => character.to_uppercase(),
        Key::Named(Named::Enter) => "\u{21b5}".to_owned(),
        Key::Named(Named::Backspace) => "\u{232b}".to_owned(),
        Key::Named(Named::ArrowLeft) => "\u{2190}".to_owned(),
        Key::Named(Named::ArrowRight) => "\u{2192}".to_owned(),
        Key::Named(Named::ArrowUp) => "\u{2191}".to_owned(),
        Key::Named(Named::ArrowDown) => "\u{2193}".to_owned(),
        Key::Named(named) => format!("{named:?}"),
        Key::Unidentified => String::new(),
    };
    let mut label = String::new();
    if cfg!(target_os = "macos") {
        for (is_held, symbol) in [
            (held.control(), '\u{2303}'),
            (held.alt(), '\u{2325}'),
            (held.shift(), '\u{21e7}'),
            (held.logo(), '\u{2318}'),
        ] {
            if is_held {
                label.push(symbol);
            }
        }
    } else {
        for (is_held, name) in [
            (held.control(), "Ctrl+"),
            (held.alt(), "Alt+"),
            (held.shift(), "Shift+"),
            (held.logo(), "Super+"),
        ] {
            if is_held {
                label.push_str(name);
            }
        }
    }
    label.push_str(&key_name);
    label
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
            let keys = GitKeys::defaults_for(mac);
            for (index, left) in Shortcut::ALL.iter().enumerate() {
                for right in &Shortcut::ALL[index + 1..] {
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
            let keys = GitKeys::defaults_for(mac);
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
    fn an_override_replaces_the_default_key() {
        let held = Modifiers::CTRL | Modifiers::ALT;
        let keys = GitKeys::with_overrides(
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
    fn an_empty_list_leaves_the_action_without_a_key() {
        let keys = GitKeys::with_overrides(
            &[KeyOverride {
                action: "fetch".to_owned(),
                chords: Vec::new(),
            }],
            false,
        )
        .unwrap();
        assert_eq!(Shortcut::Fetch.label(&keys), "");
        assert_eq!(Shortcut::Fetch.binding(&keys), None);
    }

    #[test]
    fn an_override_on_the_key_of_another_action_is_refused() {
        let (held, key) = Shortcut::Pull.default_chord(Modifiers::CTRL);
        let refused = GitKeys::with_overrides(
            &[KeyOverride {
                action: "fetch".to_owned(),
                chords: vec![(held, key)],
            }],
            false,
        )
        .unwrap_err();
        assert_eq!(refused.place, 0);
        assert!(refused.reason.contains("pull"), "{}", refused.reason);
    }

    #[test]
    fn two_overrides_on_one_key_are_refused() {
        let same = chord(Modifiers::CTRL | Modifiers::ALT, "q");
        let refused = GitKeys::with_overrides(
            &[
                KeyOverride {
                    action: "fetch".to_owned(),
                    chords: vec![same.clone()],
                },
                KeyOverride {
                    action: "pull".to_owned(),
                    chords: vec![same],
                },
            ],
            false,
        )
        .unwrap_err();
        assert_eq!(refused.place, 0);
    }

    #[test]
    fn a_swap_of_two_actions_is_allowed() {
        let (fetch_held, fetch_key) = Shortcut::Fetch.default_chord(Modifiers::CTRL);
        let (pull_held, pull_key) = Shortcut::Pull.default_chord(Modifiers::CTRL);
        let keys = GitKeys::with_overrides(
            &[
                KeyOverride {
                    action: "fetch".to_owned(),
                    chords: vec![(pull_held, pull_key)],
                },
                KeyOverride {
                    action: "pull".to_owned(),
                    chords: vec![(fetch_held, fetch_key)],
                },
            ],
            false,
        );
        keys.unwrap();
    }

    #[test]
    fn an_unknown_action_is_refused() {
        let refused = GitKeys::with_overrides(
            &[KeyOverride {
                action: "teleport".to_owned(),
                chords: Vec::new(),
            }],
            false,
        )
        .unwrap_err();
        assert!(refused.reason.contains("teleport"));
    }
}
