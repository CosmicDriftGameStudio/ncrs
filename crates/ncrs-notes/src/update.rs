//! What a message does to the notes tab.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iced::widget::{operation, text_editor};
use iced::Task;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::markdown_view::{classify, parse_markdown, Link};

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{Message, Mode};
use crate::state::{State, Write};
use crate::store::{self, Expect, Kind, Note, Replaced, Scan, WriteError};
use crate::tree::RowKey;
use crate::view::{EDITOR_ID, SEARCH_ID};

/// How long typing pauses before the note is written.
const SAVE_PAUSE: Duration = Duration::from_millis(400);

/// Reads the notes folder on a blocking thread; the answer is
/// [`Message::Loaded`].
fn load(root: PathBuf, epoch: u64) -> Task<Message> {
    let fallback = root.clone();
    Task::perform(
        async move { tokio::task::spawn_blocking(move || store::scan(&root)).await },
        move |joined| {
            let scan = joined
                .unwrap_or_else(|err| Err(format!("{}: {err}", fallback.display())))
                .map(|scan| Scan { epoch, ..scan });
            Message::Loaded(Box::new(scan))
        },
    )
}

/// Reads the notes folder again. While writes are on their way the notes in
/// memory are ahead of the disk, so the read waits for them.
pub fn reload(state: &mut State) -> Task<Message> {
    if state.busy() {
        state.reload_after_writes = true;
        return Task::none();
    }
    state.reload_after_writes = false;
    state.loading = true;
    load(state.root.clone(), state.write_counter)
}

/// Shows the tab and reads the folder again.
pub fn open(state: &mut State) -> Task<Message> {
    state.open = true;
    reload(state)
}

pub fn update(state: &mut State, message: Message, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    let nothing = (Task::none(), None);
    match message {
        Message::Reload => (reload(state), None),
        Message::Loaded(scan) => loaded(state, *scan, ctx),
        Message::Pressed(key) => (pressed(state, key), None),
        Message::HitPressed(index) => (open_hit(state, index), None),
        Message::SearchChanged(query) => {
            state.query = query;
            state.hit_cursor = 0;
            state.refresh_hits();
            nothing
        }
        Message::SearchSubmit | Message::Enter => (enter(state), None),
        Message::FocusSearch => (operation::focus(SEARCH_ID), None),
        Message::NewNote => (new_note(state, ctx), None),
        Message::NewFolder => (new_folder(state, ctx), None),
        Message::FolderMade(Ok(_)) => nothing,
        Message::FolderMade(Err(reason)) => (Task::none(), Some(failed(ctx, &reason))),
        Message::Saved(path, generation, result) => saved(state, &path, generation, result, ctx),
        Message::SetMode(mode) => (set_mode(state, mode), None),
        Message::Edited(action) => (edited(state, action), None),
        Message::SaveDue(generation) if generation == state.generation => (flush(state), None),
        Message::SaveDue(_) => nothing,
        Message::LinkClicked(target) => (Task::none(), link(state, &target)),
        Message::ShowFile => (Task::none(), shown_file(state).map(Action::Reveal)),
        Message::LinksResolved(asked, answer) => links_resolved(state, &asked, answer, ctx),
        Message::EditNoteOn(file) => edit_note_on(state, file, ctx),
        Message::Up => {
            step(state, -1);
            nothing
        }
        Message::Down => {
            step(state, 1);
            nothing
        }
        Message::Escape => escape(state),
        Message::Close => close(state),
    }
}

fn failed(ctx: &Ctx, reason: &str) -> Action {
    Action::Error(fill_template(
        ctx.lang.text(Msg::NotesWriteFailed),
        &[("{reason}", reason)],
    ))
}

/// Writes go one at a time, in the order they were asked for: a later write
/// of a note must not be overtaken by an earlier one, and a rename's removal of
/// the old file must come after the first write of it.
fn written(state: &mut State, write: Write) -> Task<Message> {
    if state.writing() {
        state.queue(write);
        return Task::none();
    }
    start_write(state, write)
}

