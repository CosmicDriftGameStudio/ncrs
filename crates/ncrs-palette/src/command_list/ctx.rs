use iced::Size;
use ncrs_config::keymap::{KeyAction, KeyScope, Keymap};
use ncrs_config::SettingsTarget;
use ncrs_i18n::Language;

/// Which of the Hermes commands on a file the list offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HermesCommands {
    /// The Hermes column is not up: none of them.
    None,
    /// No locations are configured: only those that set a folder's role.
    RolesOnly,
    /// Everything but "process again", which cannot run for the row under the
    /// cursor.
    WithoutProcessAgain,
    All,
}

impl HermesCommands {
    /// From what the app knows: the Hermes column is up (`ready`), locations
    /// are configured (`active`), and "process again" can run for the row
    /// under the cursor.
    pub fn from_facts(
        ready: bool,
        active: bool,
        process_again_enabled: impl FnOnce() -> bool,
    ) -> Self {
        if !ready {
            Self::None
        } else if !active {
            Self::RolesOnly
        } else if !process_again_enabled() {
            Self::WithoutProcessAgain
        } else {
            Self::All
        }
    }
}

/// What the command list reads from the app.
#[derive(Debug)]
pub struct CommandPaletteCtx<'a> {
    pub lang: Language,
    pub keymap: &'a Keymap,
    /// The commands run last, most recent first.
    pub recent: &'a [KeyAction],
    pub window: Size,
    /// A dialog or a page covers the panels: the list cannot open.
    pub blocked: bool,
    /// Where the keys work now; the list offers the commands of this scope.
    pub scope: KeyScope,
    pub hermes: HermesCommands,
    /// The settings pages and settings a query can find. Read only when the
    /// list opens.
    pub settings_targets: Vec<SettingsTarget>,
}
