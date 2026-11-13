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
    session.send(Message::Notes(ncrs_notes::Message::LinksResolved(
        vec![file.clone()],
        Ok(vec![(Some(7), moved.clone())]),
    )));
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

/// What the panels hold and where the cursor is: the part of the app a key
/// that leaks out of a text field would change.
#[derive(Debug, PartialEq)]
struct PanelsAtRest {
    left: Vec<String>,
    right: Vec<String>,
    active_folder: std::path::PathBuf,
    cursor_name: Option<String>,
    tagged: (bool, bool),
}

fn panels_at_rest(app: &App) -> PanelsAtRest {
    PanelsAtRest {
        left: names(app, PanelSide::Left),
        right: names(app, PanelSide::Right),
        active_folder: app.active_panel().path.clone(),
        cursor_name: app
            .active_panel()
            .selected_entry()
            .map(|entry| entry.name.to_string_lossy().into_owned()),
        tagged: (
            app.panel(PanelSide::Left).selection.any_tagged(),
            app.panel(PanelSide::Right).selection.any_tagged(),
        ),
    }
}

fn press_the_panel_keys(
    session: &mut Session<impl iced::Program<State = App, Message = Message> + 'static>,
) {
    let command = Modifiers::COMMAND;
    for (key, modifiers) in [
        (Key::Character("z".into()), command),
        (Key::Named(Named::Backspace), command),
        (Key::Named(Named::F8), Modifiers::default()),
        (Key::Named(Named::F8), Modifiers::SHIFT),
        (Key::Named(Named::Space), Modifiers::default()),
        (Key::Named(Named::Insert), Modifiers::default()),
        (Key::Named(Named::Tab), Modifiers::default()),
        (Key::Named(Named::Enter), Modifiers::default()),
        (Key::Character("a".into()), command),
        (Key::Named(Named::Delete), Modifiers::default()),
        (Key::Named(Named::F6), Modifiers::default()),
        (Key::Named(Named::F5), Modifiers::default()),
    ] {
        session.press_with(key, modifiers);
    }
}

#[test]
fn keys_pressed_while_a_note_is_edited_reach_neither_panel_and_the_text_is_saved() {
    let dir = scratch("notes-keys");
    for name in ["alpha.txt", "beta.txt", "offer.pdf"] {
        write(&dir.path().join(name), "x");
    }
    write(&dir.path().join("home/right.txt"), "r");
    let bin = tempfile::tempdir().unwrap();
    let mut session = session_with_trash(
        dir.path(),
        std::sync::Arc::new(FolderTrash(bin.path().to_path_buf())),
    );
    cursor_to(&mut session, "offer.pdf");
    let before = panels_at_rest(&session.app);
    session.press_with(Key::Character("n".into()), note_on_file_keys());
    session.settle();
    assert_eq!(session.app.notes_for_test().mode, Mode::Edit);
    let note = session.app.notes_for_test().current.clone().unwrap();

    session.type_into_widgets("first line");
    session.press_in_widgets(Key::Named(Named::Enter), Modifiers::default());
    press_the_panel_keys(&mut session);
    session.type_into_widgets("second line");
    session.settle();

    assert_eq!(panels_at_rest(&session.app), before);
    assert!(
        !session.app.delete_dialog_is_open(),
        "a key opened the delete question"
    );
    assert!(session.app.notes_are_shown_for_test());
    assert_eq!(session.app.notes_for_test().mode, Mode::Edit);
    assert!(
        std::fs::read_dir(bin.path()).unwrap().next().is_none(),
        "something went to the trash"
    );
    for name in ["alpha.txt", "beta.txt", "offer.pdf"] {
        assert!(dir.path().join(name).exists(), "{name} is gone");
    }

    let notes_before = session.app.notes_for_test().notes.len();
    session.press_with(Key::Character("n".into()), Modifiers::COMMAND);
    session.settle();
    assert_eq!(
        session.app.notes_for_test().notes.len(),
        notes_before + 1,
        "⌘N makes a note"
    );
    assert_eq!(panels_at_rest(&session.app), before);
    session.app.quit_for_test();

    let on_disk = std::fs::read_to_string(note).unwrap();
    assert!(on_disk.contains("first line\n"), "{on_disk:?}");
    assert!(on_disk.contains("second line"), "{on_disk:?}");
}
