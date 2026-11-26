//! The keys of the Git actions once `[keys.git]` is applied. The app builds
//! them from the config and hands them to the tab; the key handler, the menus
//! and the tooltips read them.

use iced::keyboard::{Key, Modifiers};
use ncrs_config::keymap::{ChordTable, ChordTableError, TabActions};

pub use ncrs_config::keymap::chord_label;
pub use ncrs_config::keymap::{Chord, KeyOverride};

use crate::shortcuts::Shortcut;

/// Why `[keys.git]` cannot be applied: the place of the line in the
/// overrides, and what is wrong with it.
pub type GitKeysError = ChordTableError;

/// The keys of every Git action, the first one of an action is the one shown.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GitKeys {
    /// By [`Shortcut::index`].
    table: ChordTable,
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
            table: ChordTable::new(
                Shortcut::ALL
                    .iter()
                    .map(|&shortcut| shortcut.default_chord(command).into_iter().collect())
                    .collect(),
            ),
        }
    }

    /// The default keys with the actions of `overrides` rebound: a named
    /// action loses its default key and gets exactly the listed ones, `[]`
    /// leaves it without a key. Two actions on one key are refused.
    pub fn with_overrides(overrides: &[KeyOverride], mac: bool) -> Result<Self, GitKeysError> {
        let place_of = |name: &str| Shortcut::from_name(name).map(Shortcut::index);
        let name_of = |index: usize| Shortcut::ALL.get(index).map_or("", |s| s.name());
        let table = Self::defaults_for(mac).table.with_overrides(
            overrides,
            &TabActions {
                table: "git",
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

    /// The action a press stands for.
    pub fn pressed(&self, key: &Key, modifiers: Modifiers) -> Option<Shortcut> {
        let index = self.table.pressed(key, modifiers)?;
        Shortcut::ALL.get(index).copied()
    }

    /// The first key of the action as this platform spells it: `⇧⌘E` on a
    /// Mac, `Ctrl+Shift+E` elsewhere; empty for an action without a key.
    pub fn label(&self, shortcut: Shortcut) -> String {
        self.table.label(shortcut.index())
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
            let keys = GitKeys::defaults_for(mac);
            for (index, left) in Shortcut::ALL.iter().enumerate() {
                if keys.chords(*left).is_empty() {
                    continue;
                }
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
        let (default_held, default_key) = Shortcut::Fetch.default_chord(Modifiers::CTRL).unwrap();
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
        let (held, key) = Shortcut::Pull.default_chord(Modifiers::CTRL).unwrap();
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
        let (fetch_held, fetch_key) = Shortcut::Fetch.default_chord(Modifiers::CTRL).unwrap();
        let (pull_held, pull_key) = Shortcut::Pull.default_chord(Modifiers::CTRL).unwrap();
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
