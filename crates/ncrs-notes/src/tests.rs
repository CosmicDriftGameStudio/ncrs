// reason: a failing assertion is the signal in a test
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::fs;
use std::path::{Path, PathBuf};

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_i18n::Language;

use crate::frontmatter::{compose, split, FrontMatter};
use crate::search::search;
use crate::store::{self, file_note_path, scan, Kind, FILES_DIR};
use crate::tree::RowKey;
use crate::{notes_key, update, Action, Ctx, Message, Mode, State};

const CTX: Ctx = Ctx {
    lang: Language::English,
};

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn loaded(root: &Path) -> State {
    let mut state = State::new(root.to_owned());
    let _ = update(&mut state, Message::Loaded(Box::new(scan(root))), &CTX);
    state
}

fn action(state: &mut State, message: Message) -> Option<Action> {
    update(state, message, &CTX).1
}

#[test]
fn the_scan_reads_topics_and_file_notes_and_hides_dot_entries() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "Haus/Heizung.md", "# Heizung\n\nWartung\n");
    write(dir.path(), "Reisen.md", "Italien");
    write(dir.path(), ".hidden/Secret.md", "no");
    write(dir.path(), "readme.txt", "not a note");
    let file = Path::new("/docs/Angebot.pdf");
    let path = file_note_path(dir.path(), file);
    write(
        dir.path(),
        &format!(
            "{FILES_DIR}/{}",
            path.file_name().unwrap().to_string_lossy()
        ),
        "---\nfile: /docs/Angebot.pdf\ndocument: 7\n---\nGilt bis Dezember\n",
    );
    let found = scan(dir.path()).unwrap();
    let mut titles: Vec<_> = found.notes.iter().map(|note| note.title.as_str()).collect();
    titles.sort_unstable();
    assert_eq!(titles, ["Angebot.pdf", "Heizung", "Reisen"]);
    assert_eq!(found.folders, [PathBuf::from("Haus")]);
    let on_file = found
        .notes
        .iter()
        .find(|note| matches!(note.kind, Kind::File { .. }))
        .unwrap();
    assert_eq!(
        on_file.kind,
        Kind::File {
            file: file.to_owned(),
            document: Some(7)
        }
    );
    assert_eq!(on_file.text, "Gilt bis Dezember\n");
}

#[test]
fn a_missing_folder_is_an_empty_one() {
    let dir = tempfile::tempdir().unwrap();
    let found = scan(&dir.path().join("Notes")).unwrap();
    assert!(found.notes.is_empty());
}

#[test]
fn front_matter_round_trips_and_text_without_it_stays_whole() {
    let front = FrontMatter {
        file: PathBuf::from("/a/b c.pdf"),
        document: Some(12),
    };
    let text = compose(&front, "Body\n");
    let (read, body) = split(&text);
    assert_eq!(read, Some(front));
    assert_eq!(body, "Body\n");
    assert_eq!(split("# Title\n").0, None);
}

#[test]
fn the_name_of_a_file_note_is_stable_and_differs_by_path() {
    let root = Path::new("/n");
    let a = file_note_path(root, Path::new("/x/a.pdf"));
    assert_eq!(a, file_note_path(root, Path::new("/x/a.pdf")));
    assert_ne!(a, file_note_path(root, Path::new("/y/a.pdf")));
    assert!(a.starts_with("/n/.files"));
    assert_eq!(store::unique_title("Note", |name| name == "Note"), "Note 2");
}

#[test]
fn the_search_finds_titles_first_then_text_and_the_file_path() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "Heizung.md", "Wartung im Oktober");
    write(dir.path(), "Reisen.md", "Die Heizung im Hotel war kalt");
    write(dir.path(), "Steuern.md", "nichts");
    let state = loaded(dir.path());
    let hits = search(&state.notes, "heizung");
    assert_eq!(hits.len(), 2);
    assert!(hits[0].note.ends_with("Heizung.md"));
    assert!(hits[1].snippet.contains("Heizung im Hotel"));
    assert!(search(&state.notes, "  ").is_empty());
}

#[test]
fn the_tree_lists_folders_notes_and_the_group_of_file_notes() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "Haus/Heizung.md", "x");
    write(dir.path(), "Reisen.md", "x");
    let mut state = loaded(dir.path());
    let keys: Vec<_> = state.rows().into_iter().map(|row| row.key).collect();
    assert_eq!(
        keys,
        [
            RowKey::Folder("Haus".into()),
            RowKey::Note(dir.path().join("Reisen.md"))
        ]
    );
    let _ = update(
        &mut state,
        Message::Pressed(RowKey::Folder("Haus".into())),
        &CTX,
    );
    assert_eq!(state.rows().len(), 3);
}