fn start_write(state: &mut State, write: Write) -> Task<Message> {
    state.running = Some(write.clone());
    let ledger = Arc::clone(&state.ledger);
    let (path, generation) = (write.path.clone(), write.generation);
    Task::perform(
        tokio::task::spawn_blocking(move || store::write_note(&write, &ledger)),
        move |joined| {
            Message::Saved(
                path.clone(),
                generation,
                joined.unwrap_or_else(|err| Err(WriteError::Io(err.to_string()))),
            )
        },
    )
}

fn saved(
    state: &mut State,
    path: &Path,
    generation: u64,
    result: Result<u64, WriteError>,
    ctx: &Ctx,
) -> (Task<Message>, Option<Action>) {
    let write = state
        .running
        .take_if(|running| running.path == path && running.generation == generation);
    let mut action = None;
    match result {
        Ok(hash) => state.set_base(path, hash),
        Err(WriteError::Io(reason)) => {
            if let Some(write) = write.filter(|write| !write.background) {
                state.keep_unsaved(write);
            }
            action = Some(failed(ctx, &reason));
        }
        Err(WriteError::Conflict(_)) => {
            if let Some(write) = write {
                let (retry, copy) = resolve_conflict(state, &write, ctx);
                if let Some(retry) = retry {
                    state.queue(retry);
                }
                action = copy.map(|name| conflicted(ctx, &name));
            }
        }
    }
    let next = match state.queued.pop_front() {
        Some(write) => start_write(state, write),
        None => Task::none(),
    };
    let reread = if state.reload_after_writes && !state.busy() {
        reload(state)
    } else {
        Task::none()
    };
    (Task::batch([next, reread]), action)
}

fn conflicted(ctx: &Ctx, copy: &str) -> Action {
    Action::Error(fill_template(
        ctx.lang.text(Msg::NotesConflict),
        &[("{name}", copy)],
    ))
}

/// A write was refused because the disk is not what it was based on. A new
/// note whose name was taken meanwhile takes a free name. An edit over a file
/// that changed outside is saved as a copy next to it, named after the note;
/// the copy's name is the second answer. The folder is read again so the
/// outside version shows up. A write the program made on its own is dropped.
fn resolve_conflict(
    state: &mut State,
    write: &Write,
    ctx: &Ctx,
) -> (Option<Write>, Option<String>) {
    state.reload_after_writes = true;
    if write.background {
        return (None, None);
    }
    let Some(note) = state.note(&write.path).cloned() else {
        return (None, None);
    };
    let name_was_taken = write.expect == Expect::Absent && note.kind == Kind::Topic;
    let (folder, base_title) = if name_was_taken {
        (note.folder.clone(), note.title.clone())
    } else {
        let suffix = ctx.lang.text(Msg::NotesConflictSuffix);
        let folder = match note.kind {
            Kind::Topic => note.folder.clone(),
            Kind::File { .. } => PathBuf::new(),
        };
        (folder, format!("{} ({suffix})", note.title))
    };
    let title = store::unique_title(&base_title, |candidate| {
        let path = store::topic_path(&state.root, &folder, candidate);
        store::is_taken(&path) || state.note(&path).is_some()
    });
    let path = store::topic_path(&state.root, &folder, &title);
    let copy_name = (!name_was_taken).then(|| title.clone());
    if let Some(moved) = state.note_mut(&write.path) {
        moved.path = path.clone();
        moved.title = title;
        moved.folder = folder;
        moved.kind = Kind::Topic;
        moved.base = None;
    }
    if state.current.as_ref() == Some(&write.path) {
        state.current = Some(path.clone());
        if !name_was_taken {
            state.auto_title = false;
        }
    }
    if state.cursor == Some(RowKey::Note(write.path.clone())) {
        state.cursor = Some(RowKey::Note(path.clone()));
    }
    let text = if name_was_taken {
        write.text.clone()
    } else {
        state.note(&path).map(Note::file_text).unwrap_or_default()
    };
    let retry = state.numbered(Write {
        path,
        text,
        expect: Expect::Absent,
        replaces: name_was_taken.then(|| write.replaces.clone()).flatten(),
        generation: 0,
        root: state.root.clone(),
        background: false,
    });
    (Some(retry), copy_name)
}

