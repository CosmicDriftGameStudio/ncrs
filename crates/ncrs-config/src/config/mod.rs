//! The config file: the shared shape of everything the user can set.
//!
//! Later subsystems (keymap, theme, language) add their sections as optional
//! fields on `Config`, so a file written for today's version keeps loading.
//! `version` is there for migration. Unknown fields are rejected rather than
//! ignored: a misspelt key would otherwise silently do nothing.
//!
//! A missing file is not an error, the defaults apply. A broken file is: the
//! whole config falls back to the defaults (no partial merge, which would
//! leave a half-applied setup nobody can reason about) and the caller reports
//! the error.

use std::ffi::OsString;
use std::fmt;
use std::io::Read as _;
use std::num::{NonZeroI64, NonZeroU32, NonZeroUsize};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub use ncrs_fs::{
    forbid_user_machine_in_tests, is_separator, replace_file_atomically, OpenPrograms, ProgramLine,
    ProgramLineProblem,
};
use ncrs_index::{iban_is_valid, CustomKind, KindCatalog, KindSkip};
use ncrs_intake::{pattern_is_valid, LocationMode};
use ncrs_pipeline::Load;
use serde::de::{Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_spanned::Spanned;

use crate::file_list_tags::{FileListTags, TagGroup};
use crate::keymap::{KeyAction, Keymap};
use crate::palette::{
    ColorRole, Palette, PaletteFromSection as _, ThemePreset, PRESET_KEY, VSCODE_KEY,
};
use crate::vscode_theme::VscodeTheme;
use ncrs_fs::OpenKind;
use ncrs_i18n::Language;

const FIRST_VERSION: u32 = 1;

/// The newest version this build reads. A change that would reject or
/// reinterpret what an older file says raises it and converts the older form
/// while loading, in memory; the file is never rewritten to migrate it.
pub const CURRENT_VERSION: u32 = 1;

/// A config is a few lines; anything bigger is not one, and reading it would
/// only delay the start. A VS Code theme file is held to the same limit.
pub const SIZE_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub version: Version,
    /// The `lang` entry as written; `load` checks it into `lang`.
    #[serde(default, rename = "lang")]
    pub lang_entry: Option<Spanned<String>>,
    /// The language the session starts in; F9 switches it for the session
    /// only, nothing is written back.
    #[serde(skip)]
    pub lang: Option<Language>,
    /// Set when `lang` names no known language; English is used then.
    #[serde(skip)]
    pub language_warning: Option<LanguageWarning>,
    #[serde(default)]
    pub open: OpenPrograms,
    /// The `[keys]` section as written; `load` turns it into `keymap`.
    #[serde(default)]
    pub keys: KeySection,
    /// The keys in effect: the built-in ones with `keys` applied.
    #[serde(skip)]
    pub keymap: Arc<Keymap>,
    /// The `[theme]` section as written; `load` turns it into `palette`.
    #[serde(default)]
    pub theme: ThemeSection,
    /// The colors in effect: the built-in ones with `theme` applied.
    #[serde(skip)]
    pub palette: Palette,
    /// Set when the palette is legal but hard to read. It does not stop the
    /// palette from being used.
    #[serde(skip)]
    pub contrast_warning: Option<ContrastWarning>,
    /// Set when the VS Code theme named in `[theme]` could not be used in
    /// full. The preset and the roles still apply.
    #[serde(skip)]
    pub vscode_warning: Option<VscodeWarning>,
    /// The `[modules]` sections as written; `load` turns `filing` into the
    /// field of the same name below.
    #[serde(default)]
    pub modules: ModulesSection,
    /// The filing module as `load` checked it: text rules only, the folders
    /// may not exist.
    #[serde(skip)]
    pub filing: Filing,
    /// The `[[servers]]` entries as written; `load` checks them into `servers`.
    #[serde(default, rename = "servers")]
    pub server_sections: Vec<ServerSection>,
    /// The saved SFTP servers, in the order of the file.
    #[serde(skip)]
    pub servers: Vec<ServerEntry>,
    /// The `[file_list_tags]` section as written; `load` checks it into
    /// `file_list_tags`.
    #[serde(default, rename = "file_list_tags")]
    pub file_list_tags_section: FileListTagsSection,
    /// The tags the file panels show after a name.
    #[serde(skip)]
    pub file_list_tags: FileListTags,
    /// The `[[kinds]]` entries as written; `load` checks them into `kinds`.
    #[serde(default, rename = "kinds")]
    pub kind_sections: Vec<KindSection>,
    /// The document kinds in effect: the built-in ones with `kind_sections`
    /// applied.
    #[serde(skip)]
    pub kinds: Arc<KindCatalog>,
    /// What `load` left out of `kind_sections`, if anything.
    #[serde(skip)]
    pub kind_warning: Option<KindWarning>,
}

