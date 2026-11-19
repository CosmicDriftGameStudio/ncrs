use std::fs;
use std::path::{Path, PathBuf};

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_i18n::Language;

use crate::frontmatter::{compose, split, FrontMatter};
use crate::search::search;
use crate::state::Write;
use crate::store::{self, file_note_path, scan, Expect, Kind, Replaced, WriteError, FILES_DIR};
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

fn flush_all(state: &mut State) {
    assert_eq!(flush_now(state, &CTX), Vec::<String>::new());
}

fn job(path: &Path, text: &str, expect: Expect, generation: u64) -> Write {
    Write {
        path: path.to_owned(),
        text: text.to_owned(),
        expect,
        replaces: None,
        generation,
        root: path.parent().unwrap().to_owned(),
        background: false,
    }
}

fn hash_of(text: &str) -> u64 {
    store::content_hash(text.as_bytes())
}

/// Answers the write that runs, as the runtime would.
fn saved_ok(state: &mut State) {
    let running = state.running.clone().expect("a write runs");
    let hash = hash_of(&running.text);
    let _ = update(
        state,
        Message::Saved(running.path, running.generation, Ok(hash)),
        &CTX,
    );
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
            document: Some(7),
            hash: None
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
        hash: None,
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
    assert_eq!(asked, Some(Action::ResolveLinks(vec![link(None, &file)])));
    assert!(state.editing());
    assert!(state.noted.contains(&file));
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
        Message::LinksResolved(vec![old.clone()], Ok(vec![link(Some(5), &moved)])),
        &CTX,
    );
    assert!(state.note_on(&old).is_none());
    let note = state.note_on(&moved).unwrap();
    assert_eq!(
        note.kind,
        Kind::File {
            file: moved,
            document: Some(5),
            hash: None
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

    flush_all(&mut state);

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
    flush_all(state);
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
            hash: Some("ab12".to_owned()),
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
        Message::LinksResolved(vec![b.clone()], Ok(vec![link(Some(2), &b_moved)])),
        &CTX,
    );
    let _ = update(
        &mut state,
        Message::LinksResolved(vec![a.clone()], Ok(vec![link(Some(1), &a_moved)])),
        &CTX,
    );
    assert!(state.note_on(&a).is_none() && state.note_on(&b).is_none());
    assert_eq!(
        state.note_on(&a_moved).unwrap().kind,
        Kind::File {
            file: a_moved.clone(),
            document: Some(1),
            hash: None
        }
    );
    assert_eq!(
        state.note_on(&b_moved).unwrap().kind,
        Kind::File {
            file: b_moved.clone(),
            document: Some(2),
            hash: None
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
    assert!(state.writing(), "the first write of the new note runs");
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

    saved_ok(&mut state);
    wait_for("the second write", || {
        root.join("Plan.md").exists() && !root.join("New note.md").exists()
    })
    .await;
    saved_ok(&mut state);
    let written = root.join("Plan B.md");
    wait_for("the third write", || {
        written.exists() && !root.join("Plan.md").exists()
    })
    .await;
    saved_ok(&mut state);
    assert!(!state.writing() && state.queued.is_empty());
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
    store::write_note(&job(&path, "second", Expect::Absent, 2), &ledger).unwrap();
    store::write_note(&job(&path, "first", Expect::Absent, 1), &ledger).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "second");
}

#[test]
fn a_skipped_older_write_still_removes_the_file_it_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("Old.md");
    let path = dir.path().join("New.md");
    fs::write(&old, "x").unwrap();
    let ledger = store::Ledger::default();
    store::write_note(&job(&path, "second", Expect::Absent, 2), &ledger).unwrap();
    let mut older = job(&path, "first", Expect::Absent, 1);
    older.replaces = Some(Replaced {
        path: old.clone(),
        base: Some(hash_of("x")),
    });
    store::write_note(&older, &ledger).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    assert!(!old.exists());
}

#[cfg(unix)]
#[test]
fn a_written_note_gets_the_umask_and_an_existing_file_keeps_its_rights() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let probe = dir.path().join("probe");
    fs::write(&probe, "a").unwrap();
    let usual = fs::metadata(&probe).unwrap().permissions().mode() & 0o777;
    let ledger = store::Ledger::default();
    let fresh = dir.path().join("Fresh.md");
    store::write_note(&job(&fresh, "a", Expect::Absent, 1), &ledger).unwrap();
    assert_eq!(
        fs::metadata(&fresh).unwrap().permissions().mode() & 0o777,
        usual
    );
    let existing = dir.path().join("Own.md");
    fs::write(&existing, "a").unwrap();
    fs::set_permissions(&existing, fs::Permissions::from_mode(0o600)).unwrap();
    store::write_note(
        &job(&existing, "b", Expect::Content(hash_of("a")), 1),
        &ledger,
    )
    .unwrap();
    assert_eq!(
        fs::metadata(&existing).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn a_note_that_is_a_link_is_written_through_it() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("elsewhere.md");
    fs::write(&target, "old").unwrap();
    let link = dir.path().join("Linked.md");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let ledger = store::Ledger::default();
    store::write_note(
        &job(&link, "new", Expect::Content(hash_of("old")), 1),
        &ledger,
    )
    .unwrap();
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
}

