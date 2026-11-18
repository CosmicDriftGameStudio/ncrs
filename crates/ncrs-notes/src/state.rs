//! The state of the notes tab.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use iced::widget::text_editor;
use ncrs_ui_kit::markdown_view::{parse_markdown, MarkdownItem};

use crate::message::Mode;
use crate::search::{search, Hit};
use crate::store::{Expect, Kind, Ledger, Note, Replaced, Scan};
use crate::tree::{rows, Row, RowKey};

/// Everything the tab keeps. The notes are held in memory: the folder is read
/// once at start and when the tab opens, writes keep the list current.
#[derive(Debug, Default)]
pub struct State {
    pub root: PathBuf,
    pub notes: Vec<Note>,
    pub folders: Vec<PathBuf>,
    pub expanded: HashSet<PathBuf>,
    pub files_open: bool,
    /// The row under the cursor in the tree.
    pub cursor: Option<RowKey>,
    /// The note on the right, by its file.
    pub current: Option<PathBuf>,
    pub mode: Mode,
    pub editor: text_editor::Content,
    pub items: Vec<MarkdownItem>,
    pub query: String,
    pub hits: Vec<Hit>,
    pub hit_cursor: usize,
    /// Set once the folder has been read.
    pub loaded: bool,
    /// Shown in place of the tab while the folder cannot be used.
    pub problem: Option<String>,
    /// Counts the edits; a write waits for the pause after the last one.
    pub generation: u64,
    /// The shown note has changes that are not written.
    pub dirty: bool,
    /// The shown note was made here and takes its title from its first line.
    pub auto_title: bool,
    /// A note on this file is to be shown as soon as the folder is read.
    pub pending_file: Option<PathBuf>,
    pub open: bool,
    /// The write running on a blocking thread.
    pub running: Option<Write>,
    /// Writes asked for while one was running, oldest first.
    pub queued: VecDeque<Write>,
    /// Writes that failed and wait for the next try, at most one per file.
    pub unsaved: Vec<Write>,
    /// The folder is being read.
    pub loading: bool,
    /// A read of the folder was thrown away because a write was in the way.
    pub reload_after_writes: bool,
    /// A quit was held back because a note could not be saved.
    pub quit_warned: bool,
    /// Numbers the writes in the order they were asked for.
    pub write_counter: u64,
    /// The newest write done per file, shared with the blocking tasks.
    pub ledger: Ledger,
}

/// One note file to write: what the disk must look like for it to go ahead,
/// and the file a rename leaves behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    pub path: PathBuf,
    pub text: String,
    pub expect: Expect,
    pub replaces: Option<Replaced>,
    pub generation: u64,
    /// Made by the program, not by an edit: a refusal drops it instead of saving a copy.
    pub background: bool,
}

impl State {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            files_open: true,
            ..Self::default()
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    pub fn editing(&self) -> bool {
        self.open && self.mode == Mode::Edit && self.current.is_some()
    }

    pub fn rows(&self) -> Vec<Row> {
        rows(self)
    }

    pub fn note(&self, path: &Path) -> Option<&Note> {
        self.notes.iter().find(|note| note.path == path)
    }

    pub fn note_mut(&mut self, path: &Path) -> Option<&mut Note> {
        self.notes.iter_mut().find(|note| note.path == path)
    }

    pub fn shown(&self) -> Option<&Note> {
        self.note(self.current.as_deref()?)
    }

    /// The note on `file`, if there is one.
    pub fn note_on(&self, file: &Path) -> Option<&Note> {
        self.notes
            .iter()
            .find(|note| matches!(&note.kind, Kind::File { file: of, .. } if of == file))
    }

    /// The files that have a note.
    pub fn noted_files(&self) -> HashSet<PathBuf> {
        self.notes
            .iter()
            .filter_map(|note| match &note.kind {
                Kind::File { file, .. } => Some(file.clone()),
                Kind::Topic => None,
            })
            .collect()
    }

