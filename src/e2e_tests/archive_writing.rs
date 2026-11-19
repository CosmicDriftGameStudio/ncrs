use super::*;
use crate::fs::archive::fixtures::{write_archive, Item};
use crate::fs::archive::locate_in_archive;
use crate::transfer_dialog::{Policy, TransferDialogMessage};
use iced::keyboard::key::Named;

/// The names directly inside `inner` of `archive`, sorted, with sizes.
fn listed(archive: &Path, inner: &str) -> Vec<(String, u64)> {
    let place = if inner.is_empty() {
        archive.to_path_buf()
    } else {
        archive.join(inner)
    };
    let location = locate_in_archive(&place).expect("a place in the archive");
    let mut found: Vec<_> = crate::fs::archive::list(&location)
        .expect("a listing")
        .entries
        .iter()
        .filter(|entry| !entry.is_parent)
        .map(|entry| (entry.name.to_string_lossy().into_owned(), entry.size))
        .collect();
    found.sort();
    found
}

/// Opens `name` in the right panel, then gives the left panel the focus back.
fn open_in_right_panel<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
    name: &str,
) {
    session.press(Key::Named(Named::Tab));
    for _ in 0..64 {
        if selected(&session.app, PanelSide::Right).as_deref() == Some(name) {
            break;
        }
        session.press(Key::Named(Named::ArrowDown));
    }
    assert_eq!(
        selected(&session.app, PanelSide::Right).as_deref(),
        Some(name)
    );
    session.press(Key::Named(Named::Enter));
    session.press(Key::Named(Named::Tab));
}

#[test]
fn alt_f5_packs_the_selection_into_a_new_archive_in_the_other_panel() {
    let dir = scratch("pack");
    write(&dir.path().join("alpha.txt"), "abc");
    let mut session = session_in(dir.path());
    cursor_to(&mut session, "alpha.txt");

    session.press_with(Key::Named(Named::F5), Modifiers::ALT);
    session.press(Key::Named(Named::Enter));

    let archive = dir.path().join("home").join("alpha.txt.zip");
    assert_eq!(listed(&archive, ""), [("alpha.txt".to_string(), 3)]);
    assert!(session.app.job_is_idle());
    assert!(session.app.job_error_for_test().is_none());
    assert_eq!(
        selected(&session.app, PanelSide::Right).as_deref(),
        Some("alpha.txt.zip")
    );
}

#[test]
fn a_pack_name_that_is_taken_or_not_an_archive_starts_no_job() {
    let dir = scratch("pack-name");
    write(&dir.path().join("alpha.txt"), "abc");
    write(&dir.path().join("home").join("alpha.txt.zip"), "taken");
    let mut session = session_in(dir.path());
    cursor_to(&mut session, "alpha.txt");

    session.press_with(Key::Named(Named::F5), Modifiers::ALT);
    session.press(Key::Named(Named::Enter));

    assert!(session.app.job_is_idle());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("home").join("alpha.txt.zip")).unwrap(),
        "taken"
    );
}

#[test]
fn f5_adds_the_file_to_the_archive_in_the_other_panel() {
    let dir = scratch("copy-in");
    write(&dir.path().join("alpha.txt"), "abc");
    let archive = dir.path().join("home").join("box.zip");
    write_archive(&archive, &[Item::File("old.txt", b"old")]);
    let mut session = session_in(dir.path());
    open_in_right_panel(&mut session, "box.zip");
    cursor_to(&mut session, "alpha.txt");

    session.press(Key::Named(Named::F5));
    session.press(Key::Named(Named::Enter));

    assert_eq!(
        listed(&archive, ""),
        [("alpha.txt".to_string(), 3), ("old.txt".to_string(), 3)]
    );
    assert!(dir.path().join("alpha.txt").is_file(), "the source went");
    assert!(session.app.job_error_for_test().is_none());
    assert!(names(&session.app, PanelSide::Right).contains(&"alpha.txt".to_string()));
}

#[test]
fn a_name_in_the_way_stops_ask_and_overwrite_replaces_it() {
    let dir = scratch("copy-in-conflict");
    write(&dir.path().join("alpha.txt"), "abc");
    let archive = dir.path().join("home").join("box.zip");
    write_archive(&archive, &[Item::File("alpha.txt", b"x")]);
    let mut session = session_in(dir.path());
    open_in_right_panel(&mut session, "box.zip");
    cursor_to(&mut session, "alpha.txt");
    let before = std::fs::read(&archive).unwrap();

    session.press(Key::Named(Named::F5));
    session.press(Key::Named(Named::Enter));
    assert!(session.app.job_error_for_test().is_some());
    assert_eq!(std::fs::read(&archive).unwrap(), before);

    session.press(Key::Named(Named::F5));
    session.send(Message::TransferDialog(TransferDialogMessage::SetPolicy(
        Policy::Overwrite,
    )));
    session.press(Key::Named(Named::Enter));
    assert_eq!(listed(&archive, ""), [("alpha.txt".to_string(), 3)]);
}

