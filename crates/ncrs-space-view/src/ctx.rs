//! What the Platz tab takes from the app: data it reads, never state it changes.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use iced::Size;
use ncrs_config::space::SpaceSettings;
use ncrs_i18n::Language;
use ncrs_ui_kit::palette::Palette;

use crate::keys::SpaceKeys;

/// What `update` reads from the app.
#[derive(Debug, Clone, PartialEq)]
pub struct Ctx {
    pub lang: Language,
    pub home: PathBuf,
    /// The repositories of the Repos tab: where worktrees and `target` folders are looked for.
    pub repos: Vec<PathBuf>,
    /// `[space]` of the config.
    pub settings: SpaceSettings,
    /// The running processes can be listed. `false` in the App Store build: the delete
    /// then needs the user's confirm that ncrs cannot check for a running build.
    pub processes_known: bool,
    /// The App Store build: no worktree removal, no simulators.
    pub store_build: bool,
    /// Where the run logs are written and read.
    pub log_dir: Option<PathBuf>,
    /// The trash can empty single files (Linux, Windows): "empty the trash afterwards" is offered.
    pub trash_can_purge: bool,
    /// The app holds a run of its own trash that Undo can bring back.
    pub can_undo_trash: bool,
    pub window: Size,
    pub now: SystemTime,
}

impl Ctx {
    /// A tab that logs nothing: every check possible, the trash cannot purge.
    pub fn new(lang: Language, home: PathBuf) -> Self {
        Self {
            lang,
            home,
            repos: Vec::new(),
            settings: SpaceSettings::default(),
            processes_known: true,
            store_build: false,
            log_dir: None,
            trash_can_purge: false,
            can_undo_trash: false,
            window: Size::new(1740.0, 1130.0),
            now: SystemTime::UNIX_EPOCH,
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
    /// The keys of the actions, `[keys.space]` applied.
    pub keys: &'a SpaceKeys,
    /// What a leading `~` shortens in a place.
    pub home: &'a Path,
    pub now: SystemTime,
    pub store_build: bool,
    pub processes_known: bool,
    pub trash_can_purge: bool,
    pub can_undo_trash: bool,
    /// The drive counts as full from this used share on.
    pub warn_from_percent: u8,
    /// The label of the key that opens the tab from anywhere; `None` when it is unbound.
    pub open_key: Option<&'a str>,
}
