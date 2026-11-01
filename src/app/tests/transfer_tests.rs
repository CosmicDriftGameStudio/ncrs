use crate::app::*;
use crate::fs::FileEntry;
use std::ffi::OsString;

fn file(name: &str) -> FileEntry {
    FileEntry {
        name: OsString::from(name),
        path: PathBuf::from("/left").join(name),
        is_dir: false,
        is_symlink: false,
        is_parent: false,
        size: 1,
        modified: None,
        folder_size: None,
    }
}

/// An app with three files in the left panel, the right one empty and
/// ready to receive.
fn app_with_files() -> App {
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = PathBuf::from("/left");
    app.panels.right.path = PathBuf::from("/right");
    app.panels.left.entries = vec![file("a.txt"), file("b.txt"), file("c.txt")];
    app
}

/// NC's rule: nothing tagged means the row under the cursor. F5 on one file
/// has to work or the key does nothing.
#[tokio::test]
async fn an_untagged_panel_transfers_the_cursor_row() {
    let mut app = app_with_files();
    app.panels.left.selected = 1;

    let _task = app.start_transfer(TransferKind::Copy);

    let transfer = app.ops.transfer.as_ref().expect("F5 set up nothing");
    assert_eq!(transfer.sources, vec![PathBuf::from("/left/b.txt")]);
    assert_eq!(transfer.target, PathBuf::from("/right"));
}

fn dialog_app(root: &Path) -> (App, PathBuf, PathBuf) {
    let from = root.join("from");
    let to = root.join("to");
    std::fs::create_dir_all(&from).unwrap();
    std::fs::create_dir_all(&to).unwrap();
    std::fs::write(from.join("a.txt"), "a").unwrap();
    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = from.clone();
    app.panels.right.path = to.clone();
    let mut entry = file("a.txt");
    entry.path = from.join("a.txt");
    app.panels.left.entries = vec![entry];
    (app, from, to)
}

fn dialog_message(message: TransferDialogMessage) -> Message {
    Message::TransferDialog(message)
}

#[tokio::test]
async fn f5_asks_first_and_starts_nothing() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, _, to) = dialog_app(root.path());
    let _task = app.update(Message::Transfer(TransferKind::Copy));
    let dialog = app.ops.transfer_dialog.as_ref().expect("no confirmation");
    assert_eq!(dialog.target_path(), to);
    assert!(app.ops.transfer.is_none());
    assert!(!app.jobs.is_busy());
}

#[tokio::test]
async fn confirming_starts_the_transfer_with_the_chosen_policy() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, _, to) = dialog_app(root.path());
    let _open = app.update(Message::Transfer(TransferKind::Copy));
    let _policy = app.update(dialog_message(TransferDialogMessage::SetPolicy(
        Policy::Overwrite,
    )));
    let _confirm = app.update(dialog_message(TransferDialogMessage::Confirm));
    assert!(app.ops.transfer_dialog.is_none());
    let transfer = app.ops.transfer.as_ref().expect("nothing started");
    assert_eq!(transfer.target, std::fs::canonicalize(&to).unwrap());
    assert_eq!(
        app.ops.conflict_rule,
        Some(fs::transfer::OnConflict::Overwrite)
    );
}

#[tokio::test]
async fn asking_leaves_no_standing_answer() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, _, _) = dialog_app(root.path());
    let _open = app.update(Message::Transfer(TransferKind::Move));
    let _confirm = app.update(dialog_message(TransferDialogMessage::Confirm));
    assert!(app.ops.transfer.is_some());
    assert_eq!(app.ops.conflict_rule, None);
}

#[tokio::test]
async fn a_target_inside_the_selection_is_refused_in_the_dialog() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, from, _) = dialog_app(root.path());
    let inside = from.join("a.txt").join("deeper");
    let _open = app.update(Message::Transfer(TransferKind::Copy));
    let _typed = app.update(dialog_message(TransferDialogMessage::TargetInput(
        inside.display().to_string(),
    )));
    let _confirm = app.update(dialog_message(TransferDialogMessage::Confirm));
    assert!(app.ops.transfer.is_none());
    let dialog = app.ops.transfer_dialog.as_ref().expect("dialog closed");
    assert!(dialog.refusal().is_some());
}

