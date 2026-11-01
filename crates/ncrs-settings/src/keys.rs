//! What a key does while one of the settings overlays is up. The app asks the
//! keymap what the key is bound to and passes in what the overlay lets
//! through.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_config::keymap::recorded_binding;

use crate::key_bindings::VISIBLE_ROWS;
use crate::message::{KeyBindingsMessage, ThemeFetchMessage};

#[derive(Debug, Clone, PartialEq)]
pub enum ThemeFetchKey {
    Message(ThemeFetchMessage),
    Quit,
}

/// Keys while the theme download card is up. `quits` is asked for a key the
/// card does not know: whether it is bound to quitting and is no key a text
/// field types.
pub fn theme_fetch_key(
    key: &Key,
    modifiers: Modifiers,
    quits: impl FnOnce() -> bool,
) -> Option<ThemeFetchKey> {
    let message = |message| Some(ThemeFetchKey::Message(message));
    match key.as_ref() {
        Key::Named(Named::Escape) => message(ThemeFetchMessage::Close),
        Key::Named(Named::Enter) => message(ThemeFetchMessage::Activate),
        Key::Named(Named::ArrowUp) if modifiers.is_empty() => message(ThemeFetchMessage::Move(-1)),
        Key::Named(Named::ArrowDown) if modifiers.is_empty() => message(ThemeFetchMessage::Move(1)),
        _ => quits().then_some(ThemeFetchKey::Quit),
    }
}

/// The keyboard shortcuts dialog. While it records, every key but Escape is
/// the key recorded, also one bound to something else. Otherwise the search
/// field takes the typing and Enter (and `InputState` takes Escape); a key it
/// does not type searches for what is bound to it, and the list takes the
/// arrows, Delete and Backspace.
pub fn key_bindings_key(
    recording: bool,
    key: &Key,
    modifiers: Modifiers,
) -> Option<KeyBindingsMessage> {
    use KeyBindingsMessage as Shortcuts;
    let is_escape = matches!(key.as_ref(), Key::Named(Named::Escape));
    if recording {
        if is_escape {
            return Some(Shortcuts::CancelRecording);
        }
        return Some(Shortcuts::Recorded(recorded_binding(key, modifiers)?));
    }
    let plain = modifiers.is_empty();
    let page = isize::try_from(VISIBLE_ROWS).unwrap_or(1);
    match key.as_ref() {
        Key::Named(Named::Escape) => None,
        Key::Named(Named::Enter) if plain => Some(Shortcuts::Record),
        Key::Named(Named::ArrowUp) if plain => Some(Shortcuts::Move(-1)),
        Key::Named(Named::ArrowDown) if plain => Some(Shortcuts::Move(1)),
        Key::Named(Named::PageUp) if plain => Some(Shortcuts::Move(-page)),
        Key::Named(Named::PageDown) if plain => Some(Shortcuts::Move(page)),
        Key::Named(Named::ArrowLeft) if plain => Some(Shortcuts::MoveKey(-1)),
        Key::Named(Named::ArrowRight) if plain => Some(Shortcuts::MoveKey(1)),
        Key::Named(Named::Delete | Named::Backspace) if plain => Some(Shortcuts::RemoveKey),
        Key::Named(Named::Delete | Named::Backspace) if modifiers == Modifiers::SHIFT => {
            Some(Shortcuts::Reset)
        }
        Key::Character(typed) if (modifiers - Modifiers::SHIFT).is_empty() => {
            Some(Shortcuts::Typed(typed.to_string()))
        }
        _ => Some(Shortcuts::SearchByKey(recorded_binding(key, modifiers)?)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsKey {
    /// Escape: closes the tab, or the filing setup that stands in its place.
    Close,
    /// The press is bound to one of the few commands that reach through.
    Bound,
}

/// The tab shows no panel, so the panel keys do nothing. Of the bindings the
/// command list, the key that closes the tab and quitting reach through.
/// `passes` says whether the press is bound to one of them and is no key a
/// text field types.
pub fn settings_key(key: &Key, passes: impl FnOnce() -> bool) -> Option<SettingsKey> {
    if matches!(key.as_ref(), Key::Named(Named::Escape)) {
        return Some(SettingsKey::Close);
    }
    passes().then_some(SettingsKey::Bound)
}