#[cfg(unix)]
#[test]
fn a_note_that_is_a_link_out_of_the_notes_folder_is_not_written_through() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("notes");
    fs::create_dir(&root).unwrap();
    let outside = dir.path().join("zshrc");
    fs::write(&outside, "keep").unwrap();
    let link = root.join("x.md");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let ledger = store::Ledger::default();

    let result = store::write_note(
        &job(&link, "new", Expect::Content(hash_of("keep")), 1),
        &ledger,
    );

    assert!(matches!(result, Err(store::WriteError::Io(_))));
    assert_eq!(fs::read_to_string(&outside).unwrap(), "keep");
}

#[tokio::test]
async fn a_file_note_with_windows_line_ends_and_a_mark_is_read_and_never_emptied() {
    let dir = tempfile::tempdir().unwrap();
    let file = Path::new("/docs/Offer.pdf");
    let path = file_note_path(dir.path(), file);
    let original =
        "\u{feff}---\r\nfile: /docs/Offer.pdf\r\ndocument: 4\r\n---\r\nValid until May\r\n";
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, original).unwrap();
    let mut state = loaded(dir.path());
    let note = state.note_on(file).expect("the note is read");
    assert_eq!(note.text, "Valid until May\r\n");
    let _ = action(&mut state, Message::EditNoteOn(file.to_owned()));
    flush_all(&mut state);
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    assert_eq!(state.notes.len(), 1);
}

#[tokio::test]
async fn a_file_that_is_not_a_note_where_a_file_note_would_go_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let file = Path::new("/docs/Offer.pdf");
    let path = file_note_path(dir.path(), file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes: &[u8] = &[0xff, 0xfe, b'x', 0x80];
    fs::write(&path, bytes).unwrap();
    let mut state = loaded(dir.path());
    assert!(state.notes.is_empty());
    let asked = action(&mut state, Message::EditNoteOn(file.to_owned()));
    assert!(matches!(asked, Some(Action::Error(_))), "{asked:?}");
    assert!(state.notes.is_empty());
    flush_all(&mut state);
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn a_write_over_a_file_that_changed_outside_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Plan.md");
    fs::write(&path, "theirs").unwrap();
    let ledger = store::Ledger::default();
    let stale = store::write_note(
        &job(&path, "mine", Expect::Content(hash_of("ours")), 1),
        &ledger,
    );
    assert_eq!(stale, Err(WriteError::Conflict(path.clone())));
    let taken = store::write_note(&job(&path, "mine", Expect::Absent, 2), &ledger);
    assert_eq!(taken, Err(WriteError::Conflict(path.clone())));
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs");
}

#[test]
fn two_own_writes_in_a_row_are_no_conflict_even_with_a_stale_base() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Plan.md");
    let ledger = store::Ledger::default();
    store::write_note(&job(&path, "one", Expect::Absent, 1), &ledger).unwrap();
    store::write_note(&job(&path, "two", Expect::Absent, 2), &ledger).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "two");
}

#[test]
fn a_rename_onto_a_name_that_appeared_meanwhile_overwrites_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("Old.md");
    let wanted = dir.path().join("Plan.md");
    fs::write(&old, "mine").unwrap();
    fs::write(&wanted, "theirs").unwrap();
    let mut rename = job(&wanted, "mine", Expect::Absent, 1);
    rename.replaces = Some(Replaced {
        path: old.clone(),
        base: Some(hash_of("mine")),
    });
    let result = store::write_note(&rename, &store::Ledger::default());
    assert!(matches!(result, Err(WriteError::Conflict(_))));
    assert_eq!(fs::read_to_string(&wanted).unwrap(), "theirs");
    assert!(old.exists());
}

#[test]
fn a_replaced_file_that_changed_outside_stays() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("Old.md");
    fs::write(&old, "theirs").unwrap();
    let mut rename = job(&dir.path().join("New.md"), "mine", Expect::Absent, 1);
    rename.replaces = Some(Replaced {
        path: old.clone(),
        base: Some(hash_of("ours")),
    });
    store::write_note(&rename, &store::Ledger::default()).unwrap();
    assert!(old.exists());
}

#[tokio::test]
async fn an_edit_over_a_note_changed_outside_is_saved_as_a_copy() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "Trips.md", "Italy");
    let mut state = loaded(dir.path());
    state.open = true;
    let note = dir.path().join("Trips.md");
    let _ = update(
        &mut state,
        Message::Pressed(RowKey::Note(note.clone())),
        &CTX,
    );
    let _ = update(&mut state, Message::Enter, &CTX);
    fs::write(&note, "Italy and spring").unwrap();
    paste(&mut state, " and Spain");
    flush_all(&mut state);
    assert_eq!(fs::read_to_string(&note).unwrap(), "Italy and spring");
    let copy = dir.path().join("Trips (conflict).md");
    assert!(fs::read_to_string(&copy).unwrap().contains("and Spain"));
    assert_eq!(state.current.as_ref(), Some(&copy));
}

