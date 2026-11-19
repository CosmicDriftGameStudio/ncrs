use super::trash::{trash_session, SHIFT_F8};
use crate::e2e_tests::*;
use crate::keymap::KeymapMessages as _;
use iced::keyboard::key::Named;

/// Deleting one file and answering the dialog reloads through the real read
/// task; the cursor must end up on the file that moved up into its place.
fn delete_under_the_cursor(label: &str, name: &str) -> Option<String> {
    let dir = scratch(label);
    for file in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"] {
        write(&dir.path().join(file), file);
    }
    let (_bin, mut session) = trash_session(dir.path());
    cursor_to(&mut session, name);

    session.press(Key::Named(Named::F8));
    session.press(Key::Named(Named::Enter));

    assert!(!dir.path().join(name).exists(), "{name} was not deleted");
    selected(&session.app, PanelSide::Left)
}

#[test]
fn deleting_the_middle_file_leaves_the_cursor_on_the_next_one() {
    assert_eq!(
        delete_under_the_cursor("del-cursor-mid", "c.txt").as_deref(),
        Some("d.txt")
    );
}

#[test]
fn deleting_the_last_file_leaves_the_cursor_on_the_new_last() {
    assert_eq!(
        delete_under_the_cursor("del-cursor-last", "e.txt").as_deref(),
        Some("d.txt")
    );
}

#[test]
fn escape_cancels_the_delete() {
    let dir = scratch("del-escape");
    write(&dir.path().join("alpha.txt"), "a");
    let (bin, mut session) = trash_session(dir.path());
    cursor_to(&mut session, "alpha.txt");

    session.press(Key::Named(Named::F8));
    session.press(Key::Named(Named::Escape));

    assert!(!session.app.delete_dialog_is_open(), "the dialog stayed up");
    assert!(dir.path().join("alpha.txt").is_file(), "Escape deleted it");
    assert!(!bin.path().join("alpha.txt").exists());
}

/// Enter is the reflex; in the permanent dialog it must not be a delete.
#[test]
fn shift_f8_then_enter_deletes_nothing() {
    let dir = scratch("perm-enter");
    write(&dir.path().join("alpha.txt"), "a");
    let (bin, mut session) = trash_session(dir.path());
    cursor_to(&mut session, "alpha.txt");

    session.press_with(Key::Named(SHIFT_F8.0), SHIFT_F8.1);
    assert!(
        session.app.delete_dialog_is_open(),
        "Shift+F8 asked nothing"
    );
    session.press(Key::Named(Named::Enter));

    assert!(!session.app.delete_dialog_is_open(), "the dialog stayed up");
    assert!(dir.path().join("alpha.txt").is_file(), "Enter deleted it");
    assert!(!bin.path().join("alpha.txt").exists());
}

/// Space and Shift+Down tag without an Insert key, and F8 then takes every
/// tagged entry.
#[test]
fn space_and_shift_down_tag_and_f8_deletes_them_all() {
    let dir = scratch("tag-keys");
    for name in ["alpha.txt", "beta.txt", "gamma.txt", "omega.txt"] {
        write(&dir.path().join(name), "x");
    }
    let (bin, mut session) = trash_session(dir.path());
    cursor_to(&mut session, "alpha.txt");
    session.press(Key::Named(Named::Space));
    session.press_with(Key::Named(Named::ArrowDown), Modifiers::SHIFT);

    session.press(Key::Named(Named::F8));
    session.press(Key::Named(Named::Enter));

    for name in ["alpha.txt", "beta.txt"] {
        assert!(!dir.path().join(name).exists(), "{name} still there");
        assert!(bin.path().join(name).is_file(), "{name} not in the trash");
    }
    for name in ["gamma.txt", "omega.txt"] {
        assert!(dir.path().join(name).is_file(), "{name} went too");
    }
}

