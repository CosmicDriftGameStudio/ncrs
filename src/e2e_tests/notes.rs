//! A note on a file in the real app: ⌘⇧N makes it and opens the notes tab in
//! the editor, the row gets its marker, and a note finds its file again when
//! the index says the file moved.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use iced::widget::text_editor::{Action, Edit};
use ncrs_notes::Mode;

use super::*;

fn note_on_file_keys() -> Modifiers {
    Modifiers::COMMAND | Modifiers::SHIFT
}

#[test]
fn command_shift_n_makes_a_note_on_the_file_and_the_note_follows_a_moved_file() {
    let dir = scratch("notes");
    write(&dir.path().join("offer.pdf"), "pdf");
    let file = std::fs::canonicalize(dir.path()).unwrap().join("offer.pdf");

    let mut session = session_in(dir.path());
    cursor_to(&mut session, "offer.pdf");
    session.press_with(Key::Character("n".into()), note_on_file_keys());
    session.settle();

    assert!(session.app.notes_are_shown_for_test());
    assert_eq!(session.app.notes_for_test().mode, Mode::Edit);
    assert!(session.app.noted_for_test().contains(&file));

    session.send(Message::Notes(ncrs_notes::Message::Edited(Action::Edit(
        Edit::Paste("Valid until December".to_owned().into()),
    ))));
    session.send(Message::Notes(ncrs_notes::Message::Escape));
    session.settle();
    assert_eq!(session.app.notes_for_test().mode, Mode::View);

    let moved = file.with_file_name("archive").join("offer.pdf");
    session.send(Message::Notes(ncrs_notes::Message::LinksResolved(Ok(
        vec![(Some(7), moved.clone())],
    ))));
    session.settle();
    assert!(session.app.noted_for_test().contains(&moved));
    assert!(!session.app.noted_for_test().contains(&file));

    session.press(Key::Named(Named::Escape));
    assert!(!session.app.notes_are_shown_for_test());
}

#[test]
fn a_note_typed_in_the_last_pause_is_on_disk_when_the_app_quits() {
    let dir = scratch("notes-quit");
    write(&dir.path().join("offer.pdf"), "pdf");

    let mut session = session_in(dir.path());
    cursor_to(&mut session, "offer.pdf");
    session.press_with(Key::Character("n".into()), note_on_file_keys());
    session.settle();
    let note = session
        .app
        .notes_for_test()
        .current
        .clone()
        .expect("the note is shown");

    session.send(Message::Notes(ncrs_notes::Message::Edited(Action::Edit(
        Edit::Paste("typed just before quitting".to_owned().into()),
    ))));
    session.app.quit_for_test();

    let on_disk = std::fs::read_to_string(note).unwrap();
    assert!(on_disk.contains("typed just before quitting"), "{on_disk}");
}
