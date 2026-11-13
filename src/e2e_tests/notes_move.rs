//! #295's core criterion in the real app: a note on a file survives a move of
//! the file inside an indexed location. Hermes runs over a read-only location
//! with a plain index; the move is the user's own F6.

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::PathBuf;
use std::time::Duration;

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use iced::widget::text_editor::{Action, Edit};
use ncrs_notes::store::Kind;
use ncrs_notes::tree::RowKey;

use super::{cursor_to, write, Session};
use crate::app::App;
use crate::filing::{FilingMessage, Phase};
use crate::hermes_protocol::real_start::{boot_with, empty_store, pump_for};
use crate::hermes_protocol::thread::is_ready;
use crate::messages::{Message, PanelSide};
use crate::session_state::SessionState;

const NOTE_TEXT: &str = "Kundennummer 4711";

pub(in crate::e2e_tests) trait Live:
    iced::Program<State = App, Message = Message> + 'static
{
}
impl<P: iced::Program<State = App, Message = Message> + 'static> Live for P {}

/// An indexed location with a file and a subfolder, and a note on the file.
pub(in crate::e2e_tests) struct NoteOnIndexedFile {
    pub _data: tempfile::TempDir,
    pub notes: PathBuf,
    pub before: PathBuf,
    pub after: PathBuf,
}

/// Whether the index has a document at `file`. The scan indexes the 82 files in
/// no fixed order, so a count of documents does not say `file` is among them.
fn index_knows(app: &App, file: &std::path::Path) -> bool {
    app.filing_module_for_test()
        .and_then(|module| module.note_link_finder())
        .and_then(|finder| finder.resolve(&[(None, file.to_path_buf())]).ok())
        .is_some_and(|resolved| resolved.first().is_some_and(|(id, _)| id.is_some()))
}

fn has_document(app: &App, file: &std::path::Path) -> bool {
    app.notes_for_test().note_on(file).is_some_and(|note| {
        matches!(
            note.kind,
            Kind::File {
                document: Some(_),
                ..
            }
        )
    })
}

/// Boots Hermes on a location (left panel at its top, right panel in `Sub`),
/// waits until the index knows the files and makes the note with ⌘⇧N.
pub(in crate::e2e_tests) fn boot_with_a_note_on_an_indexed_file(
    language: &str,
) -> (NoteOnIndexedFile, Session<impl Live>) {
    let data = tempfile::TempDir::new().unwrap();
    let root = std::fs::canonicalize(tempfile::TempDir::new().unwrap().keep()).unwrap();
    write(
        &root.join("offer.txt"),
        "Angebot Heizung Vaillant, gueltig bis Dezember",
    );
    write(&root.join("other.txt"), "Mietvertrag Haus");
    for number in 0..40 {
        write(&root.join(format!("Beleg-{number:02}.txt")), "Beleg");
        write(&root.join(format!("Sub/Notiz-{number:02}.txt")), "Notiz");
    }
    let notes = data.path().join("Notizen");
    std::fs::write(
        data.path().join("config.toml"),
        format!(
            "version = 1\nlang = \"{language}\"\n[modules.filing]\nenabled = true\n\
             [[modules.filing.locations]]\npath = {root:?}\nmode = \"archive\"\n\
             [notes]\nfolder = {notes:?}\n"
        ),
    )
    .unwrap();
    drop(ncrs_index::Index::create_plain(&data.path().join("index.db")).unwrap());

    let mut saved = SessionState::default();
    saved.left.path = Some(root.to_string_lossy().into_owned());
    saved.right.path = Some(root.join("Sub").to_string_lossy().into_owned());
    let mut session = boot_with(data.path(), empty_store(), saved);
    let mut confirmed = false;
    for _ in 0..300 {
        pump_for(&mut session, Duration::from_millis(200), is_ready);
        if is_ready(&session.app) {
            break;
        }
        if !confirmed
            && session
                .app
                .filing_module_for_test()
                .is_some_and(|module| matches!(module.phase(), Phase::PlainFileWarning))
        {
            confirmed = true;
            session.send(Message::Filing(FilingMessage::ConfirmPlainFile));
        }
    }
    assert!(is_ready(&session.app), "Hermes never got ready");
    let offer = root.join("offer.txt");
    for _ in 0..300 {
        pump_for(&mut session, Duration::from_millis(200), |_| false);
        if index_knows(&session.app, &offer) {
            break;
        }
    }
    assert!(
        index_knows(&session.app, &offer),
        "the index never learned offer.txt"
    );

    let before = offer;
    session.press(Key::Named(Named::Home));
    cursor_to(&mut session, "offer.txt");
    session.press_with(
        Key::Character("n".into()),
        Modifiers::COMMAND | Modifiers::SHIFT,
    );
    session.settle();
    assert!(session.app.notes_are_shown_for_test());
    for _ in 0..100 {
        if has_document(&session.app, &before) {
            break;
        }
        pump_for(&mut session, Duration::from_millis(100), |_| false);
    }
    assert!(
        has_document(&session.app, &before),
        "the note never learned the document id"
    );
    session.send(Message::Notes(ncrs_notes::Message::Edited(Action::Edit(
        Edit::Paste(NOTE_TEXT.to_owned().into()),
    ))));
    session.send(Message::Notes(ncrs_notes::Message::Escape));
    session.send(Message::Notes(ncrs_notes::Message::Escape));
    assert!(!session.app.notes_are_shown_for_test());
    let after = root.join("Sub").join("offer.txt");
    (
        NoteOnIndexedFile {
            _data: data,
            notes,
            before,
            after,
        },
        session,
    )
}