/// Focus moved to "Delete" on purpose: Enter deletes, a held Enter does not.
#[test]
fn the_permanent_dialog_needs_a_deliberate_focus_change_and_a_fresh_enter() {
    let dir = scratch("perm-focus");
    write(&dir.path().join("alpha.txt"), "a");
    let (_bin, mut session) = trash_session(dir.path());
    cursor_to(&mut session, "alpha.txt");

    session.press_with(Key::Named(SHIFT_F8.0), SHIFT_F8.1);
    session.press(Key::Named(Named::ArrowLeft));
    session.press_repeat(Key::Named(Named::Enter), Modifiers::default());
    assert!(
        dir.path().join("alpha.txt").is_file() && session.app.delete_dialog_is_open(),
        "a held Enter pressed the button"
    );

    session.press(Key::Named(Named::Enter));
    assert!(
        !dir.path().join("alpha.txt").exists(),
        "Enter on the focused Delete did not delete"
    );
}

/// Tab and Shift+Tab walk the three conflict buttons; Enter presses the
/// focused one.
#[test]
fn the_conflict_dialog_is_answered_with_focus_and_enter() {
    for (keys, expected) in [
        (vec![(Named::Tab, Modifiers::default())], "new"),
        (vec![(Named::Tab, Modifiers::SHIFT)], "old"),
        (
            vec![
                (Named::ArrowRight, Modifiers::default()),
                (Named::ArrowRight, Modifiers::default()),
            ],
            "old",
        ),
        (vec![], "old"),
    ] {
        let dir = scratch("conflict-focus");
        let home = dir.path().join("home");
        write(&dir.path().join("alpha.txt"), "new");
        write(&home.join("alpha.txt"), "old");
        let mut session = session_in(dir.path());
        cursor_to(&mut session, "alpha.txt");
        session.press(Key::Named(Named::F5));
        session.press(Key::Named(Named::Enter));
        for (named, modifiers) in &keys {
            session.press_with(Key::Named(*named), *modifiers);
        }
        session.press(Key::Named(Named::Enter));

        assert_eq!(
            std::fs::read_to_string(home.join("alpha.txt")).expect("the file"),
            expected,
            "after {keys:?}"
        );
        assert!(!session.app.conflict_is_pending(), "after {keys:?}");
    }
}

/// Confirming Shift+F8 deletes for good: not in the trash either.
#[test]
fn shift_f8_confirmed_deletes_permanently() {
    let dir = scratch("perm-confirm");
    write(&dir.path().join("alpha.txt"), "a");
    write(&dir.path().join("beta.txt"), "b");
    let (bin, mut session) = trash_session(dir.path());

    cursor_to(&mut session, "alpha.txt");
    session.press_with(Key::Named(SHIFT_F8.0), SHIFT_F8.1);
    session.press_with(Key::Named(SHIFT_F8.0), SHIFT_F8.1);
    assert!(
        !dir.path().join("alpha.txt").exists(),
        "chord did not delete"
    );

    // The button does the same as the chord.
    cursor_to(&mut session, "beta.txt");
    session.press_with(Key::Named(SHIFT_F8.0), SHIFT_F8.1);
    session.send(Message::DeleteConfirm);
    assert!(
        !dir.path().join("beta.txt").exists(),
        "button did not delete"
    );

    assert!(
        std::fs::read_dir(bin.path())
            .expect("the trash")
            .next()
            .is_none(),
        "a permanent delete went through the trash"
    );
}

/// A link is removed itself; what it points at is not touched.
#[cfg(unix)]
#[test]
fn a_link_to_a_directory_outside_is_deleted_without_touching_the_target() {
    let dir = scratch("perm-link");
    let outside = scratch("perm-outside");
    write(&outside.path().join("files/keep.txt"), "k");
    let link = dir.path().join("shortcut");
    std::os::unix::fs::symlink(outside.path().join("files"), &link).expect("a symlink");
    let (_bin, mut session) = trash_session(dir.path());
    cursor_to(&mut session, "shortcut");

    session.press_with(Key::Named(SHIFT_F8.0), SHIFT_F8.1);
    session.press_with(Key::Named(SHIFT_F8.0), SHIFT_F8.1);

    assert!(
        std::fs::symlink_metadata(&link).is_err(),
        "the link remains"
    );
    assert!(
        outside.path().join("files/keep.txt").is_file(),
        "the delete followed the link into the target"
    );
}

