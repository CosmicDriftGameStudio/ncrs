//! The state of the notes tab.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};
use std::time::Instant;

use iced::widget::text_editor;
use ncrs_core::notes_sync::{NotesSyncDirection, NotesSyncWhen};
use ncrs_ui_kit::markdown_view::{parse_markdown, MarkdownItem};

use crate::crypt::{is_encrypted_path, NoteKey};
use crate::message::Mode;
use crate::search::{search_where, Hit};
use crate::store::{Expect, Kind, Ledger, Note, Replaced, Scan};
use crate::sync::SyncReport;
use crate::tree::{rows, Row, RowKey};
use crate::vault::Vault;

/// Everything the tab keeps. The notes are held in memory: the folder is read
/// once at start and when the tab opens, writes keep the list current.
#[derive(Debug, Default)]
pub struct State {
    pub root: PathBuf,
    pub notes: Vec<Note>,
    /// The files that have a note; `refresh_noted` rebuilds it after the notes changed.
    pub noted: HashSet<PathBuf>,
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
    /// The key of the encrypted notes and whether it is in memory.
    pub vault: Vault,
    /// What the settings say about encryption, sync and Hermes.
    pub options: Options,
    pub sync: SyncRun,
    /// The row whose context menu is open.
    pub menu: Option<RowKey>,
    /// The key is being fetched from the credential store.
    pub unlocking: bool,
    /// What was asked while the key had to be fetched first: the rows to seal
    /// (`true`) or unseal (`false`).
    pub pending_seal: Option<(Seal, bool)>,
}

/// What the encryption of notes is applied to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seal {
    /// A note or a folder of the tree.
    Row(RowKey),
    /// Every note of its own.
    Everything,
}

/// What the settings say about the notes tab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// New notes are written encrypted.
    pub encrypt_all: bool,
    /// Hermes may read and write notes.
    pub hermes_access: bool,
    pub sync_folder: Option<PathBuf>,
    pub direction: NotesSyncDirection,
    pub when: NotesSyncWhen,
}

/// The sync as it ran so far.
#[derive(Debug, Clone, Default)]
pub struct SyncRun {
    pub running: bool,
    /// The last run that ended without an error.
    pub last: Option<SyncReport>,
    /// Why the last run failed; the next success clears it.
    pub error: Option<String>,
    /// When the last run started, for the interval.
    pub started_at: Option<Instant>,
    /// A change was saved while a run was going: run again when it ends.
    pub again: bool,
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
    /// The notes folder: a link is only written through when it leads inside it.
    pub root: PathBuf,
    /// Made by the program, not by an edit: a refusal drops it instead of saving a copy.
    pub background: bool,
    /// Seals the text when the path is an encrypted note; `numbered` sets it.
    pub key: Option<Arc<NoteKey>>,
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

    /// Rebuilds `noted` after the notes changed.
    pub fn refresh_noted(&mut self) {
        self.noted = self
            .notes
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

    /// Writes are on their way; the notes in memory are ahead of the disk. Failed
    /// writes waiting for a retry do not count: a folder that stays read-only
    /// must not block reading it, `apply_scan` keeps those notes as they are.
    pub fn busy(&self) -> bool {
        self.running.is_some() || !self.queued.is_empty()
    }

    /// `write` numbered after all earlier ones. A failed write of the same file
    /// that waits for a retry gives way to it and hands over what it was based on.
    pub fn numbered(&mut self, mut write: Write) -> Write {
        self.write_counter += 1;
        write.generation = self.write_counter;
        write.key = self.vault.key();
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
        // A note that is being typed in, or whose write failed, stays as it is
        // in memory.
        let typing = self
            .current
            .clone()
            .filter(|_| self.dirty)
            .and_then(|path| self.note(&path).cloned());
        let unsaved: Vec<Note> = self
            .unsaved
            .iter()
            .filter_map(|write| self.note(&write.path).cloned())
            .collect();
        let left_behind: Vec<PathBuf> = self
            .unsaved
            .iter()
            .filter_map(|write| write.replaces.as_ref().map(|old| old.path.clone()))
            .collect();
        let kept_in_memory = |path: &Path| {
            typing.as_ref().is_some_and(|own| own.path == path)
                || unsaved.iter().any(|note| note.path == path)
        };
        {
            // What the disk holds is the new base, also of a file changed
            // outside. Not for the note being typed in: it keeps the base it
            // was typed on, so its next write sees the outside change.
            let mut written = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
            for note in &scan.notes {
                let kept = kept_in_memory(&note.path);
                if let (Some(hash), Some(last), false) =
                    (note.base, written.get_mut(&note.path), kept)
                {
                    last.hash = hash;
                }
            }
        }
        self.root = scan.root;
        self.notes = scan.notes;
        // The file a failed rename left behind is the note held in memory.
        self.notes.retain(|note| !left_behind.contains(&note.path));
        self.folders = scan.folders;
        self.loaded = true;
        self.problem = None;
        for note in unsaved {
            self.notes.retain(|other| other.path != note.path);
            self.notes.push(note);
        }
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

    /// An encrypted note while the key is not in memory.
    pub fn is_locked(&self, note: &Note) -> bool {
        is_encrypted_path(&note.path) && !self.vault.is_unlocked()
    }

    /// The shown note is encrypted and cannot be read now.
    pub fn shown_is_locked(&self) -> bool {
        self.shown().is_some_and(|note| self.is_locked(note))
    }

    /// Whether the folder holds notes and all of them, in its subfolders too,
    /// are encrypted.
    pub fn folder_is_sealed(&self, folder: &Path) -> bool {
        let mut inside = self
            .notes
            .iter()
            .filter(|note| note.kind == Kind::Topic && note.folder.starts_with(folder))
            .peekable();
        inside.peek().is_some() && inside.all(|note| is_encrypted_path(&note.path))
    }

    /// The notes Hermes may search, read and write: none unless the setting
    /// allows it, and the encrypted ones only while unlocked.
    pub fn notes_for_hermes(&self) -> impl Iterator<Item = &Note> {
        let allowed = self.options.hermes_access;
        self.notes
            .iter()
            .filter(move |note| allowed && !self.is_locked(note))
    }

    pub fn refresh_hits(&mut self) {
        self.hits = search_where(&self.notes, &self.query, |note| !self.is_locked(note));
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