#[tokio::test]
async fn tab_switches_between_copy_and_move_and_escape_closes() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, _, _) = dialog_app(root.path());
    let _open = app.update(Message::Transfer(TransferKind::Copy));
    let tab = route_key(
        &app.prompt_key_state(),
        Key::Named(Named::Tab),
        Modifiers::default(),
    );
    assert_eq!(tab, Some(dialog_message(TransferDialogMessage::ToggleKind)));
    let _toggled = app.update(tab.unwrap());
    assert_eq!(
        app.ops.transfer_dialog.as_ref().unwrap().kind(),
        TransferKind::Move
    );

    let escape = route_key(
        &app.prompt_key_state(),
        Key::Named(Named::Escape),
        Modifiers::default(),
    );
    assert_eq!(escape, Some(dialog_message(TransferDialogMessage::Cancel)));
    let _closed = app.update(escape.unwrap());
    assert!(app.ops.transfer_dialog.is_none());
    assert!(app.ops.transfer.is_none());
}

#[tokio::test]
async fn f5_during_a_transfer_brings_the_progress_back() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, _, _) = dialog_app(root.path());
    let _open = app.update(Message::Transfer(TransferKind::Copy));
    let _confirm = app.update(dialog_message(TransferDialogMessage::Confirm));
    let _hidden = app.update(dialog_message(TransferDialogMessage::Background));
    assert!(!app.ops.progress_mask_is_open());
    let _again = app.update(Message::Transfer(TransferKind::Copy));
    assert!(app.ops.progress_mask_is_open());
    assert!(app.ops.transfer_dialog.is_none());
}

/// Tagged rows win over the cursor. This is the whole point of tagging: the
/// cursor is wherever the user last looked, and must not change what F5
/// touches.
#[tokio::test]
async fn tagged_rows_win_over_the_cursor() {
    let mut app = app_with_files();
    app.panels.left.selected = 2;
    let left = app.panels.left.path.clone();
    app.panels.left.selection.toggle(&left.join("a.txt"));
    app.panels.left.selection.toggle(&left.join("c.txt"));

    let _task = app.start_transfer(TransferKind::Copy);

    let transfer = app.ops.transfer.as_ref().expect("F5 set up nothing");
    assert_eq!(
        transfer.sources,
        vec![PathBuf::from("/left/a.txt"), PathBuf::from("/left/c.txt")],
        "the tagged rows, in the order they appear, not the cursor row"
    );
}

/// `..` is not a thing to copy into the other panel.
#[test]
fn the_parent_entry_is_never_transferred() {
    let mut app = app_with_files();
    app.panels
        .left
        .entries
        .get_mut(1)
        .expect("the fixture has three files")
        .is_parent = true;
    app.panels.left.selected = 1;

    let _task = app.start_transfer(TransferKind::Copy);

    assert!(
        app.ops.transfer.is_none(),
        "F5 offered to copy .. into the other panel"
    );
}

/// The target is the other panel, not the same one. Copying a directory
/// into itself is the mistake this prevents.
#[tokio::test]
async fn the_target_is_the_other_panel() {
    let mut app = app_with_files();
    app.panels.left.selected = 0;

    let _task = app.start_transfer(TransferKind::Copy);

    let transfer = app.ops.transfer.as_ref().expect("F5 set up nothing");
    assert_eq!(transfer.target, app.panels.right.path);
    assert_ne!(transfer.target, transfer.sources[0].parent().unwrap());
}

/// Switching the active panel swaps which way the transfer goes.
#[tokio::test]
async fn the_target_follows_the_active_panel() {
    let mut app = app_with_files();
    app.panels.active = PanelSide::Right;
    // A file in the right panel, so the path matches the panel it is in.
    let mut x = file("x.txt");
    x.path = PathBuf::from("/right/x.txt");
    app.panels.right.entries = vec![x];
    app.panels.right.selected = 0;

    let _task = app.start_transfer(TransferKind::Copy);

    let transfer = app.ops.transfer.as_ref().expect("F5 set up nothing");
    // The source comes from the active panel, which is now the right one,
    // and the target is the other.
    assert_eq!(transfer.sources, vec![PathBuf::from("/right/x.txt")]);
    assert_eq!(transfer.target, PathBuf::from("/left"));
}
