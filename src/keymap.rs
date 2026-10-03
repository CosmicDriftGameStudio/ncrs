//! Keyboard bindings and the header hints, from one source.
//!
//! A `KeyAction` names what a key can do; the `Keymap` says which keys do it.
//! The key handler and the header hint list are both derived from the keymap,
//! so a binding cannot exist in one and be missing in the other. The built-in
//! keymap is the default; `[keys]` in the config file replaces the keys of the
//! actions it names.
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use iced::keyboard::{key, Key, Modifiers};

use crate::config::KeySection;
use crate::fs::{OpenKind, SortColumn};
use crate::i18n::{Language, Msg};
use crate::messages::Message;
use crate::messages::{ClipboardKind, PanelSide, TransferKind};

/// A key plus the modifier that must be held. A bare press has no modifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    pub fn with_modifiers(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }
}

/// Defines `KeyAction` and its config names from one list, so a name cannot be
/// spelt differently in the enum and in the parser.
macro_rules! key_actions {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        /// Something a key can do. The name is what `[keys]` calls it.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum KeyAction {
            $($variant),+
        }

        impl KeyAction {
            #[cfg(test)]
            pub const ALL: &'static [KeyAction] = &[$(Self::$variant),+];

            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),+
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

key_actions! {
    Quit => "quit",
    GoUp => "go_up",
    Open => "open",
    SwitchPanel => "switch_panel",
    SwitchLanguage => "switch_language",
    ToggleTag => "toggle_tag",
    TagDown => "tag_down",
    TagUp => "tag_up",
    TagAll => "tag_all",
    ClearTags => "clear_tags",
    AbortJob => "abort_job",
    CopyPath => "copy_path",
    CopyName => "copy_name",
    View => "view",
    Edit => "edit",
    Copy => "copy",
    Move => "move",
    Delete => "delete",
    DeletePermanently => "delete_permanently",
    Mkdir => "mkdir",
    DriveMenuLeft => "drive_menu_left",
    DriveMenuRight => "drive_menu_right",
    SortByName => "sort_by_name",
    SortByModified => "sort_by_modified",
    SortBySize => "sort_by_size",
    ContextMenu => "context_menu",
    CursorUp => "cursor_up",
    CursorDown => "cursor_down",
    PageUp => "page_up",
    PageDown => "page_down",
    First => "first",
    Last => "last",
}

impl KeyAction {
    /// The message the key sends. Derived rather than stored next to the key,
    /// so a rebound key cannot send anything but its action.
    pub fn message(self) -> Message {
        match self {
            Self::Quit => Message::Quit,
            Self::GoUp => Message::GoUp,
            Self::Open => Message::OpenSelected,
            Self::SwitchPanel => Message::SwitchPanel,
            Self::SwitchLanguage => Message::SwitchLanguage,
            Self::ToggleTag => Message::ToggleTag,
            Self::TagDown => Message::TagMove(1),
            Self::TagUp => Message::TagMove(-1),
            Self::TagAll => Message::TagAll,
            Self::ClearTags => Message::ClearTags,
            Self::AbortJob => Message::AbortJob,
            Self::CopyPath => Message::CopyToClipboard(ClipboardKind::Path),
            Self::CopyName => Message::CopyToClipboard(ClipboardKind::Name),
            Self::View => Message::OpenExternal(OpenKind::View),
            Self::Edit => Message::OpenExternal(OpenKind::Edit),
            Self::Copy => Message::Transfer(TransferKind::Copy),
            Self::Move => Message::Transfer(TransferKind::Move),
            Self::Delete => Message::Delete { permanent: false },
            Self::DeletePermanently => Message::Delete { permanent: true },
            Self::Mkdir => Message::CreateDirPrompt,
            Self::DriveMenuLeft => Message::VolumeMenu(PanelSide::Left),
            Self::DriveMenuRight => Message::VolumeMenu(PanelSide::Right),
            Self::SortByName => Message::SortActive(SortColumn::Name),
            Self::SortByModified => Message::SortActive(SortColumn::Modified),
            Self::SortBySize => Message::SortActive(SortColumn::Size),
            Self::ContextMenu => Message::ContextMenuKey,
            Self::CursorUp => Message::MoveSelection(-1),
            Self::CursorDown => Message::MoveSelection(1),
            Self::PageUp => Message::PageUp,
            Self::PageDown => Message::PageDown,
            Self::First => Message::SelectFirst,
            Self::Last => Message::SelectLast,
        }
    }

    /// The translated hint the header shows. `None` for actions that are bound
    /// but not advertised, like the arrow keys: the header has room for five
    /// or six hints, not twenty.
    pub fn hint(self) -> Option<Msg> {
        match self {
            Self::Quit => Some(Msg::ShortcutQuit),
            Self::SwitchLanguage => Some(Msg::ShortcutSwitchLanguage),
            Self::View => Some(Msg::ShortcutView),
            Self::Edit => Some(Msg::ShortcutEdit),
            Self::Copy => Some(Msg::ShortcutCopy),
            Self::Move => Some(Msg::ShortcutMove),
            Self::Delete => Some(Msg::ShortcutDelete),
            Self::Mkdir => Some(Msg::ShortcutMkdir),
            _ => None,
        }
    }
}

