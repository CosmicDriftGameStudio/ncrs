//! The keys of the Platz tab once `[keys.space]` is applied, and what a press does
//! while the tab is up. The app builds [`SpaceKeys`] from the config and hands them
//! to the tab; the key handler, the menus and the status bar read them.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_config::keymap::{ChordTable, ChordTableError, TabActions};

pub use ncrs_config::keymap::{Chord, KeyOverride};

use crate::message::Message;
use crate::shortcuts::Shortcut;

/// Why `[keys.space]` cannot be applied: the place of the line in the overrides,
/// and what is wrong with it.
pub type SpaceKeysError = ChordTableError;

/// The keys of every action of the tab, the first one of an action is the one shown.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpaceKeys {
    /// By [`Shortcut::index`].
    table: ChordTable,
}

impl Default for SpaceKeys {
    fn default() -> Self {
        Self::defaults_for(cfg!(target_os = "macos"))
    }
}

impl SpaceKeys {
    /// The keys the actions ship with on a Mac (`mac`) or elsewhere, whatever this binary runs on.
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
                    .map(|&shortcut| vec![shortcut.default_chord(command)])
                    .collect(),
            ),
        }
    }

    /// The default keys with the actions of `overrides` rebound: a named action loses
    /// its default key and gets exactly the listed ones, `[]` leaves it without a key.
    /// Two actions on one key are refused.
    pub fn with_overrides(overrides: &[KeyOverride], mac: bool) -> Result<Self, SpaceKeysError> {
        let table = Self::defaults_for(mac).table.with_overrides(
            overrides,
            &TabActions {
                table: "space",
                place_of: |name| Shortcut::from_name(name).map(Shortcut::index),
                name_of: |index| {
                    Shortcut::ALL
                        .get(index)
                        .map_or("", |shortcut| shortcut.name())
                },
            },
        )?;
        Ok(Self { table })
    }

    /// The keys of one action, the one shown first.
    pub fn chords(&self, shortcut: Shortcut) -> &[Chord] {
        self.table.chords(shortcut.index())
    }

    /// The action a press stands for. A symbol that needs Shift on some layouts (`/`)
    /// counts with or without it.
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

    /// The first key of the action as this platform spells it; empty for an action without a key.
    pub fn label(&self, shortcut: Shortcut) -> String {
        self.table.label(shortcut.index())
    }
}

/// A key of the app's own keymap that the tab answers for the row under its cursor:
/// the app resolves the press, so the tab follows what the user bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKey {
    ContextMenu,
}

/// What has the keyboard besides the list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyMode {
    pub filter_focused: bool,
    pub menu_open: bool,
    pub confirm_open: bool,
}

/// The message of a press; `app_key` is what the app's keymap makes of it. Escape is not
/// here: the app sends [`Message::Escape`] whatever field took the key. While the filter
/// has the keyboard only the keys that leave it count.
pub fn space_key(
    keys: &SpaceKeys,
    key: &Key,
    modifiers: Modifiers,
    mode: KeyMode,
    app_key: Option<AppKey>,
) -> Option<Message> {
    if mode.menu_open {
        if !modifiers.is_empty() {
            return None;
        }
        return match key.as_ref() {
            Key::Named(Named::ArrowUp) => Some(Message::MenuUp),
            Key::Named(Named::ArrowDown) => Some(Message::MenuDown),
            Key::Named(Named::Enter | Named::Space) => Some(Message::MenuEnter),
            _ => None,
        };
    }
    if mode.confirm_open {
        if !modifiers.is_empty() {
            return None;
        }
        return match key.as_ref() {
            Key::Named(Named::Tab | Named::ArrowLeft | Named::ArrowRight) => {
                Some(Message::ConfirmSwitchFocus)
            }
            Key::Named(Named::Enter) => Some(Message::ConfirmEnter),
            _ => None,
        };
    }
    if mode.filter_focused {
        return match (modifiers.is_empty(), key.as_ref()) {
            (true, Key::Named(Named::ArrowDown)) => Some(Message::Down),
            (true, Key::Named(Named::ArrowUp)) => Some(Message::Up),
            (true, Key::Named(Named::Enter)) => Some(Message::FilterSubmitted),
            _ => None,
        };
    }
    if let Some(AppKey::ContextMenu) = app_key {
        return Some(Message::OpenMenuKey);
    }
    if let Some(shortcut) = keys.pressed(key, modifiers) {
        return Some(shortcut.message());
    }
    if !modifiers.is_empty() {
        return None;
    }
    match key.as_ref() {
        Key::Named(Named::ArrowUp) => Some(Message::Up),
        Key::Named(Named::ArrowDown) => Some(Message::Down),
        Key::Named(Named::ArrowLeft) => Some(Message::Left),
        Key::Named(Named::ArrowRight) => Some(Message::Right),
        Key::Named(Named::PageUp) => Some(Message::PageUp),
        Key::Named(Named::PageDown) => Some(Message::PageDown),
        Key::Named(Named::Home) => Some(Message::First),
        Key::Named(Named::End) => Some(Message::Last),
        Key::Named(Named::Space) => Some(Message::Toggle),
        Key::Named(Named::Enter) => Some(Message::Enter),
        _ => None,
    }
}
