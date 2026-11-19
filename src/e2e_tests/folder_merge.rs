//! Folder merge and a cancel half way through, on trees of 50 to 100 entries
//! at the real disk, driven by the real keys and the real conflict dialog.

#![cfg(unix)]

use std::path::{Path, PathBuf};

use iced::keyboard::key::Named;
use iced::keyboard::Key;

use super::{cursor_to, scratch, session_in, write, Session};
use crate::app::App;
use crate::messages::{ConflictChoice, Message};

fn tag_and_copy(
    session: &mut Session<impl iced::Program<State = App, Message = Message> + 'static>,
    rows: &[String],
) {
    for row in rows {
        cursor_to(session, row);
        session.press(Key::Named(Named::Insert));
    }
    session.press(Key::Named(Named::F5));
    session.press(Key::Named(Named::Enter));
}

fn files_below(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}

fn no_staging_left(root: &Path) {
    let left: Vec<PathBuf> = files_below(root)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".ncrs-partial"))
        })
        .collect();
    assert!(left.is_empty(), "staging files left behind: {left:?}");
}

/// 80 files in the source folder (20 of them one level down), 40 of those
/// already in the target with other content, and 20 files only the target has.
fn big_folder_pair() -> (tempfile::TempDir, PathBuf) {
    let dir = scratch("merge-big");
    let source = dir.path().join("photos");
    let target = dir.path().join("home/photos");
    for index in 0..80 {
        let place = if index >= 60 { "sub/" } else { "" };
        write(
            &source.join(format!("{place}f{index}.jpg")),
            &format!("new{index}"),
        );
    }
    for index in 0..40 {
        write(
            &target.join(format!("f{index}.jpg")),
            &format!("old{index}"),
        );
    }
    for index in 0..20 {
        write(&target.join(format!("only{index}.txt")), "target only");
    }
    (dir, target)
}

fn source_name() -> Vec<String> {
    vec!["photos".to_owned()]
}

#[test]
fn merging_a_big_folder_keeps_what_only_the_target_has() {
    let (dir, target) = big_folder_pair();
    let mut session = session_in(dir.path());
    tag_and_copy(&mut session, &source_name());
    assert!(
        session.app.conflict_is_pending(),
        "no question about the folder"
    );

    session.send(Message::from(ncrs_ops::OpsMessage::TransferConflict(
        ConflictChoice::ThisOverwrite,
    )));

    assert!(!session.app.conflict_is_pending());
    for index in 0..80 {
        let place = if index >= 60 { "sub/" } else { "" };
        assert_eq!(
            std::fs::read_to_string(target.join(format!("{place}f{index}.jpg"))).unwrap(),
            format!("new{index}"),
        );
    }
    for index in 0..20 {
        assert_eq!(
            std::fs::read_to_string(target.join(format!("only{index}.txt"))).unwrap(),
            "target only",
            "a file only the target had was lost"
        );
    }
    assert_eq!(files_below(&target).len(), 100);
    no_staging_left(dir.path());
}

#[test]
fn keeping_a_big_folder_leaves_the_target_as_it_was() {
    let (dir, target) = big_folder_pair();
    let mut session = session_in(dir.path());
    tag_and_copy(&mut session, &source_name());
    assert!(session.app.conflict_is_pending());

    session.send(Message::from(ncrs_ops::OpsMessage::TransferConflict(
        ConflictChoice::ThisKeep,
    )));

    for index in 0..40 {
        assert_eq!(
            std::fs::read_to_string(target.join(format!("f{index}.jpg"))).unwrap(),
            format!("old{index}"),
        );
    }
    for index in 0..20 {
        assert!(target.join(format!("only{index}.txt")).is_file());
    }
    no_staging_left(dir.path());
}

#[test]
fn cancelling_the_question_about_a_big_folder_changes_nothing() {
    let (dir, target) = big_folder_pair();
    let before = files_below(&target).len();
    let mut session = session_in(dir.path());
    tag_and_copy(&mut session, &source_name());
    assert!(session.app.conflict_is_pending());

    session.send(Message::from(ncrs_ops::OpsMessage::TransferConflict(
        ConflictChoice::Cancel,
    )));

    assert!(!session.app.conflict_is_pending());
    assert!(session.app.job_is_idle());
    assert_eq!(files_below(&target).len(), before);
    assert_eq!(
        std::fs::read_to_string(target.join("f0.jpg")).unwrap(),
        "old0"
    );
    no_staging_left(dir.path());
}

#[test]
fn a_cancel_in_the_middle_of_fifty_files_keeps_the_done_and_the_untouched() {
    let dir = scratch("cancel-fifty");
    let home = dir.path().join("home");
    let names: Vec<String> = (0..50).map(|index| format!("a{index:03}.txt")).collect();
    for name in &names {
        write(&dir.path().join(name), "new");
    }
    write(&home.join("a025.txt"), "old");
    let mut session = session_in(dir.path());
    tag_and_copy(&mut session, &names);
    assert!(session.app.conflict_is_pending(), "no question about a025");

    session.send(Message::from(ncrs_ops::OpsMessage::TransferConflict(
        ConflictChoice::Cancel,
    )));

    assert!(session.app.job_is_idle());
    for name in &names[..25] {
        assert_eq!(
            std::fs::read_to_string(home.join(name)).unwrap(),
            "new",
            "{name}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(home.join("a025.txt")).unwrap(),
        "old"
    );
    for name in &names[26..] {
        assert!(
            !home.join(name).exists(),
            "{name} was copied after the cancel"
        );
    }
    for name in &names {
        assert_eq!(
            std::fs::read_to_string(dir.path().join(name)).unwrap(),
            "new",
            "a source was touched"
        );
    }
    no_staging_left(dir.path());
}
