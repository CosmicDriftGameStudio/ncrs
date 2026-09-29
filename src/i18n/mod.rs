//! Localization: translated UI strings with per-string context.
//!
//! Every entry carries a `note` describing where the text appears and what it
//! means. Mechanical translation (a person or tool working from this file
//! alone) needs that context: "Open" as a button label, as a menu entry and
//! inside "Cannot open file" are three different strings.
//!
//! Adding a language: add a module in `lang/`, give it the same key set, run
//! the test — a missing key is a compile error in `tests::all_languages_agree`.

use std::fmt;

pub mod lang {
    pub mod de;
    pub mod en;
}

/// Every translatable string, addressed by an enum so the compiler finds
/// call sites when a string is added or renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    // --- Panel column headers ---
    ColName,
    ColSize,
    ColModified,
    /// Cell content for a directory. Not a word — a marker in angle brackets,
    /// kept as-is in all languages.
    MarkerDir,
    /// Cell content for the "go to parent" entry.
    MarkerUp,
    /// Status bar while a directory read is in flight.
    Loading,
    // --- Header shortcuts ---
    ShortcutSwitchPanel,
    ShortcutSwitchLanguage,
    ShortcutOpen,
    ShortcutUp,
    ShortcutQuit,
    // --- Errors (fs layer) ---
    ErrorCannotRead,
    ErrorTaskFailed,
}

impl Msg {
    pub fn text(self) -> &'static str {
        match self {
            Msg::ColName => lang::en::COL_NAME,
            Msg::ColSize => lang::en::COL_SIZE,
            Msg::ColModified => lang::en::COL_MODIFIED,
            Msg::MarkerDir => lang::en::MARKER_DIR,
            Msg::MarkerUp => lang::en::MARKER_UP,
            Msg::Loading => lang::en::LOADING,
            Msg::ShortcutSwitchPanel => lang::en::SHORTCUT_SWITCH_PANEL,
            Msg::ShortcutSwitchLanguage => lang::en::SHORTCUT_SWITCH_LANGUAGE,
            Msg::ShortcutOpen => lang::en::SHORTCUT_OPEN,
            Msg::ShortcutUp => lang::en::SHORTCUT_UP,
            Msg::ShortcutQuit => lang::en::SHORTCUT_QUIT,
            Msg::ErrorCannotRead => lang::en::ERROR_CANNOT_READ,
            Msg::ErrorTaskFailed => lang::en::ERROR_TASK_FAILED,
        }
    }

    /// Stable identifier, independent of the Rust variant name. This is what a
    /// `strings.json` would key on, so renaming a variant does not break
    /// existing translations.
    ///
    /// Only the translation dump reads this; same reasoning as [`Self::note`].
    #[allow(dead_code)]
    pub fn key(self) -> &'static str {
        match self {
            Msg::ColName => "column.name",
            Msg::ColSize => "column.size",
            Msg::ColModified => "column.modified",
            Msg::MarkerDir => "marker.dir",
            Msg::MarkerUp => "marker.up",
            Msg::Loading => "status.loading",
            Msg::ShortcutSwitchPanel => "shortcut.switch_panel",
            Msg::ShortcutSwitchLanguage => "shortcut.switch_language",
            Msg::ShortcutOpen => "shortcut.open",
            Msg::ShortcutUp => "shortcut.up",
            Msg::ShortcutQuit => "shortcut.quit",
            Msg::ErrorCannotRead => "error.cannot_read",
            Msg::ErrorTaskFailed => "error.task_failed",
        }
    }

    /// Context for translators. Read this before translating [`Self::text`].
    ///
    /// Only the localization test reads this, which is why it carries
    /// `#[allow(dead_code)]`: it is a deliverable for translators, not runtime
    /// behaviour, and it must stay compiled so a string cannot lose its context.
    #[allow(dead_code)]
    pub fn note(self) -> &'static str {
        match self {
            Msg::ColName => "Column header above the file name column. One word, noun.",
            Msg::ColSize => "Column header above the file size column. One word, noun.",
            Msg::ColModified => "Column header above the modification date. One word; may expand in some languages.",
            Msg::MarkerDir => "Shown in the size column for directories, like <DIR>. Convention from Norton Commander. Keep the angle brackets; translate only the letters if the language convention differs.",
            Msg::MarkerUp => "Shown in the size column for the parent-directory entry '..', like <UP>. Same convention as the directory marker.",
            Msg::Loading => "Status bar text while a directory is being read. Show progress; the surrounding UI is monospace, so an ellipsis character is fine.",
            Msg::ShortcutSwitchPanel => "Header hint describing the Tab key. Imperative or infinitive, matching the other hints in this row.",
            Msg::ShortcutSwitchLanguage => "Header hint describing the F9 key, which switches the interface language. Say 'language', not 'translation' or 'locale'.",
            Msg::ShortcutOpen => "Header hint describing the Enter key, which opens the selected directory.",
            Msg::ShortcutUp => "Header hint describing the Backspace key, which goes to the parent directory.",
            Msg::ShortcutQuit => "Header hint describing the F10 and Q keys, which quit the app.",
            Msg::ErrorCannotRead => "Status bar error combining the directory path and the OS error message. Both must appear; word order is up to the language (German leads with the path). Keep the {path} and {reason} placeholders exactly as spelled.",
            Msg::ErrorTaskFailed => "Status bar error shown when the background read task itself failed. Internal error, users should rarely see it.",
        }
    }
}