/// The keys this binary ships with, in the order that decides which one a
/// label shows. Adding a file operation means adding a `KeyAction` and its
/// keys here; the key handler and the hint list follow from it.
///
/// Built at runtime rather than as a `static` because `Key::Character` needs a
/// `SmolStr`, which is not a const constructor.
fn default_bindings() -> Vec<(KeyAction, Binding)> {
    use key::Named::*;
    use Key::{Character, Named as KeyNamed};
    use KeyAction as A;

    vec![
        // --- Backspace, Enter, Tab and Cmd+Q are not on the function key bar ---
        // Not a bare Q: that letter belongs to type-ahead.
        (
            A::Quit,
            Binding::with_modifiers(Character("q".into()), Modifiers::COMMAND),
        ),
        (A::GoUp, Binding::key(KeyNamed(Backspace))),
        (A::Open, Binding::key(KeyNamed(Enter))),
        (A::SwitchPanel, Binding::key(KeyNamed(Tab))),
        (A::SwitchLanguage, Binding::key(KeyNamed(F9))),
        // --- selection: tagged rows, not advertised ---
        (A::ToggleTag, Binding::key(KeyNamed(Insert))),
        (A::ToggleTag, Binding::key(KeyNamed(Space))),
        (
            A::TagDown,
            Binding::with_modifiers(KeyNamed(ArrowDown), Modifiers::SHIFT),
        ),
        (
            A::TagUp,
            Binding::with_modifiers(KeyNamed(ArrowUp), Modifiers::SHIFT),
        ),
        (A::TagAll, Binding::key(Character("*".into()))),
        (
            A::ClearTags,
            Binding::with_modifiers(Character("*".into()), Modifiers::CTRL),
        ),
        // Ctrl+C stops a running job.
        (
            A::AbortJob,
            Binding::with_modifiers(Character("c".into()), Modifiers::CTRL),
        ),
        // Option+Cmd+C and Ctrl+Cmd+C on a Mac; Ctrl+Alt+C and Ctrl+Shift+C
        // elsewhere. Plain Ctrl+C stays "stop the job".
        (
            A::CopyPath,
            Binding::with_modifiers(Character("c".into()), Modifiers::ALT | Modifiers::COMMAND),
        ),
        (
            A::CopyName,
            Binding::with_modifiers(
                Character("c".into()),
                if cfg!(target_os = "macos") {
                    Modifiers::CTRL | Modifiers::COMMAND
                } else {
                    Modifiers::CTRL | Modifiers::SHIFT
                },
            ),
        ),
        (A::View, Binding::key(KeyNamed(F3))),
        (A::Edit, Binding::key(KeyNamed(F4))),
        (A::Copy, Binding::key(KeyNamed(F5))),
        (A::Move, Binding::key(KeyNamed(F6))),
        (A::Delete, Binding::key(KeyNamed(F8))),
        // Not advertised: the header has no room, and a key that deletes for
        // good should be found in the help, not stumbled over.
        (
            A::DeletePermanently,
            Binding::with_modifiers(KeyNamed(F8), Modifiers::SHIFT),
        ),
        (A::Mkdir, Binding::key(KeyNamed(F7))),
        // Alt (Option on macOS) with F1 or F2 picks the drive of the left or
        // right panel, as in Norton Commander. Not advertised: the bar shows
        // bare keys only, and NC does not list these either.
        (
            A::DriveMenuLeft,
            Binding::with_modifiers(KeyNamed(F1), Modifiers::ALT),
        ),
        (
            A::DriveMenuRight,
            Binding::with_modifiers(KeyNamed(F2), Modifiers::ALT),
        ),
        (A::SortByName, sort_binding(SortColumn::Name)),
        (A::SortByModified, sort_binding(SortColumn::Modified)),
        (A::SortBySize, sort_binding(SortColumn::Size)),
        // Shift+F10 is the context menu key on Windows and Linux. Registered so
        // the exact chord wins over the fall-back to bare F10, which quits.
        (
            A::ContextMenu,
            Binding::with_modifiers(KeyNamed(F10), Modifiers::SHIFT),
        ),
        // Quit is also F10, the Norton Commander convention.
        (A::Quit, Binding::key(KeyNamed(F10))),
        // --- bound, not advertised: the header has no room for these ---
        (A::CursorUp, Binding::key(KeyNamed(ArrowUp))),
        (A::CursorDown, Binding::key(KeyNamed(ArrowDown))),
        (A::PageUp, Binding::key(KeyNamed(PageUp))),
        (A::PageDown, Binding::key(KeyNamed(PageDown))),
        // Cmd+Up/Down page on a Mac, where there is no PageUp key; `COMMAND`
        // is Ctrl elsewhere.
        (
            A::PageUp,
            Binding::with_modifiers(KeyNamed(ArrowUp), Modifiers::COMMAND),
        ),
        (
            A::PageDown,
            Binding::with_modifiers(KeyNamed(ArrowDown), Modifiers::COMMAND),
        ),
        (A::First, Binding::key(KeyNamed(Home))),
        (A::Last, Binding::key(KeyNamed(End))),
    ]
}