/// The files of the notes on files, with the document each is known by, as
/// the question for the index.
fn resolve_all(state: &State) -> Option<Action> {
    let files: Vec<_> = state
        .notes
        .iter()
        .filter_map(|note| match &note.kind {
            Kind::File { file, document } => Some((*document, file.clone())),
            Kind::Topic => None,
        })
        .collect();
    (!files.is_empty()).then_some(Action::ResolveLinks(files))
}

fn loaded(
    state: &mut State,
    scan: Result<Scan, String>,
    ctx: &Ctx,
) -> (Task<Message>, Option<Action>) {
    state.loading = false;
    let stale = scan
        .as_ref()
        .is_ok_and(|fresh| fresh.epoch != state.write_counter);
    if state.busy() || stale {
        state.reload_after_writes = true;
        return (reload(state), None);
    }
    match scan {
        Ok(fresh) => {
            state.apply_scan(fresh);
            let (task, problem) = match state.pending_file.take() {
                Some(file) => match make_file_note_current(state, file, ctx) {
                    Ok(task) => (task, None),
                    Err(problem) => (Task::none(), Some(problem)),
                },
                None => (Task::none(), None),
            };
            (task, problem.or_else(|| resolve_all(state)))
        }
        Err(reason) => {
            state.loaded = true;
            state.problem = Some(fill_template(
                ctx.lang.text(Msg::NotesProblem),
                &[
                    ("{path}", &state.root.to_string_lossy()),
                    ("{reason}", &reason),
                ],
            ));
            (Task::none(), None)
        }
    }
}

/// Writes the shown note when it has changes, and tries again what failed
/// earlier. A note made here that takes its title from its first line is
/// renamed with the write.
fn flush(state: &mut State) -> Task<Message> {
    let mut tasks = Vec::new();
    if let Some(write) = pending_write(state) {
        tasks.push(written(state, write));
    }
    for write in std::mem::take(&mut state.unsaved) {
        tasks.push(written(state, write));
    }
    Task::batch(tasks)
}

/// Writes everything now, on this thread, for the moment the app ends and no
/// task would run any more. Returns why writes failed; those stay for the next try.
pub fn flush_now(state: &mut State, ctx: &Ctx) -> Vec<String> {
    let pending = pending_write(state);
    let mut todo: VecDeque<Write> = state
        .running
        .take()
        .into_iter()
        .chain(state.queued.drain(..))
        .chain(state.unsaved.drain(..))
        .chain(pending)
        .collect();
    let mut failures = Vec::new();
    while let Some(write) = todo.pop_front() {
        match store::write_note(&write, &state.ledger) {
            Ok(hash) => state.set_base(&write.path, hash),
            Err(WriteError::Io(reason)) => {
                failures.push(reason);
                if !write.background {
                    state.keep_unsaved(write);
                }
            }
            Err(WriteError::Conflict(_)) => todo.extend(resolve_conflict(state, &write, ctx).0),
        }
    }
    if failures.is_empty() {
        state.quit_warned = false;
    }
    failures
}

/// What a write of the shown note would write: the file, its text and the file
/// a rename leaves behind. `None` when there is nothing to write.
fn pending_write(state: &mut State) -> Option<Write> {
    if !state.dirty {
        return None;
    }
    let Some(old) = state.current.clone() else {
        state.dirty = false;
        return None;
    };
    let write = planned_write(state, &old)?;
    state.dirty = false;
    Some(write)
}

