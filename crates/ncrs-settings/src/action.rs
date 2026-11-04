use std::path::PathBuf;
use std::sync::Arc;

use ncrs_config::config::Config;
use ncrs_config::keymap::Keymap;
use ncrs_fs::OpenKind;

use crate::facts::AreaFacts;
use crate::folders::AddAs;
use crate::message::SettingChange;

/// What the settings ask of the app, which they cannot do themselves. The app
/// runs the actions in order, right after `update`.
#[derive(Debug)]
pub enum Action {
    CloseContextMenu,
    SetDialogFocus(usize),
    ShowNotice(String),
    /// The settings page opens in place of a page of Hermes: that page
    /// closes, and so do the context menu, the path field and the notice.
    LeaveAblagePage,
    /// The system file dialog for the program behind F3 or F4.
    PickProgram {
        kind: OpenKind,
        apps_only: bool,
    },
    PickThemeFile,
    ShowTagsPage,
    OpenServerAdd,
    OpenServer(PathBuf),
    OpenFilingStatus,
    /// The button at this position of the protection of the index.
    Protection(usize),
    /// The system folder dialog for a read folder.
    PickFolder(AddAs),
    /// Keeps the access to a picked folder for the next start (sandbox).
    GrantFolder {
        kind: AddAs,
        path: PathBuf,
    },
    /// Opens the chat (F2), at the change of this area when one is named.
    OpenChat {
        area: Option<AreaFacts>,
    },
    TogglePause,
    /// Puts the run of this journal batch back.
    UndoRun(i64),
    OpenRules,
    /// The file now says this about `change`.
    Apply {
        change: SettingChange,
        config: Box<Config>,
    },
    /// An installed theme is in effect.
    ApplyLook(Box<Config>),
    /// The keymap read back after an edit of the keys.
    KeymapChanged(Arc<Keymap>),
    RefreshAiModel,
}