/// Ctrl+F3, Ctrl+F5 and Ctrl+F6 as in Norton Commander. A Mac takes Ctrl+F-keys
/// for itself, so there it is Cmd+1 (name), Cmd+2 (size) and Cmd+3 (modified).
pub fn sort_binding(column: SortColumn) -> Binding {
    use key::Named::{F3, F5, F6};
    if cfg!(target_os = "macos") {
        let digit = match column {
            SortColumn::Name => "1",
            SortColumn::Size => "2",
            SortColumn::Modified => "3",
        };
        Binding::with_modifiers(Key::Character(digit.into()), Modifiers::COMMAND)
    } else {
        let function_key = match column {
            SortColumn::Name => F3,
            SortColumn::Modified => F5,
            SortColumn::Size => F6,
        };
        Binding::with_modifiers(Key::Named(function_key), Modifiers::CTRL)
    }
}

/// More keys than anyone binds to one action; a bound on what a config file
/// can make the lookup walk through.
const MAX_KEYS_PER_ACTION: usize = 16;

/// Invisible formatting characters (Unicode category Cf): a key written with
/// one would look like another in the error line and the status bar.
fn is_format_character(c: char) -> bool {
    matches!(c,
        '\u{AD}'
        | '\u{600}'..='\u{605}'
        | '\u{61C}'
        | '\u{6DD}'
        | '\u{70F}'
        | '\u{180E}'
        | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{206F}'
        | '\u{FEFF}'
        | '\u{FFF9}'..='\u{FFFB}')
}

/// Why `[keys]` cannot be applied, and where in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapError {
    pub span: Range<usize>,
    pub reason: String,
}

/// Which keys do what: the built-in table, or that table with the `[keys]`
/// section of the config applied.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keymap {
    bindings: Vec<(KeyAction, Binding)>,
}

impl Default for Keymap {
    fn default() -> Self {
        (*Self::built_in()).clone()
    }
}

impl Keymap {
    /// The built-in table, built once on first use.
    ///
    /// `map_key` runs on every key press, and a binding owns a `Key`, which
    /// owns a `SmolStr`. Building the list per press meant an allocation per
    /// key repeat; holding a cursor key down made the UI visibly lag behind
    /// the keystroke.
    pub fn built_in() -> Arc<Self> {
        static BUILT_IN: OnceLock<Arc<Keymap>> = OnceLock::new();
        Arc::clone(BUILT_IN.get_or_init(|| {
            Arc::new(Keymap {
                bindings: default_bindings(),
            })
        }))
    }

    /// The built-in table with the actions named in `overrides` rebound. A
    /// named action loses all its built-in keys and gets exactly the listed
    /// ones; `[]` leaves it without a key. Two actions on one key are an
    /// error, also when the other one only has its built-in key: silently
    /// letting one win would hide the other action without a word.
    pub fn with_overrides(overrides: &KeySection) -> Result<Self, KeymapError> {
        let mut rebound: Vec<KeyAction> = Vec::new();
        for entry in &overrides.entries {
            if let Some(action) = KeyAction::from_name(entry.action.as_ref()) {
                rebound.push(action);
            }
        }

        let mut taken: Vec<(KeyAction, Binding)> = default_bindings()
            .into_iter()
            .filter(|(action, _)| !rebound.contains(action))
            .collect();
        let mut configured: Vec<(KeyAction, Vec<Binding>)> = Vec::new();
        for entry in &overrides.entries {
            let name = entry.action.as_ref();
            let action = KeyAction::from_name(name).ok_or_else(|| KeymapError {
                span: entry.action.span(),
                reason: format!("unknown action {name:?}"),
            })?;
            let mut bindings = Vec::new();
            for (index, chord) in entry.chords.iter().enumerate() {
                if index >= MAX_KEYS_PER_ACTION {
                    return Err(KeymapError {
                        span: chord.span(),
                        reason: format!("at most {MAX_KEYS_PER_ACTION} keys per action"),
                    });
                }
                let text = chord.as_ref().trim();
                let binding = parse_chord(text).map_err(|reason| KeymapError {
                    span: chord.span(),
                    reason,
                })?;
                if bindings.contains(&binding) {
                    return Err(KeymapError {
                        span: chord.span(),
                        reason: format!("{text:?} is listed twice for {name}"),
                    });
                }
                if let Some((other, _)) = taken.iter().find(|(_, held)| *held == binding) {
                    let advice = if rebound.contains(other) {
                        String::new()
                    } else {
                        format!("; give {} other keys too, or []", other.name())
                    };
                    return Err(KeymapError {
                        span: chord.span(),
                        reason: format!("{text:?} is already bound to {}{advice}", other.name()),
                    });
                }
                taken.push((action, binding.clone()));
                bindings.push(binding);
            }
            configured.push((action, bindings));
        }

        let mut bindings = Vec::new();
        let mut placed: Vec<KeyAction> = Vec::new();
        for (action, binding) in default_bindings() {
            if !rebound.contains(&action) {
                bindings.push((action, binding));
            } else if !placed.contains(&action) {
                placed.push(action);
                for (configured_action, keys) in &configured {
                    if *configured_action == action {
                        bindings.extend(keys.iter().map(|key| (action, key.clone())));
                    }
                }
            }
        }
        Ok(Self { bindings })
    }

