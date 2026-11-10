use std::os::unix::fs::PermissionsExt as _;

use super::*;

#[test]
fn a_folder_with_mode_000_is_made_readable_and_walked() {
    let dir = tempfile::tempdir().unwrap();
    let top = dir.path().join("top");
    let locked = top.join("locked");
    fs::create_dir_all(&locked).unwrap();
    let file = locked.join("f");
    fs::write(&file, "x").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let (sender, _receiver) = mpsc::unbounded_channel();
    let outcome = change_permissions(
        std::slice::from_ref(&top),
        &PermissionPlan {
            edit: PermissionEdit {
                set: 0o700,
                clear: 0,
                read_only: None,
            },
            recurse: Some((ForFiles::AsAbove, ForFolders::AsAbove)),
        },
        &sender,
    );
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    assert_eq!(mode(&locked), 0o700);
    assert_eq!(mode(&file), 0o700);
}

fn mode_of(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

#[test]
fn removing_read_and_execute_from_folders_still_reaches_everything_below() {
    let dir = tempfile::tempdir().unwrap();
    let top = dir.path().join("top");
    let inner = top.join("inner");
    fs::create_dir_all(&inner).unwrap();
    let file = inner.join("f");
    fs::write(&file, "x").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    let (sender, _receiver) = mpsc::unbounded_channel();
    let outcome = change_permissions(
        std::slice::from_ref(&top),
        &PermissionPlan {
            edit: PermissionEdit {
                set: 0,
                clear: 0o500,
                read_only: None,
            },
            recurse: Some((ForFiles::AsAbove, ForFolders::AsAbove)),
        },
        &sender,
    );
    let open_up = |path: &Path| {
        let before = mode_of(path);
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        before
    };
    let top_mode = open_up(&top);
    let inner_mode = open_up(&inner);
    let file_mode = mode_of(&file);
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    assert_eq!(top_mode & 0o500, 0);
    assert_eq!(inner_mode & 0o500, 0);
    assert_eq!(file_mode, 0o244);
    assert_eq!(outcome.changed, 3);
}

#[test]
fn a_folder_that_cannot_be_searched_is_a_failure_not_silence() {
    let dir = tempfile::tempdir().unwrap();
    let top = dir.path().join("top");
    fs::create_dir_all(&top).unwrap();
    fs::write(top.join("f"), "x").unwrap();
    fs::set_permissions(&top, fs::Permissions::from_mode(0o600)).unwrap();
    let (sender, _receiver) = mpsc::unbounded_channel();
    let outcome = change_permissions(
        std::slice::from_ref(&top),
        &PermissionPlan {
            edit: PermissionEdit::default(),
            recurse: Some((ForFiles::AsAbove, ForFolders::Unchanged)),
        },
        &sender,
    );
    fs::set_permissions(&top, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(outcome.failures.len(), 1, "{:?}", outcome.failures);
    assert!(outcome
        .failures
        .first()
        .is_some_and(|failure| failure.path.ends_with("f")));
}
