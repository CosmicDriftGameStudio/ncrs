use crate::e2e_tests::*;
use crate::messages::ConflictChoice;
use crate::messages::OpsMessage;
use iced::keyboard::key::Named;

/// The panels have to be pointing at the scratch tree before a key means
/// anything. `App::starting_in` is given the scratch tree for the left panel and its
/// `home` subdirectory for the right one.
#[test]
fn the_panels_load_the_working_directory() {
    let dir = scratch("panels");
    write(&dir.path().join("alpha.txt"), "a");
    write(&dir.path().join("beta.txt"), "b");

    let session = session_in(dir.path());

    assert_eq!(
        session.app.panel(PanelSide::Left).path,
        std::fs::canonicalize(dir.path()).expect("a canonical path"),
        "the left panel did not load the working directory"
    );
    assert!(
        names(&session.app, PanelSide::Left).contains(&"alpha.txt".to_string()),
        "the left panel shows {:?}",
        names(&session.app, PanelSide::Left)
    );
}

/// The first reported symptom: selecting rows did nothing. A key has to
/// reach `route_key` and move the cursor.
///
/// Asserted on the *name* under the cursor rather than on a row count: the
/// order depends on the fixture, and a test that counted presses would
/// break every time the fixture changed.
#[test]
fn arrow_keys_move_the_cursor() {
    let dir = scratch("cursor");
    write(&dir.path().join("alpha.txt"), "a");
    write(&dir.path().join("beta.txt"), "b");

    let mut session = session_in(dir.path());
    assert_eq!(
        selected(&session.app, PanelSide::Left),
        Some("..".to_string()),
        "the panel should start on the parent row; it holds {:?}",
        names(&session.app, PanelSide::Left)
    );

    let first = {
        session.press(Key::Named(Named::ArrowDown));
        let after = selected(&session.app, PanelSide::Left);
        assert_ne!(after.as_deref(), Some(".."), "the cursor did not move");
        after.expect("a row is selected")
    };

    session.press(Key::Named(Named::ArrowDown));
    let second = selected(&session.app, PanelSide::Left).expect("a row is selected");
    assert_ne!(
        second, first,
        "the second press did not move the cursor off {first:?}"
    );

    // And back up again, so the key is not merely advancing one way.
    session.press(Key::Named(Named::ArrowUp));
    assert_eq!(
        selected(&session.app, PanelSide::Left),
        Some(first),
        "the cursor did not move back"
    );
}

/// The second reported symptom: copying threw the focus away and lost the
/// tags. A copy has to move the file *and* leave the cursor where it was.
///
/// The panels start on the working directory and on `$HOME`, so the copy
/// target is `$HOME`. That is not something to do to the developer's
/// machine, so the session points `$HOME` at the scratch tree first.
#[test]
fn a_copy_moves_the_file_and_keeps_the_cursor_and_the_tags() {
    let dir = scratch("copy");
    let home = dir.path().join("home");
    write(&dir.path().join("alpha.txt"), "a");
    write(&dir.path().join("beta.txt"), "b");

    let mut session = session_in(dir.path());
    cursor_to(&mut session, "alpha.txt");
    session.press(Key::Named(Named::Insert));
    // Insert moves on to the next row; the copy must leave the cursor
    // wherever it is, so put it back on alpha.txt first.
    session.press(Key::Named(Named::ArrowUp));
    session.press(Key::Named(Named::F5));
    session.press(Key::Named(Named::Enter));

    assert!(
        home.join("alpha.txt").is_file(),
        "F5 did not copy alpha.txt"
    );
    assert!(
        dir.path().join("alpha.txt").is_file(),
        "copying removed the source"
    );
    assert_eq!(
        selected(&session.app, PanelSide::Left),
        Some("alpha.txt".to_string()),
        "the copy moved the cursor; the panel holds {:?}",
        names(&session.app, PanelSide::Left)
    );
    assert!(
        session.app.panel(PanelSide::Left).selection.any_tagged(),
        "the copy dropped the tags"
    );
}

/// Move is copy plus delete. If the source is still there, F6 is a copy.
#[test]
fn a_move_removes_the_source() {
    let dir = scratch("move");
    let home = dir.path().join("home");
    write(&dir.path().join("alpha.txt"), "a");

    let mut session = session_in(dir.path());
    cursor_to(&mut session, "alpha.txt");
    session.press(Key::Named(Named::F6));
    session.press(Key::Named(Named::Enter));

    assert!(
        home.join("alpha.txt").is_file(),
        "F6 did not move alpha.txt"
    );
    assert!(
        !dir.path().join("alpha.txt").exists(),
        "F6 left the source in place"
    );
}