/// No silent fallback: a refused trash leaves the file, says so, and
/// points at Shift+F8.
#[test]
fn a_refused_trash_deletes_nothing_and_names_shift_f8() {
    let dir = scratch("trash-refused");
    write(&dir.path().join("alpha.txt"), "a");
    let mut session = session_with_trash(dir.path(), std::sync::Arc::new(RefusingTrash));
    cursor_to(&mut session, "alpha.txt");

    session.press(Key::Named(Named::F8));
    session.press(Key::Named(Named::Enter));

    assert!(dir.path().join("alpha.txt").is_file(), "deleted anyway");
    let message = session.app.job_error_for_test().expect("no error shown");
    let permanent_key = crate::keymap::Keymap::built_in()
        .shortcut_label(&Message::Delete { permanent: true })
        .expect("the permanent delete has a key");
    assert!(
        message.contains("alpha.txt") && message.contains(&permanent_key),
        "the error does not name the entry and the way out: {message}"
    );
}

/// `..` is not an entry, so there is nothing to ask about.
#[test]
fn f8_on_the_parent_row_opens_no_dialog() {
    let dir = scratch("del-parent");
    write(&dir.path().join("alpha.txt"), "a");
    let (_bin, mut session) = trash_session(dir.path());
    assert_eq!(
        selected(&session.app, PanelSide::Left).as_deref(),
        Some("..")
    );

    session.press(Key::Named(Named::F8));
    session.press_with(Key::Named(SHIFT_F8.0), SHIFT_F8.1);

    assert!(!session.app.delete_dialog_is_open(), "a dialog for ..");
}

/// Holding Shift+F8 must not answer the dialog it opened.
#[test]
fn a_held_shift_f8_does_not_confirm_the_permanent_delete() {
    let dir = scratch("perm-held");
    write(&dir.path().join("alpha.txt"), "a");
    let (_bin, mut session) = trash_session(dir.path());
    cursor_to(&mut session, "alpha.txt");

    session.press_with(Key::Named(SHIFT_F8.0), SHIFT_F8.1);
    session.press_repeat(Key::Named(SHIFT_F8.0), SHIFT_F8.1);
    session.press_repeat(Key::Named(SHIFT_F8.0), SHIFT_F8.1);

    assert!(
        dir.path().join("alpha.txt").is_file(),
        "the repeat deleted it"
    );
    assert!(
        session.app.delete_dialog_is_open(),
        "the dialog was answered"
    );
}

/// Navigation still repeats: holding the arrow key keeps moving.
#[test]
fn a_held_arrow_key_keeps_moving() {
    let dir = scratch("held-arrow");
    write(&dir.path().join("alpha.txt"), "a");
    write(&dir.path().join("beta.txt"), "b");
    let mut session = session_in(dir.path());
    let start = selected(&session.app, PanelSide::Left);

    session.press_repeat(Key::Named(Named::ArrowDown), Modifiers::default());
    let after_one = selected(&session.app, PanelSide::Left);
    session.press_repeat(Key::Named(Named::ArrowDown), Modifiers::default());
    let after_two = selected(&session.app, PanelSide::Left);

    assert_ne!(start, after_one, "the first repeat did not move");
    assert_ne!(after_one, after_two, "the second repeat did not move");
}

/// Alt+Enter is not Enter, and Ctrl+F5 is not F5.
#[test]
fn modified_keys_do_not_trigger_the_bare_action() {
    let dir = scratch("mods");
    let home = dir.path().join("home");
    write(&dir.path().join("alpha.txt"), "a");
    let mut session = session_in(dir.path());
    cursor_to(&mut session, "alpha.txt");

    session.press_with(Key::Named(Named::F5), Modifiers::CTRL);

    assert!(!home.join("alpha.txt").exists(), "Ctrl+F5 copied");
}

// --- F3 view / F4 edit ---
