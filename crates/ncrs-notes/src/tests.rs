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
use crate::{flush_now, notes_key, update, Action, Ctx, Message, Mode, State};

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
        Message::LinksResolved(vec![old.clone()], Ok(vec![(Some(5), moved.clone())])),
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

#[tokio::test]
async fn flushing_now_writes_the_pending_edit_without_waiting_for_the_pause() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "Reisen.md", "Italien");
    let mut state = loaded(dir.path());
    state.open = true;
    let note = dir.path().join("Reisen.md");
    let _ = update(
        &mut state,
        Message::Pressed(RowKey::Note(note.clone())),
        &CTX,
    );
    let _ = update(&mut state, Message::Enter, &CTX);
    let _ = update(
        &mut state,
        Message::Edited(iced::widget::text_editor::Action::Edit(
            iced::widget::text_editor::Edit::Paste(" und Spanien".to_owned().into()),
        )),
        &CTX,
    );
    assert!(state.dirty);

    flush_now(&mut state).unwrap();

    assert!(fs::read_to_string(note).unwrap().contains("und Spanien"));
    assert!(!state.dirty);
}

fn type_first_line(state: &mut State, text: &str) {
    let _ = update(
        state,
        Message::Edited(iced::widget::text_editor::Action::SelectAll),
        &CTX,
    );
    let _ = update(
        state,
        Message::Edited(iced::widget::text_editor::Action::Edit(
            iced::widget::text_editor::Edit::Paste(text.to_owned().into()),
        )),
        &CTX,
    );
    flush_now(state).unwrap();
}

/// Every note file the program wrote, and proof that nothing else appeared outside `root`.
fn assert_only_plain_notes_in(base: &Path, root: &Path) {
    let outside: Vec<_> = fs::read_dir(base)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(
        outside,
        [root.file_name().unwrap()],
        "wrote outside the notes folder"
    );
    let canonical = root.canonicalize().unwrap();
    for entry in fs::read_dir(root).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(!name.starts_with('.'), "dot file {name}");
        assert!(!entry.file_type().unwrap().is_dir(), "{name} is not a file");
        assert_eq!(
            entry.path().canonicalize().unwrap().parent().unwrap(),
            canonical
        );
    }
}

#[tokio::test]
async fn a_first_line_never_makes_a_note_land_outside_the_notes_folder() {
    for line in [
        "../../x",
        "/etc/passwd",
        "..",
        ".hidden",
        "a/b\\c",
        "../",
        "...",
        "",
        "   \n  ",
    ] {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("notes");
        fs::create_dir_all(&root).unwrap();
        let mut state = loaded(&root);
        state.open = true;
        let _ = update(&mut state, Message::NewNote, &CTX);
        first_write_done(&root).await;
        type_first_line(&mut state, line);
        assert_only_plain_notes_in(base.path(), &root);
        let path = state.current.clone().unwrap();
        assert_eq!(path.parent().unwrap(), root, "{line:?}");
        assert!(path.exists(), "{line:?}");
        if line.trim().is_empty() {
            assert_eq!(state.shown().unwrap().title, "New note");
        }
    }
}

#[tokio::test]
async fn renaming_by_a_changed_first_line_stays_in_the_notes_folder() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("notes");
    fs::create_dir_all(&root).unwrap();
    let mut state = loaded(&root);
    state.open = true;
    let _ = update(&mut state, Message::NewNote, &CTX);
    first_write_done(&root).await;
    type_first_line(&mut state, "Plain");
    for line in ["../../x", "/etc/passwd", "..", ".hidden", "a/b\\c"] {
        type_first_line(&mut state, line);
        assert_only_plain_notes_in(base.path(), &root);
        assert_eq!(state.current.as_ref().unwrap().parent().unwrap(), root);
    }
}

/// `NewNote` starts its first write on a blocking thread at once; wait for it so it does not race the test's own writes.
async fn first_write_done(root: &Path) {
    for _ in 0..200 {
        let settled = fs::read_dir(root)
            .unwrap()
            .flatten()
            .all(|entry| !entry.file_name().to_string_lossy().starts_with('.'));
        if settled && root.join("New note.md").exists() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the first write of the new note did not finish");
}

#[test]
fn a_path_with_a_hash_spaces_or_a_line_break_reads_back_as_written() {
    for name in [
        "/x/Rechnung #12.pdf",
        "/x/a #b #c .pdf",
        "/x/two\nlines.pdf",
        "/x/carriage\rreturn \\ \"q\".pdf",
        "\"/quoted",
        " /lead and trail ",
    ] {
        let front = FrontMatter {
            file: PathBuf::from(name),
            document: Some(3),
        };
        let text = compose(&front, "Body\n---\nmore\n");
        let (read, body) = split(&text);
        assert_eq!(read, Some(front), "{name:?}");
        assert_eq!(body, "Body\n---\nmore\n");
    }
}

#[tokio::test]
async fn two_overlapping_answers_each_note_keeps_its_own_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(dir.path());
    let (a, b) = (PathBuf::from("/docs/a.pdf"), PathBuf::from("/docs/b.pdf"));
    let _ = action(&mut state, Message::EditNoteOn(a.clone()));
    let _ = action(&mut state, Message::EditNoteOn(b.clone()));
    let (a_moved, b_moved) = (PathBuf::from("/new/a.pdf"), PathBuf::from("/new/b.pdf"));
    let _ = update(
        &mut state,
        Message::LinksResolved(vec![b.clone()], Ok(vec![(Some(2), b_moved.clone())])),
        &CTX,
    );
    let _ = update(
        &mut state,
        Message::LinksResolved(vec![a.clone()], Ok(vec![(Some(1), a_moved.clone())])),
        &CTX,
    );
    assert!(state.note_on(&a).is_none() && state.note_on(&b).is_none());
    assert_eq!(
        state.note_on(&a_moved).unwrap().kind,
        Kind::File {
            file: a_moved.clone(),
            document: Some(1)
        }
    );
    assert_eq!(
        state.note_on(&b_moved).unwrap().kind,
        Kind::File {
            file: b_moved.clone(),
            document: Some(2)
        }
    );
}