/// The third reported symptom: no dialog on overwrite. Copying onto a name
/// that is taken has to ask, and must not touch the existing file until it
/// does.
#[test]
fn copying_onto_a_taken_name_asks_before_overwriting() {
    let dir = scratch("conflict");
    let home = dir.path().join("home");
    write(&dir.path().join("alpha.txt"), "new");
    write(&home.join("alpha.txt"), "old");

    let mut session = session_in(dir.path());
    cursor_to(&mut session, "alpha.txt");
    session.press(Key::Named(Named::F5));
    session.press(Key::Named(Named::Enter));

    assert_eq!(
        std::fs::read_to_string(home.join("alpha.txt")).expect("the old file"),
        "old",
        "the existing file was overwritten without asking"
    );
    assert!(
        session.app.conflict_is_pending(),
        "no conflict was raised, so the user is never asked"
    );
}

/// Answering the dialog has to actually resume the copy. Without this the
/// dialog would appear and then do nothing, which is what the user saw.
#[test]
fn answering_the_dialog_overwrites() {
    let dir = scratch("answer");
    let home = dir.path().join("home");
    write(&dir.path().join("alpha.txt"), "new");
    write(&home.join("alpha.txt"), "old");

    let mut session = session_in(dir.path());
    cursor_to(&mut session, "alpha.txt");
    session.press(Key::Named(Named::F5));
    session.press(Key::Named(Named::Enter));
    // Enter presses the focused button, which is "Keep"; Tab moves to
    // "Overwrite".
    session.press(Key::Named(Named::Tab));
    session.press(Key::Named(Named::Enter));

    assert_eq!(
        std::fs::read_to_string(home.join("alpha.txt")).expect("the file"),
        "new",
        "answering the dialog did not overwrite"
    );
}

/// Escape means "keep", and keeping is the answer that does not lose data.
#[test]
fn answering_the_dialog_with_keep_leaves_the_target_alone() {
    let dir = scratch("keep");
    let home = dir.path().join("home");
    write(&dir.path().join("alpha.txt"), "new");
    write(&home.join("alpha.txt"), "old");

    let mut session = session_in(dir.path());
    cursor_to(&mut session, "alpha.txt");
    session.press(Key::Named(Named::F5));
    session.press(Key::Named(Named::Enter));
    session.press(Key::Named(Named::Escape));

    assert_eq!(
        std::fs::read_to_string(home.join("alpha.txt")).expect("the old file"),
        "old",
        "keeping still overwrote the target"
    );
    assert!(
        !session.app.conflict_is_pending(),
        "the dialog opened again on the file that was just kept"
    );
    assert!(session.app.job_is_idle(), "the transfer never ended");
}

/// Tags the named rows, in the order given, and starts `key` on them.
fn tag_and_press(
    session: &mut Session<impl iced::Program<State = App, Message = Message> + 'static>,
    rows: &[&str],
    key: Named,
) {
    for row in rows {
        cursor_to(session, row);
        session.press(Key::Named(Named::Insert));
    }
    session.press(Key::Named(key));
    session.press(Key::Named(Named::Enter));
}

/// Keep skips one file and the transfer goes on with the next.
#[test]
fn keeping_one_file_goes_on_to_the_next() {
    let dir = scratch("keep-next");
    let home = dir.path().join("home");
    write(&dir.path().join("alpha.txt"), "new");
    write(&dir.path().join("beta.txt"), "b");
    write(&home.join("alpha.txt"), "old");

    let mut session = session_in(dir.path());
    tag_and_press(&mut session, &["alpha.txt", "beta.txt"], Named::F5);
    assert!(session.app.conflict_is_pending(), "no question about alpha");
    session.press(Key::Named(Named::Escape));

    assert!(
        !session.app.conflict_is_pending(),
        "asked about alpha twice"
    );
    assert_eq!(
        std::fs::read_to_string(home.join("alpha.txt")).unwrap(),
        "old"
    );
    assert!(
        home.join("beta.txt").is_file(),
        "the transfer stopped after keep"
    );
}