/// The `version` key: a missing one is the first version. Anything outside
/// `FIRST_VERSION..=CURRENT_VERSION` is refused, so a file from a newer ncrs
/// is not half understood.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version(u32);

impl Default for Version {
    fn default() -> Self {
        Self(FIRST_VERSION)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let number = u32::deserialize(deserializer)?;
        if number < FIRST_VERSION {
            Err(D::Error::custom(format!(
                "version {number} does not exist, the first version is {FIRST_VERSION}"
            )))
        } else if number > CURRENT_VERSION {
            Err(D::Error::custom(format!(
                "config.toml is version {number}, written by a newer ncrs; this ncrs reads up to version {CURRENT_VERSION}"
            )))
        } else {
            Ok(Self(number))
        }
    }
}

struct FilingError {
    span: Range<usize>,
    reason: String,
}

mod error;
mod loading;
mod paths;
mod resolving;
mod sections;
mod setting_writes;
mod write_support;
mod writing;

pub use self::error::ConfigError;
#[cfg(any(test, feature = "test-support"))]
pub use self::loading::from_text;
use self::loading::*;
pub use self::loading::{load, read_source};
use self::paths::*;
pub use self::paths::{
    default_index_path, default_journal_path, default_known_hosts_path, default_path,
    default_ssh_known_hosts_path, default_state_path, default_view_copies_dir, portable_data_dir,
    portable_data_dir_of,
};
use self::resolving::*;
pub use self::resolving::{check_ai_endpoint, AiEndpointProblem};
pub use self::sections::{
    AiProviderName, AiSection, AiSetting, ContrastWarning, FileListTagsSection, Filing,
    FilingLocation, FilingSection, KeyEntry, KeySection, KindSection, KindWarning, LanguageWarning,
    LoadValue, LocationEntry, LocationModeName, ModulesSection, ServerEntry, ServerSection,
    ThemeEntry, ThemeSection, VscodeWarning,
};
pub use self::setting_writes::{
    remove_vscode_theme, write_ai_enabled, write_ai_provider, write_color, write_default_pattern,
    write_file_list_tags, write_filing_enabled, write_filing_load, write_keys, write_language,
    write_own_details, write_program, write_theme_preset, AiProviderChoice, KeyChange, OwnDetail,
};
pub use self::write_support::one_config_writer;
use self::write_support::*;
use self::writing::*;
pub use self::writing::{
    check_filing_rules, write_filing_locations, write_servers, write_vscode_theme,
    FilingRuleViolation, FilingWriteError,
};

impl Config {
    /// The preset `[theme]` names; the built-in look without one.
    pub fn theme_preset(&self) -> ThemePreset {
        self.theme_text(PRESET_KEY)
            .and_then(ThemePreset::from_name)
            .unwrap_or(ThemePreset::DEFAULT)
    }

    /// The VS Code theme file `[theme]` names, as written.
    pub fn vscode_theme_path(&self) -> Option<&str> {
        self.theme_text(VSCODE_KEY)
    }

    fn theme_text(&self, key: &str) -> Option<&str> {
        self.theme
            .entries
            .iter()
            .find(|entry| entry.key.get_ref() == key)
            .map(|entry| entry.value.get_ref().as_str())
    }
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests;
