//! The keys of the actions of one tab (`[keys.git]`, `[keys.repos]`): which
//! chords an action holds once the overrides of the file are applied. The tab
//! names its actions and ships the default chords; the rules are the same.

use super::*;

use iced_core::keyboard::key::Named;

/// The modifiers held and the key pressed.
pub type Chord = (Modifiers, Key);

/// Opens the context menu on a Mac, which has no menu key and sends F10 only
/// with fn held.
pub fn mac_menu_chord() -> Chord {
    (Modifiers::CTRL, Key::Named(Named::Enter))
}

/// One line of `[keys.<tab>]`: the name of an action and the keys written for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyOverride {
    pub action: String,
    pub chords: Vec<Chord>,
}

/// Why `[keys.<tab>]` cannot be applied: the place of the line in the
/// overrides, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordTableError {
    pub place: usize,
    pub reason: String,
}

/// What a tab says about its actions, by their place in its list.
#[derive(Debug, Clone, Copy)]
pub struct TabActions<'a> {
    /// `[keys.<tab>]`, for the messages.
    pub table: &'a str,
    pub place_of: fn(&str) -> Option<usize>,
    pub name_of: fn(usize) -> &'static str,
}

/// The chords of every action of a tab, the first one of an action is the one
/// shown.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChordTable {
    chords: Vec<Vec<Chord>>,
}

impl ChordTable {
    /// One list of chords per action, in the order of the tab's actions.
    pub fn new(chords: Vec<Vec<Chord>>) -> Self {
        Self { chords }
    }

    /// These chords with the actions of `overrides` rebound: a named action
    /// loses its default key and gets exactly the listed ones, `[]` leaves it
    /// without a key. Two actions on one key are refused.
    pub fn with_overrides(
        mut self,
        overrides: &[KeyOverride],
        actions: &TabActions<'_>,
    ) -> Result<Self, ChordTableError> {
        let mut rebound: Vec<usize> = Vec::new();
        for (place, entry) in overrides.iter().enumerate() {
            let index = (actions.place_of)(&entry.action).ok_or_else(|| ChordTableError {
                place,
                reason: format!("unknown action {:?}", entry.action),
            })?;
            if let Some(slot) = self.chords.get_mut(index) {
                slot.clone_from(&entry.chords);
            }
            rebound.push(index);
        }
        for (place, (entry, index)) in overrides.iter().zip(rebound).enumerate() {
            for chord in &entry.chords {
                let holder = (0..self.chords.len())
                    .find(|other| *other != index && self.holds(*other, chord));
                if let Some(other) = holder {
                    return Err(ChordTableError {
                        place,
                        reason: format!(
                            "already bound to {} in [keys.{}]",
                            (actions.name_of)(other),
                            actions.table
                        ),
                    });
                }
            }
        }
        Ok(self)
    }

    fn holds(&self, index: usize, chord: &Chord) -> bool {
        self.chords(index)
            .iter()
            .any(|(held, key)| *held == chord.0 && same_key(key, &chord.1))
    }

    /// The keys of one action, the one shown first.
    pub fn chords(&self, index: usize) -> &[Chord] {
        self.chords.get(index).map_or(&[], Vec::as_slice)
    }

    /// The first action that holds the press.
    pub fn pressed(&self, key: &Key, modifiers: Modifiers) -> Option<usize> {
        let chord = (modifiers, key.clone());
        (0..self.chords.len()).find(|index| self.holds(*index, &chord))
    }

    /// The first key of the action as this platform spells it: `⇧⌘E` on a
    /// Mac, `Ctrl+Shift+E` elsewhere; empty for an action without a key.
    pub fn label(&self, index: usize) -> String {
        self.chords(index)
            .first()
            .map(|(held, key)| chord_label(*held, key))
            .unwrap_or_default()
    }
}

/// Letters ignore case: a press with Shift arrives as the capital.
pub fn same_key(left: &Key, right: &Key) -> bool {
    match (left, right) {
        (Key::Character(a), Key::Character(b)) => a.eq_ignore_ascii_case(b),
        _ => left == right,
    }
}

/// A chord as this platform spells it.
pub fn chord_label(held: Modifiers, key: &Key) -> String {
    let key_name = match key {
        Key::Character(character) => character.to_uppercase(),
        Key::Named(Named::Enter) => "\u{21b5}".to_owned(),
        Key::Named(Named::Backspace) => "\u{232b}".to_owned(),
        Key::Named(Named::Delete) => "Del".to_owned(),
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
