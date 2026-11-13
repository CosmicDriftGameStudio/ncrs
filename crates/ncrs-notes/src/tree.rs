//! The rows of the tree on the left: folders and notes, then the made-up
//! group of the notes on files.

use std::path::{Path, PathBuf};

use crate::state::State;
use crate::store::Kind;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowKey {
    /// A folder below the notes folder.
    Folder(PathBuf),
    /// The group of the notes on files.
    Files,
    /// A note, by the path of its file.
    Note(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: RowKey,
    pub depth: usize,
    /// `Some` for what can be folded.
    pub open: Option<bool>,
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn push_folder(state: &State, folder: &Path, depth: usize, rows: &mut Vec<Row>) {
    let mut children: Vec<&PathBuf> = state
        .folders
        .iter()
        .filter(|candidate| candidate.parent() == Some(folder))
        .collect();
    children.sort_by_key(|child| name_of(child));
    for child in children {
        let open = state.expanded.contains(child);
        rows.push(Row {
            key: RowKey::Folder(child.clone()),
            depth,
            open: Some(open),
        });
        if open {
            push_folder(state, child, depth + 1, rows);
        }
    }
    let mut notes: Vec<_> = state
        .notes
        .iter()
        .filter(|note| note.kind == Kind::Topic && note.folder == folder)
        .collect();
    notes.sort_by_key(|note| note.title.to_lowercase());
    rows.extend(notes.into_iter().map(|note| Row {
        key: RowKey::Note(note.path.clone()),
        depth,
        open: None,
    }));
}

/// The visible rows in order.
pub fn rows(state: &State) -> Vec<Row> {
    let mut rows = Vec::new();
    push_folder(state, Path::new(""), 0, &mut rows);
    let mut file_notes: Vec<_> = state
        .notes
        .iter()
        .filter(|note| matches!(note.kind, Kind::File { .. }))
        .collect();
    if !file_notes.is_empty() {
        rows.push(Row {
            key: RowKey::Files,
            depth: 0,
            open: Some(state.files_open),
        });
        if state.files_open {
            file_notes.sort_by_key(|note| match &note.kind {
                Kind::File { file, .. } => file.to_string_lossy().to_lowercase(),
                Kind::Topic => String::new(),
            });
            rows.extend(file_notes.into_iter().map(|note| Row {
                key: RowKey::Note(note.path.clone()),
                depth: 1,
                open: None,
            }));
        }
    }
    rows
}