fn the_file_note(notes: &std::path::Path) -> String {
    let found: Vec<_> = std::fs::read_dir(notes.join(".files"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(found.len(), 1, "one note on a file: {found:?}");
    std::fs::read_to_string(&found[0]).unwrap()
}

/// F6 on the file into the other panel's `Sub`, then opening the notes tab
/// until the index has told the note where the file went. Leaves the panels
/// shown.
pub(in crate::e2e_tests) fn move_the_file_and_let_the_note_follow(
    moved: &NoteOnIndexedFile,
    session: &mut Session<impl Live>,
) {
    session.press(Key::Named(Named::Home));
    cursor_to(session, "offer.txt");
    session.press(Key::Named(Named::F6));
    session.press(Key::Named(Named::Enter));
    assert!(moved.after.is_file(), "F6 did not move the file");
    assert!(!moved.before.exists());

    for _ in 0..120 {
        session.send(Message::OpenNotes);
        if session.app.noted_for_test().contains(&moved.after) {
            break;
        }
        pump_for(session, Duration::from_millis(500), |_| false);
    }
    assert!(
        session.app.noted_for_test().contains(&moved.after),
        "the note did not follow the file; it still points at {:?}",
        session.app.noted_for_test()
    );
    session.send(Message::Notes(ncrs_notes::Message::Escape));
    assert!(!session.app.notes_are_shown_for_test());
}

pub(in crate::e2e_tests) fn assert_the_note_followed(
    moved: &NoteOnIndexedFile,
    app: &App,
    front_before: &str,
) {
    let notes = app.notes_for_test();
    assert!(notes.note_on(&moved.before).is_none());
    let note = notes.note_on(&moved.after).expect("a note on the new path");
    assert!(note.text.contains(NOTE_TEXT));
    assert!(
        notes
            .rows()
            .iter()
            .any(|row| row.key == RowKey::Note(note.path.clone())),
        "the tree does not list the note"
    );
    let right = app.panel(PanelSide::Right);
    let row = right
        .entries
        .iter()
        .find(|entry| entry.name == "offer.txt")
        .expect("the moved file is listed in Sub");
    assert!(app.noted_for_test().contains(&right.path.join(&row.name)));

    let front = the_file_note(&moved.notes);
    assert!(
        front.contains(&format!("file: {}", moved.after.display())),
        "{front}"
    );
    assert!(!front.contains(&format!("file: {}\n", moved.before.display())));
    let document = |text: &str| {
        text.lines()
            .find(|line| line.starts_with("document:"))
            .map(str::to_owned)
    };
    assert!(document(&front).is_some());
    assert_eq!(document(&front), document(front_before));
    assert!(front.contains(NOTE_TEXT));
}

#[test]
fn a_note_on_a_file_follows_the_file_moved_with_f6_inside_the_location() {
    let (moved, mut session) = boot_with_a_note_on_an_indexed_file("en");
    let front_before = the_file_note(&moved.notes);
    assert!(front_before.contains(&format!("file: {}", moved.before.display())));

    move_the_file_and_let_the_note_follow(&moved, &mut session);

    assert_the_note_followed(&moved, &session.app, &front_before);
}