/// "Keep all" answers every later conflict the same way, without asking.
#[test]
fn keep_all_does_not_ask_again() {
    let dir = scratch("keep-all");
    let home = dir.path().join("home");
    for name in ["alpha.txt", "beta.txt", "gamma.txt"] {
        write(&dir.path().join(name), "new");
        write(&home.join(name), "old");
    }
    write(&dir.path().join("zeta.txt"), "z");

    let mut session = session_in(dir.path());
    tag_and_press(
        &mut session,
        &["alpha.txt", "beta.txt", "gamma.txt", "zeta.txt"],
        Named::F5,
    );
    session.send(Message::from(ncrs_ops::OpsMessage::TransferConflict(
        ConflictChoice::AllKeep,
    )));

    assert!(
        !session.app.conflict_is_pending(),
        "asked again after keep all"
    );
    for name in ["alpha.txt", "beta.txt", "gamma.txt"] {
        assert_eq!(std::fs::read_to_string(home.join(name)).unwrap(), "old");
    }
    assert!(home.join("zeta.txt").is_file(), "the rest was not copied");
}

/// Four rows, three of them already in the target, the dialog up on the
/// first conflict, and "for all files" ticked with Space.
fn all_ticked_on_three_conflicts() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    Session<impl iced::Program<State = App, Message = Message> + 'static>,
) {
    let dir = scratch("for-all");
    let home = dir.path().join("home");
    for name in ["alpha.txt", "beta.txt", "gamma.txt"] {
        write(&dir.path().join(name), "new");
        write(&home.join(name), "old");
    }
    write(&dir.path().join("zeta.txt"), "z");
    let mut session = session_in(dir.path());
    for row in ["alpha.txt", "beta.txt", "gamma.txt", "zeta.txt"] {
        cursor_to(&mut session, row);
        session.press(Key::Named(Named::Insert));
    }
    session.press(Key::Named(Named::F5));
    session.press(Key::Named(Named::Enter));
    assert!(session.app.conflict_is_pending(), "no question about alpha");
    session.press(Key::Named(Named::Space));
    (dir, home, session)
}

/// The tick is wired: Keep with it set keeps every later conflict too,
/// without a second question.
#[test]
fn keep_with_for_all_ticked_does_not_ask_again() {
    let (_dir, home, mut session) = all_ticked_on_three_conflicts();
    session.press(Key::Named(Named::Escape));

    assert!(!session.app.conflict_is_pending(), "asked again");
    for name in ["alpha.txt", "beta.txt", "gamma.txt"] {
        assert_eq!(std::fs::read_to_string(home.join(name)).unwrap(), "old");
    }
    assert!(home.join("zeta.txt").is_file(), "the rest was not copied");
}

#[test]
fn overwrite_with_for_all_ticked_does_not_ask_again() {
    let (_dir, home, mut session) = all_ticked_on_three_conflicts();
    session.press(Key::Named(Named::Tab));
    session.press(Key::Named(Named::Enter));

    assert!(!session.app.conflict_is_pending(), "asked again");
    for name in ["alpha.txt", "beta.txt", "gamma.txt"] {
        assert_eq!(std::fs::read_to_string(home.join(name)).unwrap(), "new");
    }
    assert!(home.join("zeta.txt").is_file());
}

/// Cancel ends the whole transfer: the rows after the conflict are not
/// touched and the dialog does not come back.
#[test]
fn cancel_ends_the_transfer() {
    let dir = scratch("cancel-dialog");
    let home = dir.path().join("home");
    write(&dir.path().join("alpha.txt"), "new");
    write(&dir.path().join("beta.txt"), "b");
    write(&home.join("alpha.txt"), "old");

    let mut session = session_in(dir.path());
    tag_and_press(&mut session, &["alpha.txt", "beta.txt"], Named::F5);
    // The abort key on a Mac; elsewhere it has none, and the dialog's
    // Cancel button is the way.
    if cfg!(target_os = "macos") {
        session.press_with(Key::Character("c".into()), Modifiers::CTRL);
    } else {
        session.send(Message::from(ncrs_ops::OpsMessage::TransferConflict(
            ConflictChoice::Cancel,
        )));
    }

    assert!(!session.app.conflict_is_pending(), "the dialog is still up");
    assert!(session.app.job_is_idle());
    assert_eq!(
        std::fs::read_to_string(home.join("alpha.txt")).unwrap(),
        "old"
    );
    assert!(!home.join("beta.txt").exists(), "cancel went on copying");
}