fn paste(state: &mut State, text: &str) {
    let _ = update(
        state,
        Message::Edited(iced::widget::text_editor::Action::Edit(
            iced::widget::text_editor::Edit::Paste(text.to_owned().into()),
        )),
        &CTX,
    );
}

async fn wait_for(what: &str, condition: impl Fn() -> bool) {
    for _ in 0..300 {
        if condition() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("never happened: {what}");
}

#[tokio::test]
async fn a_second_write_waits_for_the_first_and_the_rename_leaves_one_file() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("notes");
    fs::create_dir_all(&root).unwrap();
    let mut state = loaded(&root);
    state.open = true;
    let _ = update(&mut state, Message::NewNote, &CTX);
    assert!(state.writing, "the first write of the new note runs");
    paste(&mut state, "Plan");
    let first_generation = state.generation;
    let _ = update(&mut state, Message::SaveDue(first_generation), &CTX);
    paste(&mut state, " B");
    let second_generation = state.generation;
    let _ = update(&mut state, Message::SaveDue(second_generation), &CTX);
    assert_eq!(
        state.queued.len(),
        2,
        "both renames wait behind the first write"
    );
    wait_for("the first write", || root.join("New note.md").exists()).await;
    assert!(
        !root.join("Plan.md").exists(),
        "nothing overtakes the first write"
    );

    let _ = update(&mut state, Message::Saved(Ok(())), &CTX);
    wait_for("the second write", || {
        root.join("Plan.md").exists() && !root.join("New note.md").exists()
    })
    .await;
    let _ = update(&mut state, Message::Saved(Ok(())), &CTX);
    let written = root.join("Plan B.md");
    wait_for("the third write", || {
        written.exists() && !root.join("Plan.md").exists()
    })
    .await;
    let _ = update(&mut state, Message::Saved(Ok(())), &CTX);
    assert!(!state.writing && state.queued.is_empty());
    let names: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(names, ["Plan B.md"]);
    assert_eq!(fs::read_to_string(written).unwrap().trim(), "Plan B");
}

#[tokio::test]
async fn a_title_that_only_changes_its_case_keeps_the_one_file() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("notes");
    fs::create_dir_all(&root).unwrap();
    let mut state = loaded(&root);
    state.open = true;
    let _ = update(&mut state, Message::NewNote, &CTX);
    first_write_done(&root).await;
    type_first_line(&mut state, "foo");
    type_first_line(&mut state, "Foo");
    let notes: Vec<_> = fs::read_dir(&root).unwrap().flatten().collect();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(state.shown().unwrap().title, "Foo");
    assert_eq!(
        fs::read_to_string(state.current.clone().unwrap()).unwrap(),
        "Foo"
    );
}

#[test]
fn a_slow_older_write_does_not_land_over_a_newer_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Plan.md");
    let ledger = store::Ledger::default();
    store::write_note(&path, "second", None, 2, &ledger).unwrap();
    store::write_note(&path, "first", None, 1, &ledger).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "second");
}

#[test]
fn a_skipped_older_write_still_removes_the_file_it_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("Old.md");
    let path = dir.path().join("New.md");
    fs::write(&old, "x").unwrap();
    let ledger = store::Ledger::default();
    store::write_note(&path, "second", None, 2, &ledger).unwrap();
    store::write_note(&path, "first", Some(&old), 1, &ledger).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    assert!(!old.exists());
}

#[cfg(unix)]
#[test]
fn a_written_note_is_readable_by_others_and_keeps_the_rights_of_an_existing_file() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let fresh = dir.path().join("Fresh.md");
    store::write_atomic(&fresh, "a").unwrap();
    assert_eq!(
        fs::metadata(&fresh).unwrap().permissions().mode() & 0o777,
        0o644
    );
    let existing = dir.path().join("Own.md");
    fs::write(&existing, "a").unwrap();
    fs::set_permissions(&existing, fs::Permissions::from_mode(0o600)).unwrap();
    store::write_atomic(&existing, "b").unwrap();
    assert_eq!(
        fs::metadata(&existing).unwrap().permissions().mode() & 0o777,
        0o600
    );
}
