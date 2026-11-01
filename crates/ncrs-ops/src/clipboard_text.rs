//! What the copy chords put on the clipboard as text.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use ncrs_core::{FileEntry, SelectionSet};
use ncrs_i18n::{Language, Msg};

use crate::transfer_flow::child_path;

/// What the copy chords put on the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardKind {
    Path,
    Name,
    /// The directory the active panel shows, not a row in it.
    Directory,
}

/// What goes on the clipboard, and for how many rows.
#[derive(Debug, PartialEq, Eq)]
pub struct ClipboardText {
    pub text: String,
    pub count: usize,
}

/// The rows of a panel the clipboard text is made from.
#[derive(Debug, Clone, Copy)]
pub struct PanelRows<'a> {
    pub path: &'a Path,
    pub entries: &'a [FileEntry],
    pub selection: &'a SelectionSet,
    pub cursor: Option<&'a FileEntry>,
}

/// The path or the name of each of `files`, one per line, for the results of
/// the search, which are not rows of a panel.
pub fn clipboard_text_of_files(files: &[PathBuf], kind: ClipboardKind) -> Option<ClipboardText> {
    let lines: Vec<String> = files
        .iter()
        .map(|path| match kind {
            ClipboardKind::Name => path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned(),
            ClipboardKind::Path | ClipboardKind::Directory => path.to_string_lossy().into_owned(),
        })
        .collect();
    (!lines.is_empty()).then(|| ClipboardText {
        count: lines.len(),
        text: lines.join("\n"),
    })
}

/// The tagged rows, else the cursor row, one per line in listing order. On `..`
/// without tags the directory of the panel itself; the root has no file name,
/// so its path is its name. Paths are joined, not canonicalised.
pub fn clipboard_text(panel: PanelRows<'_>, kind: ClipboardKind) -> Option<ClipboardText> {
    if kind == ClipboardKind::Directory {
        return Some(ClipboardText {
            count: 1,
            text: panel.path.to_string_lossy().into_owned(),
        });
    }
    let child = |name: &OsStr| child_path(panel.path, name);
    let render = |path: &Path, name: &OsStr| match kind {
        ClipboardKind::Name => name.to_string_lossy().into_owned(),
        ClipboardKind::Path | ClipboardKind::Directory => path.to_string_lossy().into_owned(),
    };
    let lines: Vec<String> = if panel.selection.any_tagged() {
        panel
            .selection
            .ordered(panel.entries)
            .into_iter()
            .map(|entry| render(&child(&entry.name), &entry.name))
            .collect()
    } else {
        let entry = panel.cursor?;
        if entry.is_parent {
            let own_name = panel.path.file_name().unwrap_or(panel.path.as_os_str());
            vec![render(panel.path, own_name)]
        } else {
            vec![render(&child(&entry.name), &entry.name)]
        }
    };
    (!lines.is_empty()).then(|| ClipboardText {
        count: lines.len(),
        text: lines.join("\n"),
    })
}

/// The status line after `count` paths or names went on the clipboard.
pub fn copied_notice(lang: Language, kind: ClipboardKind, count: usize) -> String {
    let (one, many) = match kind {
        ClipboardKind::Path | ClipboardKind::Directory => {
            (Msg::StatusPathCopied, Msg::StatusPathsCopied)
        }
        ClipboardKind::Name => (Msg::StatusNameCopied, Msg::StatusNamesCopied),
    };
    if count == 1 {
        lang.text(one).to_string()
    } else {
        lang.text(many).replace("{count}", &count.to_string())
    }
}