/// The language the UI is currently rendered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    German,
}

impl Language {
    /// Picks the translation for `msg`. German currently has every key; the
    /// fallback to English exists so a partially translated language is
    /// usable instead of panicking.
    pub fn text(self, msg: Msg) -> &'static str {
        match (self, msg) {
            (_, m) if m == Msg::MarkerDir || m == Msg::MarkerUp => m.text(),
            (Language::English, _) => msg.text(),
            (Language::German, m) => lang::de::text(m),
        }
    }
}

/// Every translatable string. Used by the tests and by the translation dump.
#[cfg_attr(not(test), allow(dead_code))]
pub const ALL: &[Msg] = &[
    Msg::ColName,
    Msg::ColSize,
    Msg::ColModified,
    Msg::MarkerDir,
    Msg::MarkerUp,
    Msg::Loading,
    Msg::ShortcutSwitchPanel,
    Msg::ShortcutSwitchLanguage,
    Msg::ShortcutOpen,
    Msg::ShortcutUp,
    Msg::ShortcutQuit,
    Msg::ErrorCannotRead,
    Msg::ErrorTaskFailed,
];

impl Language {
    /// Switches to the next language, for the temporary key binding.
    pub fn other(self) -> Self {
        match self {
            Language::English => Language::German,
            Language::German => Language::English,
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Language::English => "en",
            Language::German => "de",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A language that is missing a key must fall back to English rather than
    /// panic, and the key set must stay in sync across languages.
    #[test]
    fn every_message_has_context_for_translators() {
        for msg in ALL {
            let note = msg.note();
            assert!(
                note.len() > 20,
                "{msg:?} has no usable translation context: {note:?}"
            );
        }
    }

    #[test]
    fn german_differs_from_english_where_it_should() {
        // Not listed: ColName, because "Name" is the German word as well, and
        // ErrorTaskFailed, an internal error whose wording is free to change.
        let differing = [
            Msg::ColSize,
            Msg::ColModified,
            Msg::Loading,
            Msg::ShortcutSwitchPanel,
            Msg::ShortcutOpen,
            Msg::ShortcutUp,
            Msg::ShortcutQuit,
            Msg::ErrorCannotRead,
        ];
        for msg in differing {
            assert_ne!(
                Language::English.text(msg),
                Language::German.text(msg),
                "{msg:?} is identical in both languages"
            );
        }
    }

    /// Templates that interpolate values must use the same placeholder names in
    /// every language, otherwise one language silently shows `{reason}` to the
    /// user. Word order may differ, spelling of the placeholder may not.
    #[test]
    fn placeholders_match_across_languages() {
        for msg in ALL {
            let en = Language::English.text(*msg);
            let de = Language::German.text(*msg);
            for placeholder in ["{path}", "{reason}"] {
                assert_eq!(
                    en.contains(placeholder),
                    de.contains(placeholder),
                    "{msg:?} uses {placeholder} in one language but not the other"
                );
            }
        }
    }

    /// The marker convention is deliberately not translated.
    #[test]
    fn markers_stay_identical_across_languages() {
        for lang in [Language::English, Language::German] {
            assert_eq!(lang.text(Msg::MarkerDir), "<DIR>");
            assert_eq!(lang.text(Msg::MarkerUp), "<UP>");
        }
    }
}

#[cfg(test)]
mod dump {
    use super::{Language, ALL};

    /// Not a test: prints every string with its context and all translations,
    /// so a translator can work from this file alone.
    /// Run with `cargo test dump -- --nocapture`.
    #[test]
    fn print_translation_sheet() {
        for msg in ALL {
            println!("\n## {:?}  (key: {})", msg, msg.key());
            println!("EN  {}", Language::English.text(*msg));
            println!("DE  {}", Language::German.text(*msg));
            println!("NOTE  {}", msg.note());
        }
        println!("\n( {} strings )", ALL.len());
    }
}