    /// Maps a key press to a message.
    pub fn map_key(&self, key: &Key, modifiers: Modifiers) -> Option<Message> {
        self.bindings
            .iter()
            .find(|(_, binding)| binding.key == *key && binding.modifiers == modifiers)
            .map(|(action, _)| action.message())
    }

    /// The key that sends `message`, spelled for this platform: `F5`, `⇧F8`,
    /// `⌥⌘C` on a Mac, `Ctrl+Alt+C` elsewhere. `None` for a message no key
    /// sends.
    pub fn shortcut_label(&self, message: &Message) -> Option<String> {
        let (_, binding) = self
            .bindings
            .iter()
            .find(|(action, _)| action.message() == *message)?;
        let Binding { key, modifiers } = binding;
        let key_name = match key {
            Key::Named(named) => format!("{named:?}"),
            Key::Character(character) => character.to_uppercase(),
            _ => return None,
        };
        let mut label = String::new();
        if cfg!(target_os = "macos") {
            for (held, symbol) in [
                (modifiers.control(), "⌃"),
                (modifiers.alt(), "⌥"),
                (modifiers.shift(), "⇧"),
                (modifiers.logo(), "⌘"),
            ] {
                if held {
                    label.push_str(symbol);
                }
            }
            label.push_str(&key_name);
        } else {
            for (held, name) in [
                (modifiers.control(), "Ctrl"),
                (modifiers.alt(), "Alt"),
                (modifiers.shift(), "Shift"),
                (modifiers.logo(), "Super"),
            ] {
                if held {
                    label.push_str(name);
                    label.push('+');
                }
            }
            label.push_str(&key_name);
        }
        Some(label)
    }

    /// The labels of the function key bar, derived from the bindings: slot
    /// `n - 1` holds the hint of the bare `Fn` binding, `None` for a free key.
    /// Only bare function keys are shown; Shift+F8 and friends are in the help.
    pub fn function_keys(&self, lang: Language) -> [Option<&'static str>; FUNCTION_KEY_COUNT] {
        let mut slots = [None; FUNCTION_KEY_COUNT];
        for (action, binding) in &self.bindings {
            let Some(hint) = action.hint() else {
                continue;
            };
            if binding.modifiers != Modifiers::default() {
                continue;
            }
            let Some(number) = function_key_number(&binding.key) else {
                continue;
            };
            if let Some(slot) = slots.get_mut(number - 1) {
                slot.get_or_insert(lang.text(hint));
            }
        }
        slots
    }
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

/// Reads one key of the config file: `f5`, `ctrl+alt+p`, `primary+q`.
///
/// Parts are separated by `+`, case does not matter, the last one is the key
/// and the ones before it are modifiers. `primary` is Cmd on a Mac and Ctrl
/// elsewhere. A literal plus is written `plus`.
fn parse_chord(text: &str) -> Result<Binding, String> {
    if text.is_empty() {
        return Err("a key is empty".to_owned());
    }
    let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let key_part = parts.pop().unwrap_or_default();
    let mut modifiers = Modifiers::default();
    for part in parts {
        let modifier = match part.to_lowercase().as_str() {
            "primary" => Modifiers::COMMAND,
            "ctrl" | "control" => Modifiers::CTRL,
            "alt" | "option" => Modifiers::ALT,
            "shift" => Modifiers::SHIFT,
            "cmd" | "super" => Modifiers::LOGO,
            _ => return Err(format!("unknown modifier {part:?} in {text:?}")),
        };
        if modifiers.intersects(modifier) {
            return Err(format!("modifier {part:?} is given twice in {text:?}"));
        }
        modifiers |= modifier;
    }
    let key = parse_key_name(key_part)
        .ok_or_else(|| format!("unknown key {key_part:?} in {text:?}"))??;
    if let Key::Character(character) = &key {
        let is_letter = character.chars().all(char::is_alphabetic);
        if modifiers.shift() && !is_letter {
            return Err(
                "shift changes the character; write the character it types, e.g. \"*\"".to_owned(),
            );
        }
        let types_a_name = character.chars().all(char::is_alphanumeric);
        if types_a_name && !modifiers.intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::LOGO)
        {
            return Err("letters and digits need ctrl, alt or cmd: without them they jump to a file name (type-ahead)".to_owned());
        }
    }
    Ok(Binding::with_modifiers(key, modifiers))
}