#[tokio::test]
async fn escape_leaves_the_editor_then_closes_the_tab() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "Reisen.md", "Italien");
    let mut state = loaded(dir.path());
    state.open = true;
    let note = dir.path().join("Reisen.md");
    let _ = update(&mut state, Message::Pressed(RowKey::Note(note)), &CTX);
    let _ = update(&mut state, Message::Enter, &CTX);
    assert_eq!(state.mode, Mode::Edit);
    assert_eq!(action(&mut state, Message::Escape), None);
    assert_eq!(state.mode, Mode::View);
    assert_eq!(action(&mut state, Message::Escape), Some(Action::Close));
    assert!(!state.is_open());
}

#[tokio::test]
async fn a_new_note_is_edited_and_takes_its_title_from_the_first_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(dir.path());
    state.open = true;
    let _ = update(&mut state, Message::NewNote, &CTX);
    assert!(state.editing());
    assert_eq!(state.shown().unwrap().title, "New note");
    let _ = update(
        &mut state,
        Message::Edited(iced::widget::text_editor::Action::Edit(
            iced::widget::text_editor::Edit::Paste("Ideen fuer Mai".to_owned().into()),
        )),
        &CTX,
    );
    assert_eq!(state.shown().unwrap().text.trim(), "Ideen fuer Mai");
    let generation = state.generation;
    let _ = update(&mut state, Message::SaveDue(generation), &CTX);
    assert_eq!(state.shown().unwrap().title, "Ideen fuer Mai");
    assert!(state
        .current
        .as_ref()
        .unwrap()
        .ends_with("Ideen fuer Mai.md"));
}

#[tokio::test]
async fn a_note_on_a_file_is_made_once_and_asks_the_index_for_the_document() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(dir.path());
    state.open = true;
    let file = PathBuf::from("/docs/a.pdf");
    let asked = action(&mut state, Message::EditNoteOn(file.clone()));
    assert_eq!(
        asked,
        Some(Action::ResolveLinks(vec![(None, file.clone())]))
    );
    assert!(state.editing());
    assert!(state.noted_files().contains(&file));
    let again = action(&mut state, Message::EditNoteOn(file.clone()));
    assert!(again.is_some());
    assert_eq!(state.notes.len(), 1);
}

#[tokio::test]
async fn the_resolved_path_replaces_the_one_in_the_note() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(dir.path());
    let old = PathBuf::from("/docs/a.pdf");
    let moved = PathBuf::from("/archive/a.pdf");
    let _ = action(&mut state, Message::EditNoteOn(old.clone()));
    let _ = update(&mut state, Message::SetMode(Mode::View), &CTX);
    let _ = update(
        &mut state,
        Message::LinksResolved(Ok(vec![(Some(5), moved.clone())])),
        &CTX,
    );
    assert!(state.note_on(&old).is_none());
    let note = state.note_on(&moved).unwrap();
    assert_eq!(
        note.kind,
        Kind::File {
            file: moved,
            document: Some(5)
        }
    );
}

#[test]
fn links_go_to_the_browser_or_to_the_file() {
    let mut state = State::default();
    assert_eq!(
        action(
            &mut state,
            Message::LinkClicked("https://example.org".into())
        ),
        Some(Action::OpenUrl("https://example.org".into()))
    );
    assert_eq!(
        action(
            &mut state,
            Message::LinkClicked("file:///a/b%20c.pdf".into())
        ),
        Some(Action::Reveal(PathBuf::from("/a/b c.pdf")))
    );
    assert_eq!(
        action(&mut state, Message::LinkClicked("#top".into())),
        None
    );
}

#[test]
fn keys_arrows_and_enter_only_outside_the_editor() {
    let key = |named| Key::Named(named);
    assert_eq!(
        notes_key(&key(Named::ArrowDown), Modifiers::empty(), true),
        None
    );
    assert_eq!(
        notes_key(&key(Named::ArrowDown), Modifiers::empty(), false),
        Some(Message::Down)
    );
    assert_eq!(
        notes_key(&key(Named::Escape), Modifiers::empty(), false),
        None
    );
    assert_eq!(
        notes_key(&Key::Character("n".into()), Modifiers::COMMAND, true),
        Some(Message::NewNote)
    );
}