/// A multi-file transfer that fails on a later row still shows the rows
/// that were done: the panels are reloaded.
#[cfg(unix)]
#[test]
fn a_failure_midway_reloads_the_panels() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("fail-midway");
    write(&dir.path().join("alpha.txt"), "a");
    write(&dir.path().join("beta.txt"), "b");
    std::fs::set_permissions(
        dir.path().join("beta.txt"),
        std::fs::Permissions::from_mode(0o000),
    )
    .unwrap();

    let mut session = session_in(dir.path());
    tag_and_press(&mut session, &["alpha.txt", "beta.txt"], Named::F5);

    assert!(
        names(&session.app, PanelSide::Right).contains(&"alpha.txt".to_string()),
        "the target panel does not show the file that was copied; it holds {:?}",
        names(&session.app, PanelSide::Right)
    );
}

/// Escape stops a running job — the fourth reported symptom was that the
/// queue could not be steered at all.
///
/// Deterministic: F5 is sent but its task is held back, so the job is
/// registered and certainly not finished when Escape arrives. The queue
/// must be idle right after the key, which only an abort achieves; a copy
/// left running would still hold its slot. The aborted task is released
/// afterwards and must not revive anything.
///
/// The file itself is not asserted: the blocking copy starts when the task
/// is built and stops only at its next progress tick, so whether a small
/// file lands is a race the abort does not promise to win.
#[test]
fn escape_stops_a_running_copy() {
    let dir = scratch("abort");
    write(&dir.path().join("alpha.txt"), "a");

    let mut session = session_in(dir.path());
    cursor_to(&mut session, "alpha.txt");
    session.send(Message::Transfer(crate::messages::TransferKind::Copy));
    let held = session.send_held_after_check(Message::Ops(OpsMessage::TransferDialog(
        crate::transfer_dialog::TransferDialogMessage::Confirm,
    )));
    assert!(!session.app.job_is_idle(), "F5 did not start a job");

    session.press(Key::Named(Named::Escape));
    assert!(
        session.app.job_is_idle(),
        "the copy is still running after Escape"
    );

    session.drive(held);
    session.settle();
    assert!(session.app.job_is_idle(), "the aborted job came back");
    assert!(!session.app.conflict_is_pending());
}

/// F7 creates a directory. Typing needs two separate abilities — a key
/// that carries a character, and text typed as a burst — and both go
/// through the app's real routing rather than a message.
#[test]
fn f7_creates_a_directory_and_selects_it() {
    let dir = scratch("mkdir");

    let mut session = session_in(dir.path());
    session.press(Key::Named(Named::F7));
    session.type_text("neuer ordner");
    session.press(Key::Named(Named::Enter));

    assert!(
        dir.path().join("neuer ordner").is_dir(),
        "F7 did not create the directory; the panel holds {:?}",
        names(&session.app, PanelSide::Left)
    );
    assert_eq!(
        selected(&session.app, PanelSide::Left),
        Some("neuer ordner".to_string()),
        "the new directory was not selected"
    );
}

/// Escape closes the prompt without creating anything. A prompt that could
/// only be left by typing a valid name would be a trap.
#[test]
fn escape_closes_the_prompt_without_creating_anything() {
    let dir = scratch("cancel-mkdir");

    let mut session = session_in(dir.path());
    session.press(Key::Named(Named::F7));
    session.type_text("weg damit");
    session.press(Key::Named(Named::Escape));

    assert!(
        !dir.path().join("weg damit").exists(),
        "Escape created the directory anyway"
    );
    assert!(
        !names(&session.app, PanelSide::Left).contains(&"weg damit".to_string()),
        "the prompt is still up: the panel holds {:?}",
        names(&session.app, PanelSide::Left)
    );
}

/// Tagging is what a multi-file copy is built on.
#[test]
fn insert_tags_the_row_under_the_cursor() {
    let dir = scratch("tag");
    write(&dir.path().join("alpha.txt"), "a");
    write(&dir.path().join("beta.txt"), "b");

    let mut session = session_in(dir.path());
    session.press(Key::Named(Named::ArrowDown));
    session.press(Key::Named(Named::Insert));

    assert!(
        session.app.panel(PanelSide::Left).selection.any_tagged(),
        "Insert tagged nothing; the panel holds {:?}",
        names(&session.app, PanelSide::Left)
    );
}
