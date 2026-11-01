use crate::app::*;
use crate::fs::FileEntry;
use std::ffi::OsString;

fn app_with_conflict() -> (App, tempfile::TempDir) {
    let scratch = tempfile::Builder::new()
        .prefix("ncrs-conflict-keys-")
        .tempdir()
        .expect("a scratch directory");
    let left = scratch.path().join("left");
    let right = scratch.path().join("right");
    std::fs::create_dir_all(&left).unwrap();
    std::fs::create_dir_all(&right).unwrap();
    std::fs::write(left.join("a.txt"), b"new").unwrap();
    std::fs::write(right.join("a.txt"), b"old").unwrap();

    let mut app = App::with_environment(Environment::isolated()).0;
    app.panels.left.path = left.clone();
    app.panels.right.path = right;
    app.panels.left.entries = vec![FileEntry {
        name: OsString::from("a.txt"),
        path: left.join("a.txt"),
        is_dir: false,
        is_symlink: false,
        is_parent: false,
        size: 3,
        modified: None,
        folder_size: None,
    }];
    app.panels.left.selected = 0;
    (app, scratch)
}

fn press(app: &App, key: Key, modifiers: Modifiers) -> Option<Message> {
    route_key(&app.prompt_key_state(), key, modifiers)
}

/// The dialog has to be answerable without the mouse, and the keys are the
/// ones a user expects from a question.
#[test]
fn enter_overwrites_and_escape_keeps() {
    let (mut app, _scratch) = app_with_conflict();
    // The conflict is pending, so the keys answer it rather than the panels.
    app.ops.pending_conflict = Some(PendingConflict {
        transfer: Transfer {
            kind: TransferKind::Copy,
            sources: vec![PathBuf::from("/left/a.txt")],
            target: PathBuf::from("/right"),
            names: Vec::new(),
        },
        index: 0,
    });

    assert_eq!(
        press(&app, Key::Named(Named::Enter), Modifiers::default()),
        Some(Message::DialogActivate),
        "Enter does not press the focused button"
    );
    assert_eq!(
        press(&app, Key::Named(Named::Escape), Modifiers::default()),
        Some(Message::TransferConflict(ConflictChoice::ThisKeep)),
        "Escape does not answer the question"
    );
}

/// With the dialog up, Escape must not stop the job instead: the app is
/// waiting for an answer, and Escape means "no" to a question.
#[tokio::test]
async fn escape_does_not_stop_the_job_while_the_dialog_is_up() {
    let (mut app, _scratch) = app_with_conflict();
    let _started = app.start_transfer(TransferKind::Copy);
    app.ops.pending_conflict = Some(PendingConflict {
        transfer: Transfer {
            kind: TransferKind::Copy,
            sources: vec![PathBuf::from("/left/a.txt")],
            target: PathBuf::from("/right"),
            names: Vec::new(),
        },
        index: 0,
    });

    let message = press(&app, Key::Named(Named::Escape), Modifiers::default());
    assert!(
        !matches!(message, Some(Message::AbortJob)),
        "Escape stopped the copy instead of answering the question"
    );
}

/// On a Mac Ctrl+C cancels the whole operation — the key that aborts a job,
/// distinct from Escape so the two answers do not collide. Elsewhere Ctrl+C
/// is copy_files, which the dialog must not turn into a clipboard write.
#[test]
fn ctrl_c_cancels_the_operation_on_a_mac_and_does_nothing_elsewhere() {
    let (mut app, _scratch) = app_with_conflict();
    app.ops.pending_conflict = Some(PendingConflict {
        transfer: Transfer {
            kind: TransferKind::Copy,
            sources: vec![PathBuf::from("/left/a.txt")],
            target: PathBuf::from("/right"),
            names: Vec::new(),
        },
        index: 0,
    });

    let expected =
        cfg!(target_os = "macos").then(|| Message::TransferConflict(ConflictChoice::Cancel));
    assert_eq!(
        press(&app, Key::Character("c".into()), Modifiers::CTRL),
        expected,
        "Ctrl+C in the conflict dialog does the wrong thing"
    );
}

/// With no dialog up, the same keys mean what they always meant. Otherwise
/// the new branches would have swallowed normal navigation.
#[test]
fn without_a_dialog_the_keys_are_unchanged() {
    let (app, _scratch) = app_with_conflict();
    assert_eq!(
        press(&app, Key::Named(Named::Enter), Modifiers::default()),
        Some(Message::Panel(PanelMessage::OpenSelected)),
        "Enter no longer opens the selected entry"
    );
    assert_eq!(
        press(&app, Key::Named(Named::Escape), Modifiers::default()),
        None
    );
}