/// The key a name stands for. `None` for a name that is not a key; `Some(Err)`
/// for one that is, but is not allowed.
fn parse_key_name(name: &str) -> Option<Result<Key, String>> {
    use key::Named;
    let lower = name.to_lowercase();
    let named = match lower.as_str() {
        "escape" | "esc" => {
            return Some(Err(
                "Escape is reserved: it closes dialogs and menus and stops jobs".to_owned(),
            ))
        }
        "f1" => Named::F1,
        "f2" => Named::F2,
        "f3" => Named::F3,
        "f4" => Named::F4,
        "f5" => Named::F5,
        "f6" => Named::F6,
        "f7" => Named::F7,
        "f8" => Named::F8,
        "f9" => Named::F9,
        "f10" => Named::F10,
        "f11" => Named::F11,
        "f12" => Named::F12,
        "enter" => Named::Enter,
        "tab" => Named::Tab,
        "backspace" => Named::Backspace,
        "space" => Named::Space,
        "insert" => Named::Insert,
        "delete" => Named::Delete,
        "home" => Named::Home,
        "end" => Named::End,
        "pageup" => Named::PageUp,
        "pagedown" => Named::PageDown,
        "up" => Named::ArrowUp,
        "down" => Named::ArrowDown,
        "left" => Named::ArrowLeft,
        "right" => Named::ArrowRight,
        "plus" => return Some(Ok(Key::Character("+".into()))),
        _ => {
            let mut chars = name.chars();
            return match (chars.next(), chars.next()) {
                (Some(only), None)
                    if !only.is_whitespace()
                        && !only.is_control()
                        && !is_format_character(only) =>
                {
                    Some(Ok(Key::Character(lower.as_str().into())))
                }
                _ => None,
            };
        }
    };
    Some(Ok(Key::Named(named)))
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
    use crate::config;
    use iced::keyboard::key::Named;

    fn map_key(key: Key, modifiers: Modifiers) -> Option<Message> {
        Keymap::built_in().map_key(&key, modifiers)
    }

    fn function_keys(lang: Language) -> [Option<&'static str>; FUNCTION_KEY_COUNT] {
        Keymap::built_in().function_keys(lang)
    }

    fn shortcut_label(message: &Message) -> Option<String> {
        Keymap::built_in().shortcut_label(message)
    }

    fn written(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    /// The keymap `[keys]` text produces, or the line and reason it fails with.
    fn keymap_from(keys: &str) -> Result<Keymap, (Option<usize>, String)> {
        let (_dir, path) = written(keys);
        config::load(&path)
            .map(|config| (*config.keymap).clone())
            .map_err(|err| (err.line(), err.reason().to_owned()))
    }

    fn rebound(keys: &str) -> Keymap {
        keymap_from(keys).unwrap()
    }

    fn failure(keys: &str) -> (Option<usize>, String) {
        keymap_from(keys).unwrap_err()
    }

    fn press(keymap: &Keymap, key: Key, modifiers: Modifiers) -> Option<Message> {
        keymap.map_key(&key, modifiers)
    }

    fn named(key: Named) -> Key {
        Key::Named(key)
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cmd_1_2_3_sort_by_name_size_modified() {
        for (digit, column) in [
            ("1", SortColumn::Name),
            ("2", SortColumn::Size),
            ("3", SortColumn::Modified),
        ] {
            assert_eq!(
                map_key(Key::Character(digit.into()), Modifiers::COMMAND),
                Some(Message::SortActive(column))
            );
            assert_eq!(
                map_key(Key::Character(digit.into()), Modifiers::default()),
                None
            );
        }
        assert_eq!(map_key(Key::Named(Named::F3), Modifiers::CTRL), None);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn ctrl_f3_f5_f6_sort_by_name_modified_size() {
        for (function_key, column) in [
            (Named::F3, SortColumn::Name),
            (Named::F5, SortColumn::Modified),
            (Named::F6, SortColumn::Size),
        ] {
            assert_eq!(
                map_key(Key::Named(function_key), Modifiers::CTRL),
                Some(Message::SortActive(column))
            );
        }
        assert_eq!(
            map_key(Key::Character("1".into()), Modifiers::COMMAND),
            None
        );
    }

    /// The bare function keys keep their meaning whatever the sort chords are.
    #[test]
    fn the_sort_chords_leave_the_bare_keys_alone() {
        assert_eq!(
            map_key(Key::Named(Named::F5), Modifiers::default()),
            Some(Message::Transfer(TransferKind::Copy))
        );
    }

    /// The point of this module: a label on the bar whose key does nothing, or
    /// a binding the bar never shows. Both used to be possible.
    #[test]
    fn every_labelled_slot_has_a_binding_that_responds() {
        for (index, label) in function_keys(Language::English).iter().enumerate() {
            let Some(text) = label else { continue };
            let number = index + 1;
            let bound = Keymap::built_in().bindings.iter().any(|(action, binding)| {
                action.hint().is_some()
                    && binding.modifiers == Modifiers::default()
                    && function_key_number(&binding.key) == Some(number)
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
                Some("View"),
                Some("Edit"),
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
            map_key(Key::Character("q".into()), Modifiers::COMMAND),
            Some(Message::Quit)
        );
        assert_eq!(
            map_key(Key::Character("q".into()), Modifiers::default()),
            None,
            "a bare q is type-ahead now"
        );
        assert_eq!(
            map_key(Key::Character("x".into()), Modifiers::default()),
            None
        );
    }

    #[test]
    fn alt_f1_and_alt_f2_open_the_drive_menu_of_their_panel() {
        assert_eq!(
            map_key(Key::Named(Named::F1), Modifiers::ALT),
            Some(Message::VolumeMenu(PanelSide::Left))
        );
        assert_eq!(
            map_key(Key::Named(Named::F2), Modifiers::ALT),
            Some(Message::VolumeMenu(PanelSide::Right))
        );
        assert_eq!(map_key(Key::Named(Named::F1), Modifiers::default()), None);
        assert_eq!(map_key(Key::Named(Named::F2), Modifiers::SHIFT), None);
        // Not on the bar: bare F1 and F2 stay free for help and the user menu.
        assert_eq!(function_keys(Language::English)[..2], [None, None]);
    }

    #[test]
    fn shift_f10_opens_the_context_menu_and_does_not_quit() {
        assert_eq!(
            map_key(Key::Named(Named::F10), Modifiers::SHIFT),
            Some(Message::ContextMenuKey)
        );
        assert_eq!(
            map_key(Key::Named(Named::F10), Modifiers::default()),
            Some(Message::Quit)
        );
    }

    #[test]
    fn shortcuts_are_spelled_for_the_platform() {
        let label = |message: Message| shortcut_label(&message).unwrap();
        assert_eq!(label(Message::OpenSelected), "Enter");
        assert_eq!(label(Message::Transfer(TransferKind::Copy)), "F5");
        if cfg!(target_os = "macos") {
            assert_eq!(label(Message::Delete { permanent: true }), "⇧F8");
            assert_eq!(label(Message::CopyToClipboard(ClipboardKind::Path)), "⌥⌘C");
            assert_eq!(label(Message::CopyToClipboard(ClipboardKind::Name)), "⌃⌘C");
        } else {
            assert_eq!(label(Message::Delete { permanent: true }), "Shift+F8");
            assert_eq!(
                label(Message::CopyToClipboard(ClipboardKind::Path)),
                "Ctrl+Alt+C"
            );
            assert_eq!(
                label(Message::CopyToClipboard(ClipboardKind::Name)),
                "Ctrl+Shift+C"
            );
        }
        assert_eq!(shortcut_label(&Message::SwitchLanguage), Some("F9".into()));
        assert_eq!(shortcut_label(&Message::ContextMenuClose), None);
    }

    /// A modifier must actually gate the binding: Alt+Enter is not Enter.
    #[test]
    fn modifiers_gate_the_lookup() {
        assert_eq!(map_key(Key::Named(Named::Enter), Modifiers::ALT), None);
    }

    #[test]
    fn action_names_round_trip_and_are_unique() {
        for action in KeyAction::ALL {
            assert_eq!(KeyAction::from_name(action.name()), Some(*action));
        }
        let mut names: Vec<_> = KeyAction::ALL.iter().map(|action| action.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), KeyAction::ALL.len());
        assert_eq!(KeyAction::from_name("nope"), None);
    }

    #[test]
    fn every_action_has_a_default_key_and_no_key_is_twice_bound() {
        let bindings = default_bindings();
        for action in KeyAction::ALL {
            assert!(
                bindings.iter().any(|(bound, _)| bound == action),
                "{} has no default key",
                action.name()
            );
        }
        for (index, (_, binding)) in bindings.iter().enumerate() {
            assert!(
                !bindings[index + 1..]
                    .iter()
                    .any(|(_, other)| other == binding),
                "{binding:?} is bound twice"
            );
        }
    }

    /// A default the user could not write would make `[keys]` unable to say
    /// what the app does by itself, and a bare letter would shadow type-ahead.
    #[test]
    fn the_defaults_obey_the_rules_for_configured_keys() {
        for (action, binding) in default_bindings() {
            if let Key::Character(character) = &binding.key {
                let types_a_name = character.chars().all(char::is_alphanumeric);
                let has_chord_modifier = binding
                    .modifiers
                    .intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::LOGO);
                assert!(
                    !types_a_name || has_chord_modifier,
                    "{} would shadow type-ahead",
                    action.name()
                );
                assert!(
                    !binding.modifiers.shift() || character.chars().all(char::is_alphabetic),
                    "{} uses shift with a character it changes",
                    action.name()
                );
            }
            assert_ne!(binding.key, Key::Named(Named::Escape));
        }
    }

    #[test]
    fn a_rebound_action_answers_the_new_key_and_not_the_old_one() {
        let keymap = rebound("[keys]\ncopy = \"F2\"\n");
        let copy = Some(Message::Transfer(TransferKind::Copy));
        assert_eq!(press(&keymap, named(Named::F2), Modifiers::default()), copy);
        assert_eq!(press(&keymap, named(Named::F5), Modifiers::default()), None);
        assert_eq!(
            keymap.shortcut_label(&Message::Transfer(TransferKind::Copy)),
            Some("F2".into())
        );
        assert_eq!(
            keymap.function_keys(Language::English)[..5],
            [None, Some("Copy"), Some("View"), Some("Edit"), None]
        );
    }

    #[test]
    fn a_file_without_keys_leaves_the_built_in_keymap() {
        assert_eq!(
            rebound("[open]\nview = [\"viewer\"]\n"),
            *Keymap::built_in()
        );
        assert_eq!(rebound(""), *Keymap::built_in());
    }

    #[test]
    fn a_key_taken_from_an_unnamed_action_is_refused_at_its_line() {
        let (line, reason) = failure("[keys]\n\nview = \"F4\"\n");
        assert_eq!(line, Some(3));
        assert!(reason.contains("edit"), "{reason}");
    }

    #[test]
    fn swapping_two_keys_works_when_both_are_named() {
        let keymap = rebound("[keys]\nview = \"F4\"\nedit = \"F3\"\n");
        assert_eq!(
            press(&keymap, named(Named::F3), Modifiers::default()),
            Some(Message::OpenExternal(OpenKind::Edit))
        );
        assert_eq!(
            press(&keymap, named(Named::F4), Modifiers::default()),
            Some(Message::OpenExternal(OpenKind::View))
        );
        assert_eq!(
            keymap.function_keys(Language::English)[2..4],
            [Some("Edit"), Some("View")]
        );
    }

    /// `move` comes after `copy` in the enum but first in the file: the later
    /// line is the one that is reported, not the later action.
    #[test]
    fn a_clash_of_two_named_actions_is_reported_at_the_later_line() {
        let (line, reason) = failure("[keys]\nmove = \"F2\"\n\ncopy = \"F2\"\n");
        assert_eq!(line, Some(4));
        assert!(reason.contains("move"), "{reason}");
        let (reversed_line, _) = failure("[keys]\ncopy = \"F2\"\nmove = \"F2\"\n");
        assert_eq!(reversed_line, Some(3));
    }

    #[test]
    fn the_same_key_twice_for_one_action_is_refused() {
        let (line, reason) = failure("[keys]\nquit = [\"F10\",\n  \"f10\"]\n");
        assert_eq!(line, Some(3));
        assert!(reason.contains("listed twice"), "{reason}");
    }

    #[test]
    fn an_unknown_action_is_reported_at_its_own_line() {
        let (line, reason) = failure("[keys]\ncopy = \"F2\"\n\nfoo = \"F7\"\n");
        assert_eq!(line, Some(4));
        assert_eq!(reason, "unknown action \"foo\"");
    }

    #[test]
    fn a_bad_key_is_reported_at_its_own_line_also_inside_a_list() {
        let (line, reason) = failure("[keys]\nquit = [\n  \"F10\",\n  \"ctrl+xyz\",\n]\n");
        assert_eq!(line, Some(4));
        assert_eq!(reason, "unknown key \"xyz\" in \"ctrl+xyz\"");
        let (second_line, _) = failure("[keys]\ncopy = \"F2\"\nquit = \"hyper+q\"\n");
        assert_eq!(second_line, Some(3));
    }

    #[test]
    fn keys_that_would_break_the_app_are_refused() {
        for bad in [
            "escape",
            "esc",
            "Ctrl+Esc",
            "a",
            "shift+a",
            "5",
            "shift+5",
            "shift+8",
            "shift+*",
            "",
            "  ",
            "ctrl+",
            "ctrl+ctrl+c",
            "ctrl+control+c",
            "ab",
        ] {
            let result = keymap_from(&format!("[keys]\ncopy = \"{bad}\"\n"));
            assert!(result.is_err(), "{bad:?} was accepted");
        }
        let (_, reason) = failure("[keys]\ncopy = \"escape\"\n");
        assert!(reason.starts_with("Escape is reserved"), "{reason}");
        let (_, letter_reason) = failure("[keys]\ncopy = \"a\"\n");
        assert!(letter_reason.contains("type-ahead"), "{letter_reason}");
    }

    #[test]
    fn a_broken_keymap_drops_the_whole_file_including_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[open]\nview = [\"viewer\"]\n[keys]\ncopy = \"escape\"\n",
        )
        .unwrap();
        config::load(&path).unwrap_err();
    }

    #[test]
    fn an_empty_list_unbinds_the_action() {
        let keymap = rebound("[keys]\ndelete_permanently = []\n");
        assert_eq!(press(&keymap, named(Named::F8), Modifiers::SHIFT), None);
        assert_eq!(
            keymap.shortcut_label(&Message::Delete { permanent: true }),
            None
        );
        assert_eq!(
            press(&keymap, named(Named::F8), Modifiers::default()),
            Some(Message::Delete { permanent: false })
        );
    }

    #[test]
    fn keys_are_case_blind_and_lists_keep_their_order() {
        let keymap =
            rebound("[keys]\nquit = [\"PRIMARY+Q\", \"f10\"]\ncopy_path = \" Ctrl + Alt + P \"\n");
        assert_eq!(
            press(&keymap, Key::Character("q".into()), Modifiers::COMMAND),
            Some(Message::Quit)
        );
        assert_eq!(
            press(&keymap, named(Named::F10), Modifiers::default()),
            Some(Message::Quit)
        );
        assert_eq!(
            press(
                &keymap,
                Key::Character("p".into()),
                Modifiers::CTRL | Modifiers::ALT
            ),
            Some(Message::CopyToClipboard(ClipboardKind::Path))
        );
        let only_first = rebound("[keys]\nquit = [\"primary+q\", \"f10\"]\n");
        assert_eq!(
            only_first.shortcut_label(&Message::Quit),
            Keymap::built_in().shortcut_label(&Message::Quit)
        );
        let reversed = rebound("[keys]\nquit = [\"f10\", \"primary+q\"]\n");
        assert_eq!(reversed.shortcut_label(&Message::Quit), Some("F10".into()));
    }

    #[test]
    fn the_readme_example_loads_as_written() {
        let keymap = rebound(
            "[keys]\n# F2 copies, F5 does nothing any more.\ncopy = \"F2\"\n# Several keys for one action.\nquit = [\"F10\", \"primary+q\"]\n# No key at all; the context menu still offers it.\ndelete_permanently = []\n# A key that belongs to another action has to be freed there too:\n# F3 and F4 swapped.\nview = \"F4\"\nedit = \"F3\"\n",
        );
        let bare = Modifiers::default();
        assert_eq!(
            press(&keymap, named(Named::F2), bare),
            Some(Message::Transfer(TransferKind::Copy))
        );
        assert_eq!(press(&keymap, named(Named::F5), bare), None);
        assert_eq!(
            press(&keymap, named(Named::F3), bare),
            Some(Message::OpenExternal(OpenKind::Edit))
        );
        assert_eq!(
            press(&keymap, named(Named::F4), bare),
            Some(Message::OpenExternal(OpenKind::View))
        );
        assert_eq!(press(&keymap, named(Named::F8), Modifiers::SHIFT), None);
    }

    #[test]
    fn too_many_keys_for_one_action_are_refused_at_the_first_excess_one() {
        let keys: Vec<String> = (0..=MAX_KEYS_PER_ACTION)
            .filter_map(|index| char::from_u32(u32::from(b'a') + index as u32))
            .map(|letter| format!("alt+{letter}"))
            .collect();
        let list = keys
            .iter()
            .map(|key| format!("  \"{key}\",\n"))
            .collect::<String>();
        let (line, reason) = failure(&format!("[keys]\ncopy = [\n{list}]\n"));
        assert_eq!(reason, "at most 16 keys per action");
        assert_eq!(line, Some(3 + MAX_KEYS_PER_ACTION));
    }

    #[test]
    fn invisible_format_characters_are_not_keys() {
        for invisible in ["\u{202E}", "\u{200B}", "\u{FEFF}", "ctrl+\u{202E}"] {
            let (_, reason) = failure(&format!("[keys]\ncopy = \"{invisible}\"\n"));
            assert!(reason.contains("unknown key"), "{invisible:?}: {reason}");
        }
    }

    #[test]
    fn plus_names_the_plus_character() {
        let keymap = rebound("[keys]\ncopy = \"ctrl+plus\"\nmove = \"PLUS\"\n");
        assert_eq!(
            press(&keymap, Key::Character("+".into()), Modifiers::CTRL),
            Some(Message::Transfer(TransferKind::Copy))
        );
        assert_eq!(
            press(&keymap, Key::Character("+".into()), Modifiers::default()),
            Some(Message::Transfer(TransferKind::Move))
        );
    }
}