#[test]
fn f8_in_an_archive_removes_the_entry() {
    let dir = scratch("delete-in");
    let archive = dir.path().join("box.zip");
    write_archive(
        &archive,
        &[Item::File("a.txt", b"a"), Item::File("b.txt", b"bb")],
    );
    let mut session = session_in(dir.path());
    cursor_to(&mut session, "box.zip");
    session.press(Key::Named(Named::Enter));
    cursor_to(&mut session, "a.txt");

    session.press(Key::Named(Named::F8));
    assert!(session.app.delete_dialog_is_open());
    assert_eq!(listed(&archive, "").len(), 2, "deleted before the answer");
    // A permanent delete starts on Cancel; ArrowLeft moves to Delete.
    session.press(Key::Named(Named::ArrowLeft));
    session.press(Key::Named(Named::Enter));

    assert_eq!(listed(&archive, ""), [("b.txt".to_string(), 2)]);
    assert!(session.app.job_error_for_test().is_none());
    assert!(!names(&session.app, PanelSide::Left).contains(&"a.txt".to_string()));
}

// The blocking write starts when its task is built and notices Escape
// only at its next tick, so whether a small write lands is a race the
// abort does not promise to win (see `escape_stops_a_running_copy`).
// What holds either way: the queue is free at once, and the archive is
// whole or absent, never half written. Cleaning up the partial file on a
// cancel is tested on the writer itself.
#[test]
fn escape_during_a_pack_frees_the_queue_and_leaves_no_broken_archive() {
    let dir = scratch("pack-abort");
    write(&dir.path().join("alpha.txt"), "abc");
    let mut session = session_in(dir.path());
    cursor_to(&mut session, "alpha.txt");
    session.press_with(Key::Named(Named::F5), Modifiers::ALT);
    let held = session.send_held_after_check(Message::PromptSubmit);
    assert!(!session.app.job_is_idle());

    session.press(Key::Named(Named::Escape));
    assert!(
        session.app.job_is_idle(),
        "the pack still runs after Escape"
    );
    session.drive(held);
    session.settle();

    assert!(session.app.job_is_idle(), "the aborted job came back");
    let archive = dir.path().join("home").join("alpha.txt.zip");
    if archive.exists() {
        assert_eq!(listed(&archive, ""), [("alpha.txt".to_string(), 3)]);
    }
}

#[test]
fn escape_during_a_copy_in_frees_the_queue_and_leaves_the_archive_whole() {
    let dir = scratch("copy-in-abort");
    write(&dir.path().join("alpha.txt"), "abc");
    let archive = dir.path().join("home").join("box.zip");
    write_archive(&archive, &[Item::File("old.txt", b"old")]);
    let mut session = session_in(dir.path());
    open_in_right_panel(&mut session, "box.zip");
    cursor_to(&mut session, "alpha.txt");
    session.press(Key::Named(Named::F5));
    let held =
        session.send_held_after_check(Message::TransferDialog(TransferDialogMessage::Confirm));
    assert!(!session.app.job_is_idle());

    session.press(Key::Named(Named::Escape));
    assert!(
        session.app.job_is_idle(),
        "the copy still runs after Escape"
    );
    session.drive(held);
    session.settle();

    assert!(session.app.job_is_idle(), "the aborted job came back");
    let old = ("old.txt".to_string(), 3);
    let listing = listed(&archive, "");
    assert!(
        listing == [old.clone()] || listing == [("alpha.txt".to_string(), 3), old],
        "{listing:?}"
    );
}

#[test]
fn f6_into_an_archive_is_still_refused() {
    let dir = scratch("move-in");
    write(&dir.path().join("alpha.txt"), "abc");
    let archive = dir.path().join("home").join("box.zip");
    write_archive(&archive, &[Item::File("old.txt", b"old")]);
    let before = std::fs::read(&archive).unwrap();
    let mut session = session_in(dir.path());
    open_in_right_panel(&mut session, "box.zip");
    cursor_to(&mut session, "alpha.txt");

    session.press(Key::Named(Named::F6));
    session.press(Key::Named(Named::Enter));

    assert_eq!(std::fs::read(&archive).unwrap(), before);
    assert!(dir.path().join("alpha.txt").is_file(), "the source went");
    assert!(session.app.job_is_idle());
}
