//! What happens in the notes tab.

use std::path::PathBuf;

use iced::widget::text_editor;

use crate::store::Scan;
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
    /// A blocking write is done.
    Saved(Result<(), String>),
    /// A link of the rendered note, as written there.
    LinkClicked(String),
    /// Show the file a note on a file is about.
    ShowFile,
    /// The paths asked about in `Action::ResolveLinks` and the answer, in the order asked.
    LinksResolved(Vec<PathBuf>, Result<Vec<(Option<i64>, PathBuf)>, String>),
    /// Open the note on this file, make it when there is none, and edit it.
    EditNoteOn(PathBuf),
    Up,
    Down,
    Enter,
    /// Escape: leaves the editor, then the search, then the tab.
    Escape,
    /// The cross of the tab.
    Close,
}
