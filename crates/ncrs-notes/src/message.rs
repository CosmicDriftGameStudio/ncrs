//! What happens in the notes tab.

use std::path::PathBuf;
use std::time::Instant;

use iced::widget::text_editor;
use ncrs_core::NoteLink;

use crate::crypt::NoteKey;
use crate::state::Seal;
use crate::store::{Scan, WriteError};
use crate::sync::SyncReport;
use crate::tree::RowKey;

/// How the shown note is drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// Markdown formatted.
    #[default]
    View,
    /// An editor over the text.
    Edit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Read the notes folder again.
    Reload,
    /// The blocking read of the folder is done.
    Loaded(Box<Result<Scan, String>>),
    /// A row of the tree was pressed.
    Pressed(RowKey),
    /// A hit of the search was pressed.
    HitPressed(usize),
    SearchChanged(String),
    SearchSubmit,
    FocusSearch,
    NewNote,
    NewFolder,
    FolderMade(Result<PathBuf, String>),
    SetMode(Mode),
    Edited(text_editor::Action),
    /// The pause after the last edit is over: write the note.
    SaveDue(u64),
    /// A blocking write of this file is done: its number and the hash of what
    /// the file holds now, or why it was not written.
    Saved(PathBuf, u64, Result<u64, WriteError>),
    /// A link of the rendered note, as written there.
    LinkClicked(String),
    /// Show the file a note on a file is about.
    ShowFile,
    /// The paths asked about in `Action::ResolveLinks` and the answer, in the order asked.
    LinksResolved(Vec<PathBuf>, Result<Vec<NoteLink>, String>),
    /// Open the note on this file, make it when there is none, and edit it.
    EditNoteOn(PathBuf),
    Up,
    Down,
    /// ←: folds the folder under the cursor, else goes to the folder above.
    Left,
    /// →: unfolds the folder under the cursor.
    Right,
    Enter,
    /// Escape: leaves the editor, then the search, then the tab.
    Escape,
    /// The cross of the tab.
    Close,
    /// The clock of the app, a few times a minute: locks after the pause
    /// without input and starts the sync that runs every few minutes.
    Tick(Instant),
    /// "Unlock": fetch the key from the credential store.
    Unlock,
    /// ⌘L: locks an open vault, unlocks a locked one.
    ToggleLock,
    /// The key from the credential store, `None` when none was stored, and
    /// when it came.
    Unlocked(Result<Option<NoteKey>, String>, Instant),
    /// The person was not verified, so the key was not fetched.
    UnlockDenied(ncrs_user_verify::Denied),
    /// Drop the key from memory now.
    Lock,
    /// A right press on a row of the tree.
    OpenMenu(RowKey),
    CloseMenu,
    /// Encrypt (`true`) or decrypt (`false`) a note, a folder or every note.
    Seal(Seal, bool),
    /// The settings ask for the recovery key.
    RecoveryRequested,
    /// The recovery key as text, or why there is none.
    RecoveryReady(Result<Option<String>, String>),
    RecoveryDenied(ncrs_user_verify::Denied),
    /// A recovery key typed in on a computer that has no key.
    Restore(String),
    /// The key from a recovery text was stored, and when.
    Restored(Result<NoteKey, String>, Instant),
    /// Copy the notes to the sync folder now.
    SyncNow,
    /// A run of the sync ended.
    SyncDone(Result<SyncReport, String>),
}
