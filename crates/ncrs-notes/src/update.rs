//! What a message does to the notes tab.

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::widget::{operation, text_editor};
use iced::Task;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::markdown_view::parse_markdown;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{Message, Mode};
use crate::state::State;
use crate::store::{self, Kind, Note, Scan};
use crate::tree::RowKey;
use crate::view::{EDITOR_ID, SEARCH_ID};

/// How long typing pauses before the note is written.
const SAVE_PAUSE: Duration = Duration::from_millis(400);

/// Reads the notes folder on a blocking thread; the answer is
/// [`Message::Loaded`].
pub fn load(root: PathBuf) -> Task<Message> {
    let fallback = root.clone();
    Task::perform(
        async move { tokio::task::spawn_blocking(move || store::scan(&root)).await },
        move |joined| {
            let scan = joined.unwrap_or_else(|err| Err(format!("{}: {err}", fallback.display())));
            Message::Loaded(Box::new(scan))
        },
    )
}

/// Shows the tab and reads the folder again.
pub fn open(state: &mut State) -> Task<Message> {
    state.open = true;
    load(state.root.clone())
}

pub fn update(state: &mut State, message: Message, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    let nothing = (Task::none(), None);
    match message {
        Message::Reload => (load(state.root.clone()), None),
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
        Message::FolderMade(Ok(_)) | Message::Saved(Ok(())) => nothing,
        Message::FolderMade(Err(reason)) | Message::Saved(Err(reason)) => {
            (Task::none(), Some(failed(ctx, &reason)))
        }
        Message::SetMode(mode) => (set_mode(state, mode), None),
        Message::Edited(action) => (edited(state, action), None),
        Message::SaveDue(generation) if generation == state.generation => (flush(state), None),
        Message::SaveDue(_) => nothing,
        Message::LinkClicked(target) => (Task::none(), link(&target)),
        Message::ShowFile => (Task::none(), shown_file(state).map(Action::Reveal)),
        Message::LinksResolved(answer) => links_resolved(state, answer, ctx),
        Message::EditNoteOn(file) => edit_note_on(state, file),
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

fn written(content: PathBuf, text: String, replaces: Option<PathBuf>) -> Task<Message> {
    Task::perform(
        tokio::task::spawn_blocking(move || {
            store::write_note(&content, &text, replaces.as_deref())
        }),
        |joined| Message::Saved(joined.unwrap_or_else(|err| Err(err.to_string()))),
    )
}

/// The files of the notes on files, with the document each is known by, as
/// the question for the index.
fn resolve_all(state: &mut State) -> Option<Action> {
    let (files, asked): (Vec<_>, Vec<_>) = state
        .notes
        .iter()
        .filter_map(|note| match &note.kind {
            Kind::File { file, document } => Some(((*document, file.clone()), file.clone())),
            Kind::Topic => None,
        })
        .unzip();
    state.asked = asked;
    (!files.is_empty()).then_some(Action::ResolveLinks(files))
}

fn loaded(
    state: &mut State,
    scan: Result<Scan, String>,
    ctx: &Ctx,
) -> (Task<Message>, Option<Action>) {
    match scan {
        Ok(fresh) => {
            state.apply_scan(fresh);
            let task = match state.pending_file.take() {
                Some(file) => make_file_note_current(state, file),
                None => Task::none(),
            };
            (task, resolve_all(state))
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

/// Writes the shown note when it has changes. A note made here that takes its
/// title from its first line is renamed with the write.
fn flush(state: &mut State) -> Task<Message> {
    match pending_write(state) {
        Some((path, text, replaces)) => written(path, text, replaces),
        None => Task::none(),
    }
}

/// Writes the shown note now, on this thread, for the moment the app ends and
/// no task would run any more.
pub fn flush_now(state: &mut State) -> Result<(), String> {
    match pending_write(state) {
        Some((path, text, replaces)) => store::write_note(&path, &text, replaces.as_deref()),
        None => Ok(()),
    }
}

/// What a write of the shown note would write: the file, its text and the file
/// a rename leaves behind. `None` when there is nothing to write.
fn pending_write(state: &mut State) -> Option<(PathBuf, String, Option<PathBuf>)> {
    if !state.dirty {
        return None;
    }
    state.dirty = false;
    let old = state.current.clone()?;
    let new = renamed(state, &old).unwrap_or_else(|| old.clone());
    let text = state.note(&new)?.file_text();
    let replaces = (new != old).then_some(old);
    Some((new, text, replaces))
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
        path != old && (path.exists() || state.note(&path).is_some())
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
    let text = state.editor.text();
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
        on_disk: false,
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

fn link(target: &str) -> Option<Action> {
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
    {
        return Some(Action::OpenUrl(target.to_owned()));
    }
    // ponytail: only the space is decoded; other escapes stay as written.
    target
        .strip_prefix("file://")
        .map(|path| Action::Reveal(PathBuf::from(path.replace("%20", " "))))
}

fn shown_file(state: &State) -> Option<PathBuf> {
    match &state.shown()?.kind {
        Kind::File { file, .. } => Some(file.clone()),
        Kind::Topic => None,
    }
}

fn links_resolved(
    state: &mut State,
    answer: Result<Vec<(Option<i64>, PathBuf)>, String>,
    ctx: &Ctx,
) -> (Task<Message>, Option<Action>) {
    let resolved = match answer {
        Ok(resolved) => resolved,
        Err(reason) => return (Task::none(), Some(failed(ctx, &reason))),
    };
    let asked = std::mem::take(&mut state.asked);
    let typing = state.dirty.then(|| state.current.clone()).flatten();
    let mut writes = Vec::new();
    for (was, (id, path)) in asked.into_iter().zip(resolved) {
        let Some(note) = state
            .notes
            .iter_mut()
            .find(|note| matches!(&note.kind, Kind::File { file, .. } if *file == was))
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
            writes.push((note.path.clone(), note.file_text()));
        }
    }
    let tasks: Vec<_> = writes
        .into_iter()
        .map(|(path, text)| written(path, text, None))
        .collect();
    (Task::batch(tasks), None)
}

fn edit_note_on(state: &mut State, file: PathBuf) -> (Task<Message>, Option<Action>) {
    state.open = true;
    if !state.loaded {
        state.pending_file = Some(file);
        return (Task::none(), None);
    }
    let task = make_file_note_current(state, file.clone());
    let document = match state.note_on(&file).map(|note| &note.kind) {
        Some(Kind::File { document, .. }) => *document,
        _ => None,
    };
    state.asked = vec![file.clone()];
    (task, Some(Action::ResolveLinks(vec![(document, file)])))
}

/// Shows the note on `file` in the editor, making it when there is none.
fn make_file_note_current(state: &mut State, file: PathBuf) -> Task<Message> {
    let saved = flush(state);
    let (path, created) = match state.note_on(&file) {
        Some(note) => (note.path.clone(), false),
        None => {
            let path = store::file_note_path(&state.root, &file);
            let title = file
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            state.notes.push(Note {
                path: path.clone(),
                title,
                folder: PathBuf::new(),
                kind: Kind::File {
                    file,
                    document: None,
                },
                text: String::new(),
                modified: None,
                on_disk: false,
            });
            (path, true)
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
    Task::batch([saved, written, operation::focus(EDITOR_ID)])
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
