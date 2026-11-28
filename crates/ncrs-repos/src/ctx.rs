//! What the Repos tab takes from the app: data it reads, never state it changes.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use iced::Size;
use ncrs_config::keymap::Keymap;
use ncrs_config::repos::ReposSettings;
use ncrs_i18n::Language;
use ncrs_ui_kit::palette::Palette;

use crate::keys::ReposKeys;

/// What `update` reads from the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ctx {
    pub lang: Language,
    /// `[repos]` of the config.
    pub settings: ReposSettings,
    pub home: Option<PathBuf>,
    /// `repos.toml`; none keeps the list in memory only.
    pub file: Option<PathBuf>,
    /// The clock, for the log and the relative times.
    pub now: SystemTime,
    /// The write actions run: git is there and this is not the sandbox build.
    pub actions_available: bool,
    /// The folders the sandbox build may read; `None` where there is no sandbox.
    pub sandbox_roots: Option<Vec<PathBuf>>,
    /// The repositories shown in a Git tab right now.
    pub open_roots: Vec<PathBuf>,
}

impl Ctx {
    /// A tab that remembers nothing: no file, no sources, the actions on.
    pub fn new(lang: Language) -> Self {
        Self {
            lang,
            settings: ReposSettings::default(),
            home: None,
            file: None,
            now: SystemTime::UNIX_EPOCH,
            actions_available: true,
            sandbox_roots: None,
            open_roots: Vec::new(),
        }
    }
}

/// What `view` reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct ViewCtx<'a> {
    /// The used share of the drive, once it is past the warn level.
    pub disk_warning: Option<u8>,
    pub lang: Language,
    pub palette: &'a Palette,
    /// The window: the tab fills it.
    pub window: Size,
    /// The keys of the actions, `[keys.repos]` applied.
    pub keys: &'a ReposKeys,
    /// The keymap of the app: the menu shows the keys it has for opening, the terminal and copying the path.
    pub keymap: &'a Keymap,
    /// What a leading `~` shortens in a place.
    pub home: Option<&'a Path>,
    /// The clock, for the relative times.
    pub now: SystemTime,
    /// The write actions run, see [`Ctx::actions_available`].
    pub actions_available: bool,
    /// `[repos]` of the config: the column widths.
    pub settings: &'a ReposSettings,
}