fn planned_write(state: &mut State, old: &Path) -> Option<Write> {
    let base = state.note(old)?.base;
    let new = renamed(state, old).unwrap_or_else(|| old.to_owned());
    let text = state.note(&new)?.file_text();
    let (expect, replaces) = if new == old {
        (Expect::of(base), None)
    } else {
        // A case-only rename leads to the file itself, which is not absent.
        let own_file = store::is_same_file(&new, old);
        let replaced = Replaced {
            path: old.to_owned(),
            base,
        };
        let expect = if own_file {
            Expect::of(base)
        } else {
            Expect::Absent
        };
        (expect, Some(replaced))
    };
    Some(state.numbered(Write {
        path: new,
        text,
        expect,
        replaces,
        generation: 0,
        root: state.root.clone(),
        background: false,
    }))
}

/// The new path of the note at `old` when its first line gives it a new title.
fn renamed(state: &mut State, old: &Path) -> Option<PathBuf> {
    if !state.auto_title {
        return None;
    }
    let note = state.note(old)?;
    if note.kind != Kind::Topic {
        return None;
    }
    let wanted = store::clean_title(note.first_line()?);
    if wanted.is_empty() || wanted == note.title {
        return None;
    }
    let folder = note.folder.clone();
    let title = store::unique_title(&wanted, |candidate| {
        let path = store::topic_path(&state.root, &folder, candidate);
        path != old
            && !store::is_same_file(&path, old)
            && (path.exists() || state.note(&path).is_some())
    });
    let path = store::topic_path(&state.root, &folder, &title);
    let renamed = state.note_mut(old)?;
    renamed.path = path.clone();
    renamed.title = title;
    state.current = Some(path.clone());
    state.cursor = Some(RowKey::Note(path.clone()));
    Some(path)
}

fn select(state: &mut State, path: PathBuf) -> Task<Message> {
    let saved = flush(state);
    state.auto_title = false;
    state.mode = Mode::View;
    state.cursor = Some(RowKey::Note(path.clone()));
    state.current = Some(path);
    state.refresh_shown();
    saved
}

fn reveal_in_tree(state: &mut State, path: &Path) {
    let Some(note) = state.note(path) else {
        return;
    };
    match note.kind {
        Kind::File { .. } => state.files_open = true,
        Kind::Topic => {
            let folder = note.folder.clone();
            state
                .expanded
                .extend(folder.ancestors().map(Path::to_owned));
        }
    }
}

fn pressed(state: &mut State, key: RowKey) -> Task<Message> {
    state.cursor = Some(key.clone());
    match key {
        RowKey::Folder(folder) => {
            if !state.expanded.remove(&folder) {
                state.expanded.insert(folder);
            }
            Task::none()
        }
        RowKey::Files => {
            state.files_open = !state.files_open;
            Task::none()
        }
        RowKey::Note(path) => select(state, path),
    }
}

fn open_hit(state: &mut State, index: usize) -> Task<Message> {
    let Some(hit) = state.hits.get(index) else {
        return Task::none();
    };
    let path = hit.note.clone();
    state.query.clear();
    state.hits.clear();
    reveal_in_tree(state, &path);
    select(state, path)
}

fn enter(state: &mut State) -> Task<Message> {
    if state.searching() {
        return open_hit(state, state.hit_cursor);
    }
    match state.cursor.clone() {
        Some(RowKey::Note(path)) if state.current.as_ref() == Some(&path) => {
            set_mode(state, Mode::Edit)
        }
        Some(key) => pressed(state, key),
        None => Task::none(),
    }
}

fn step(state: &mut State, by: isize) {
    if state.searching() {
        let last = state.hits.len().saturating_sub(1);
        state.hit_cursor = state.hit_cursor.saturating_add_signed(by).min(last);
        return;
    }
    let rows = state.rows();
    let position = state
        .cursor
        .as_ref()
        .and_then(|cursor| rows.iter().position(|row| &row.key == cursor));
    let next = match position {
        Some(index) => index
            .saturating_add_signed(by)
            .min(rows.len().saturating_sub(1)),
        None => 0,
    };
    if let Some(row) = rows.get(next) {
        state.cursor = Some(row.key.clone());
    }
}