    pub fn writing(&self) -> bool {
        self.running.is_some()
    }

    /// Writes are on their way or waiting for a retry; the notes in memory are
    /// ahead of the disk.
    pub fn busy(&self) -> bool {
        self.running.is_some() || !self.queued.is_empty() || !self.unsaved.is_empty()
    }

    /// `write` numbered after all earlier ones. A failed write of the same file
    /// that waits for a retry gives way to it and hands over what it was based on.
    pub fn numbered(&mut self, mut write: Write) -> Write {
        self.write_counter += 1;
        write.generation = self.write_counter;
        if let Some(at) = self.unsaved.iter().position(|old| old.path == write.path) {
            let old = self.unsaved.remove(at);
            write.expect = old.expect;
            write.replaces = old.replaces.or(write.replaces);
            write.background &= old.background;
        }
        write
    }

    /// Keeps a failed write for the next try.
    pub fn keep_unsaved(&mut self, write: Write) {
        self.unsaved.retain(|old| old.path != write.path);
        self.unsaved.push(write);
    }

    /// What the file of the note holds now, after a write.
    pub fn set_base(&mut self, path: &Path, hash: u64) {
        if let Some(note) = self.note_mut(path) {
            note.base = Some(hash);
        }
    }

    /// Keeps `write` behind the running one; a queued write of the same note
    /// gives way to the newer text and hands over the file it removes.
    pub fn queue(&mut self, write: Write) {
        match self
            .queued
            .iter_mut()
            .find(|older| older.path == write.path)
        {
            Some(older) => {
                older.text = write.text;
                older.generation = write.generation;
                older.replaces = older.replaces.take().or(write.replaces);
                older.background &= write.background;
            }
            None => self.queued.push_back(write),
        }
    }

    pub fn apply_scan(&mut self, scan: Scan) {
        // A note that is being typed in stays as it is in memory.
        let typing = self
            .current
            .clone()
            .filter(|_| self.dirty)
            .and_then(|path| self.note(&path).cloned());
        {
            // What the disk holds is the new base, also of a file changed
            // outside. Not for the note being typed in: it keeps the base it
            // was typed on, so its next write sees the outside change.
            let mut written = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
            for note in &scan.notes {
                let kept = typing.as_ref().is_some_and(|own| own.path == note.path);
                if let (Some(hash), Some(last), false) =
                    (note.base, written.get_mut(&note.path), kept)
                {
                    last.hash = hash;
                }
            }
        }
        self.root = scan.root;
        self.notes = scan.notes;
        self.folders = scan.folders;
        self.loaded = true;
        self.problem = None;
        if let Some(note) = typing {
            self.notes.retain(|other| other.path != note.path);
            self.notes.push(note);
        } else {
            let shown = self.current.take().filter(|path| self.note(path).is_some());
            self.current = shown;
            self.refresh_shown();
        }
        self.refresh_hits();
    }

    /// Loads the editor and the rendered text from the shown note.
    pub fn refresh_shown(&mut self) {
        let text = self.shown().map(|note| note.text.clone());
        self.editor = text_editor::Content::with_text(text.as_deref().unwrap_or(""));
        self.items = parse_markdown(text.as_deref().unwrap_or(""));
    }

    pub fn refresh_hits(&mut self) {
        self.hits = search(&self.notes, &self.query);
        self.hit_cursor = self.hit_cursor.min(self.hits.len().saturating_sub(1));
    }

    /// The folder new notes and folders go into: the folder of the cursor.
    pub fn target_folder(&self) -> PathBuf {
        match &self.cursor {
            Some(RowKey::Folder(folder)) => folder.clone(),
            Some(RowKey::Note(path)) => self
                .note(path)
                .filter(|note| note.kind == Kind::Topic)
                .map(|note| note.folder.clone())
                .unwrap_or_default(),
            Some(RowKey::Files) | None => PathBuf::new(),
        }
    }
}
