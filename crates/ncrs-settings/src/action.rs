use std::path::PathBuf;
use std::sync::Arc;

use ncrs_config::config::Config;
use ncrs_config::keymap::Keymap;
use ncrs_fs::OpenKind;

use crate::folders::AddAs;
use crate::message::{FavouritesEdit, SettingChange, StoreButton};

/// What the settings ask of the app, which they cannot do themselves. The app
/// runs the actions in order, right after `update`.
#[derive(Debug)]
pub enum Action {
    CloseContextMenu,
    SetDialogFocus(usize),
    ShowNotice(String),
    /// The system file dialog for the program behind F3 or F4.
    PickProgram {
        kind: OpenKind,
        apps_only: bool,
    },
    /// The same dialog for the program of a row of "F3 by file type"; the
    /// answer is `TypeRuleEdit::Picked`.
    PickRuleProgram {
        row: usize,
        apps_only: bool,
    },
    PickThemeFile,
    OpenServerAdd,
    OpenServer(PathBuf),
    /// The button at this position of the protection of the index.
    Protection(usize),
    /// The system folder dialog for a read folder.
    PickFolder(AddAs),
    /// The Repos tab scans its folders now.
    ScanRepos,
    /// A picked folder joins the favourites, at the end.
    AddFavourite(PathBuf),
    /// A move or removal on the Favourites page.
    EditFavourites(FavouritesEdit),
    /// Keeps the access to a picked folder for the next start (sandbox).
    GrantFolder {
        kind: AddAs,
        path: PathBuf,
    },
    TogglePause,
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
    /// The OCR models were just installed: the documents the index holds no
    /// text for are read again.
    ReadTextlessAgain,
    /// Switching Hermes on was not saved: the reason. Wherever the user asked
    /// from, the app tells them there.
    HermesNotTurnedOn(String),
    /// Show the recovery key of the notes once; the app sets it in `NotesPage`.
    NotesRecovery,
    /// A recovery key typed in on a computer that has none.
    NotesRestore(String),
    NotesSyncNow,
    /// A button of the Full version page.
    Store(StoreButton),
}