#[tokio::test]
async fn a_failed_write_is_kept_and_the_note_shown_now_is_not_marked() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "A.md", "a");
    write(dir.path(), "B.md", "b");
    let mut state = loaded(dir.path());
    state.open = true;
    let a = dir.path().join("A.md");
    let b = dir.path().join("B.md");
    let _ = update(&mut state, Message::Pressed(RowKey::Note(a.clone())), &CTX);
    let _ = update(&mut state, Message::Enter, &CTX);
    paste(&mut state, "1");
    let failing = Write {
        path: a.clone(),
        text: state.shown().unwrap().file_text(),
        expect: Expect::Content(hash_of("a")),
        replaces: None,
        generation: 1,
        root: a.parent().unwrap().to_owned(),
        background: false,
    };
    state.dirty = false;
    state.running = Some(failing);
    let _ = update(&mut state, Message::Pressed(RowKey::Note(b)), &CTX);
    let _ = update(
        &mut state,
        Message::Saved(a.clone(), 1, Err(WriteError::Io("disk full".into()))),
        &CTX,
    );
    assert!(!state.dirty, "the note shown now is not marked");
    assert_eq!(state.unsaved.len(), 1);
    flush_all(&mut state);
    assert!(fs::read_to_string(&a).unwrap().contains('1'));
    assert!(state.unsaved.is_empty());
}

#[tokio::test]
async fn a_read_of_the_folder_is_thrown_away_while_a_write_is_on_its_way() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "A.md", "a");
    let mut state = loaded(dir.path());
    state.running = Some(job(&dir.path().join("A.md"), "a", Expect::Absent, 1));
    write(dir.path(), "Other.md", "x");
    let _ = update(
        &mut state,
        Message::Loaded(Box::new(scan(dir.path()))),
        &CTX,
    );
    assert_eq!(state.notes.len(), 1);
    assert!(state.reload_after_writes);
}

#[tokio::test]
async fn a_read_that_started_before_a_write_was_asked_for_is_thrown_away() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(dir.path());
    state.open = true;
    let old_read = scan(dir.path()).unwrap();
    let _ = update(&mut state, Message::NewNote, &CTX);
    assert_eq!(state.notes.len(), 1);
    saved_ok(&mut state);
    let _ = update(&mut state, Message::Loaded(Box::new(Ok(old_read))), &CTX);
    assert_eq!(state.notes.len(), 1, "the new note stays");
    assert!(state.current.is_some());
}

#[test]
fn titles_stay_within_what_a_file_system_takes() {
    assert!(store::clean_title(&"ä".repeat(300)).len() <= 200);
    assert_eq!(store::clean_title("con"), "con_");
    assert_eq!(store::clean_title("NUL.txt"), "NUL_.txt");
    assert_eq!(store::clean_title("Plan."), "Plan");
    let long = PathBuf::from(format!("/x/{}.pdf", "é".repeat(200)));
    let name = file_note_path(Path::new("/n"), &long);
    assert!(name.file_name().unwrap().len() <= 255);
}

#[test]
fn front_matter_accepts_windows_line_ends_and_a_mark() {
    let (front, body) = split("\u{feff}---\r\nfile: /a.pdf\r\n---\r\ntext");
    assert_eq!(front.unwrap().file, PathBuf::from("/a.pdf"));
    assert_eq!(body, "text");
}

#[tokio::test]
async fn a_failed_write_does_not_stop_the_folder_from_being_read_and_keeps_its_note() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "A.md", "a");
    let mut state = loaded(dir.path());
    state.open = true;
    let a = dir.path().join("A.md");
    let _ = update(&mut state, Message::Pressed(RowKey::Note(a.clone())), &CTX);
    let _ = update(&mut state, Message::Enter, &CTX);
    paste(&mut state, "1");
    let typed = state.shown().unwrap().file_text();
    state.dirty = false;
    state.keep_unsaved(job(&a, &typed, Expect::Content(hash_of("a")), 1));
    assert!(!state.busy());
    write(dir.path(), "Other.md", "x");
    let task = crate::reload(&mut state);
    assert!(state.loading, "the read starts");
    drop(task);
    let _ = update(
        &mut state,
        Message::Loaded(Box::new(scan(dir.path()))),
        &CTX,
    );
    assert_eq!(state.notes.len(), 2, "the new file shows up");
    assert_eq!(state.note(&a).unwrap().file_text(), typed);
    flush_all(&mut state);
    assert!(fs::read_to_string(&a).unwrap().contains('1'));
}

fn link(document: Option<i64>, file: &Path) -> ncrs_core::NoteLink {
    ncrs_core::NoteLink {
        document,
        hash: None,
        file: file.to_owned(),
    }
}
