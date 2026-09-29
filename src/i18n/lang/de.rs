//! German. One `const` per key in `super::en`; see `Msg::note` for the context
//! of each string.

use crate::i18n::Msg;

pub const COL_NAME: &str = "Name";
pub const COL_SIZE: &str = "Größe";
pub const COL_MODIFIED: &str = "Geändert";
pub const MARKER_DIR: &str = "<DIR>";
pub const MARKER_UP: &str = "<UP>";
pub const LOADING: &str = "Lade…";
pub const SHORTCUT_SWITCH_PANEL: &str = "Panel wechseln";
pub const SHORTCUT_OPEN: &str = "Öffnen";
pub const SHORTCUT_UP: &str = "Zurück";
pub const SHORTCUT_QUIT: &str = "Beenden";
pub const ERROR_CANNOT_READ: &str = "{path} nicht lesbar: {reason}";
pub const ERROR_TASK_FAILED: &str = "Lesen fehlgeschlagen: {reason}";

/// Lookup for the German strings, falling back to English.
pub fn text(msg: Msg) -> &'static str {
    match msg {
        Msg::ColName => COL_NAME,
        Msg::ColSize => COL_SIZE,
        Msg::ColModified => COL_MODIFIED,
        Msg::MarkerDir => MARKER_DIR,
        Msg::MarkerUp => MARKER_UP,
        Msg::Loading => LOADING,
        Msg::ShortcutSwitchPanel => SHORTCUT_SWITCH_PANEL,
        Msg::ShortcutSwitchLanguage => SHORTCUT_SWITCH_LANGUAGE,
        Msg::ShortcutOpen => SHORTCUT_OPEN,
        Msg::ShortcutUp => SHORTCUT_UP,
        Msg::ShortcutQuit => SHORTCUT_QUIT,
        Msg::ErrorCannotRead => ERROR_CANNOT_READ,
        Msg::ErrorTaskFailed => ERROR_TASK_FAILED,
    }
}
pub const SHORTCUT_SWITCH_LANGUAGE: &str = "Sprache wechseln";
