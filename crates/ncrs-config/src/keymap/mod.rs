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
use ncrs_fs::SortColumn;
use ncrs_i18n::{Language, Msg};

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

mod action;
mod bindings;
mod chords;
mod defaults;

use self::action::*;
pub use self::action::{system_reads_scans, KeyAction, KeyScope};
pub use self::bindings::{Keymap, KeymapError, FUNCTION_KEY_COUNT, NEW_FOLDER_SLOT};
use self::chords::*;
pub use self::chords::{
    binding_label, built_in_bindings_of, configurable_chord, recorded_binding, KeyRefusal,
    MAX_KEYS_PER_ACTION,
};
pub use self::defaults::sort_binding;
use self::defaults::*;

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests;