fn set_mode(state: &mut State, mode: Mode) -> Task<Message> {
    if state.current.is_none() {
        return Task::none();
    }
    state.mode = mode;
    match mode {
        Mode::Edit => operation::focus(EDITOR_ID),
        Mode::View => {
            let task = flush(state);
            if let Some(note) = state.shown() {
                state.items = parse_markdown(&note.text);
            }
            task
        }
    }
}

fn edited(state: &mut State, action: text_editor::Action) -> Task<Message> {
    if state.mode != Mode::Edit {
        return Task::none();
    }
    let is_edit = action.is_edit();
    state.editor.perform(action);
    if !is_edit {
        return Task::none();
    }
    state.quit_warned = false;
    let text = state.editor.text();
    state.items = parse_markdown(&text);
    let Some(note) = state.current.clone().and_then(|path| state.note_mut(&path)) else {
        return Task::none();
    };
    note.text = text;
    state.dirty = true;
    state.generation += 1;
    let generation = state.generation;
    Task::perform(tokio::time::sleep(SAVE_PAUSE), move |()| {
        Message::SaveDue(generation)
    })
}

fn new_note(state: &mut State, ctx: &Ctx) -> Task<Message> {
    let saved = flush(state);
    let folder = state.target_folder();
    let base = ctx.lang.text(Msg::NotesNewNote);
    let title = store::unique_title(base, |candidate| {
        let path = store::topic_path(&state.root, &folder, candidate);
        path.exists() || state.note(&path).is_some()
    });
    let path = store::topic_path(&state.root, &folder, &title);
    state
        .expanded
        .extend(folder.ancestors().map(Path::to_owned));
    state.notes.push(Note {
        path: path.clone(),
        title,
        folder,
        kind: Kind::Topic,
        text: String::new(),
        modified: None,
        base: None,
    });
    state.query.clear();
    state.hits.clear();
    state.cursor = Some(RowKey::Note(path.clone()));
    state.current = Some(path);
    state.refresh_shown();
    state.mode = Mode::Edit;
    state.auto_title = true;
    // Written at once, so the note exists on disk before anything is typed.
    state.dirty = true;
    let created = flush(state);
    Task::batch([saved, created, operation::focus(EDITOR_ID)])
}

fn new_folder(state: &mut State, ctx: &Ctx) -> Task<Message> {
    let parent = state.target_folder();
    let base = ctx.lang.text(Msg::NotesNewFolder);
    let title = store::unique_title(base, |candidate| {
        let below = parent.join(candidate);
        state.folders.contains(&below) || state.root.join(&below).exists()
    });
    let below = parent.join(title);
    state
        .expanded
        .extend(parent.ancestors().map(Path::to_owned));
    state.folders.push(below.clone());
    state.cursor = Some(RowKey::Folder(below.clone()));
    let absolute = state.root.join(&below);
    Task::perform(
        tokio::task::spawn_blocking(move || store::make_folder(&absolute).map(|()| below)),
        |joined| Message::FolderMade(joined.unwrap_or_else(|err| Err(err.to_string()))),
    )
}

fn link(state: &State, target: &str) -> Option<Action> {
    let base = state
        .shown()
        .and_then(|note| note.path.parent())
        .unwrap_or(Path::new(""));
    match classify(target, base) {
        Link::Ignore => None,
        Link::Url(url) => Some(Action::OpenUrl(url)),
        Link::Markdown(file) | Link::File(file) => Some(Action::Reveal(file)),
    }
}

fn shown_file(state: &State) -> Option<PathBuf> {
    match &state.shown()?.kind {
        Kind::File { file, .. } => Some(file.clone()),
        Kind::Topic => None,
    }
}

