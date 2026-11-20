//! 1.6.1 acceptance: several Git tabs side by side, and the keys of the notes
//! tab reaching it through the real key routing.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_notes::tree::RowKey;

use super::acceptance_v160::{go_to, pump_until, Live, World};
use super::acceptance_v160_git::{build_development_folder, git_scanned};
use super::Session;
use crate::messages::{GitTabId, Message, ShownTab};

fn shown_git_tab<P: Live>(session: &Session<P>) -> GitTabId {
    match session.app.shown_tab_for_test() {
        ShownTab::Git(id) => id,
        other => panic!("a Git tab is shown, not {other:?}"),
    }
}

fn open_git_view_of<P: Live>(session: &mut Session<P>, folder: &str) -> GitTabId {
    session.send(Message::ShowTab(ShownTab::Panels));
    go_to(session, folder);
    session.send(Message::OpenGitView);
    shown_git_tab(session)
}

#[test]
fn every_repository_gets_its_own_git_tab_and_ctrl_tab_walks_through_them() {
    let world = World::new("git-tabs", "");
    let parent = build_development_folder(&world.root);
    let mut session = world.boot_in(&parent);
    pump_until(&mut session, 20, git_scanned);

    let first = open_git_view_of(&mut session, "ncrs-demo");
    let second = open_git_view_of(&mut session, "kleine-tools");
    assert_ne!(first, second);
    assert_eq!(session.app.git_tab_ids_for_test(), [first, second]);

    let next = Modifiers::CTRL;
    session.press_with(Key::Named(Named::Tab), next);
    assert_eq!(session.app.shown_tab_for_test(), ShownTab::Panels);
    session.press_with(Key::Named(Named::Tab), next);
    assert_eq!(session.app.shown_tab_for_test(), ShownTab::Git(first));
    session.press_with(Key::Named(Named::Tab), next);
    assert_eq!(session.app.shown_tab_for_test(), ShownTab::Git(second));

    assert_eq!(open_git_view_of(&mut session, "ncrs-demo"), first);
    assert_eq!(session.app.git_tab_ids_for_test(), [first, second]);

    session.send(Message::GitView(first, ncrs_git_view::Message::Close));
    assert_eq!(session.app.git_tab_ids_for_test(), [second]);
    assert_eq!(session.app.shown_tab_for_test(), ShownTab::Panels);
}

#[test]
fn the_arrow_keys_move_the_cursor_of_the_notes_tab() {
    let world = World::new("notes-keys", "");
    for path in ["Haus/Heizung.md", "Haus/Strom.md", "Ideen.md"] {
        super::write(&world.notes.join(path), "# Titel\n\nText\n");
    }
    let mut session = world.boot();
    session.send(Message::OpenNotes);
    pump_until(&mut session, 10, |app| app.notes_for_test().loaded);
    assert!(session.app.notes_are_shown_for_test());

    let rows = session.app.notes_for_test().rows();
    assert!(rows.len() >= 2, "{rows:?}");
    session.press(Key::Named(Named::ArrowDown));
    assert_eq!(
        session.app.notes_for_test().cursor,
        Some(rows[0].key.clone())
    );
    session.press(Key::Named(Named::ArrowDown));
    assert_eq!(
        session.app.notes_for_test().cursor,
        Some(rows[1].key.clone())
    );
    session.press(Key::Named(Named::ArrowUp));
    assert_eq!(
        session.app.notes_for_test().cursor,
        Some(rows[0].key.clone())
    );

    // A folder opens with → and closes with ←.
    let folder = RowKey::Folder("Haus".into());
    assert_eq!(rows[0].key, folder);
    session.press(Key::Named(Named::ArrowRight));
    assert!(session.app.notes_for_test().rows().len() > rows.len());
    session.press(Key::Named(Named::ArrowLeft));
    assert_eq!(session.app.notes_for_test().rows(), rows);
}