fn links_resolved(
    state: &mut State,
    asked: &[PathBuf],
    answer: Result<Vec<(Option<i64>, PathBuf)>, String>,
    ctx: &Ctx,
) -> (Task<Message>, Option<Action>) {
    let resolved = match answer {
        Ok(resolved) => resolved,
        Err(reason) => return (Task::none(), Some(failed(ctx, &reason))),
    };
    let typing = state.dirty.then(|| state.current.clone()).flatten();
    let mut writes = Vec::new();
    for (was, (id, path)) in asked.iter().zip(resolved) {
        let Some(note) = state
            .notes
            .iter_mut()
            .find(|note| matches!(&note.kind, Kind::File { file, .. } if file == was))
        else {
            continue;
        };
        let Kind::File { file, document } = &mut note.kind else {
            continue;
        };
        if *file == path && (id.is_none() || *document == id) {
            continue;
        }
        *file = path;
        *document = id.or(*document);
        // A note being typed in is written with its next flush.
        if typing.as_ref() != Some(&note.path) {
            writes.push((note.path.clone(), note.file_text(), Expect::of(note.base)));
        }
    }
    let tasks: Vec<_> = writes
        .into_iter()
        .map(|(path, text, expect)| {
            let write = state.numbered(Write {
                path,
                text,
                expect,
                replaces: None,
                generation: 0,
                root: state.root.clone(),
                background: true,
            });
            written(state, write)
        })
        .collect();
    (Task::batch(tasks), None)
}

fn edit_note_on(state: &mut State, file: PathBuf, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    state.open = true;
    if !state.loaded {
        state.pending_file = Some(file);
        return (Task::none(), None);
    }
    let task = match make_file_note_current(state, file.clone(), ctx) {
        Ok(task) => task,
        Err(problem) => return (Task::none(), Some(problem)),
    };
    let document = match state.note_on(&file).map(|note| &note.kind) {
        Some(Kind::File { document, .. }) => *document,
        _ => None,
    };
    (task, Some(Action::ResolveLinks(vec![(document, file)])))
}

/// Shows the note on `file` in the editor, making it when there is none. A
/// file where the note would go that is not a note this program can read is
/// left alone and reported.
fn make_file_note_current(
    state: &mut State,
    file: PathBuf,
    ctx: &Ctx,
) -> Result<Task<Message>, Action> {
    let new_path = store::file_note_path(&state.root, &file);
    if state.note_on(&file).is_none() && store::is_taken(&new_path) {
        return Err(Action::Error(fill_template(
            ctx.lang.text(Msg::NotesFileNoteUnreadable),
            &[("{path}", &new_path.to_string_lossy())],
        )));
    }
    let saved = flush(state);
    let (path, created) = match state.note_on(&file) {
        Some(note) => (note.path.clone(), false),
        None => {
            let title = file
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            state.notes.push(Note {
                path: new_path.clone(),
                title,
                folder: PathBuf::new(),
                kind: Kind::File {
                    file,
                    document: None,
                },
                text: String::new(),
                modified: None,
                base: None,
            });
            (new_path, true)
        }
    };
    state.query.clear();
    state.hits.clear();
    state.files_open = true;
    state.cursor = Some(RowKey::Note(path.clone()));
    state.current = Some(path);
    state.refresh_shown();
    state.mode = Mode::Edit;
    state.auto_title = false;
    state.dirty = created;
    let written = flush(state);
    Ok(Task::batch([saved, written, operation::focus(EDITOR_ID)]))
}

fn escape(state: &mut State) -> (Task<Message>, Option<Action>) {
    if state.editing() {
        return (set_mode(state, Mode::View), None);
    }
    if state.searching() {
        state.query.clear();
        state.hits.clear();
        return (Task::none(), None);
    }
    close(state)
}

fn close(state: &mut State) -> (Task<Message>, Option<Action>) {
    let task = flush(state);
    state.open = false;
    state.mode = Mode::View;
    (task, Some(Action::Close))
}
