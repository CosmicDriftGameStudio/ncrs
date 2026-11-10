use std::os::unix::fs::PermissionsExt as _;

use super::*;

#[test]
fn a_file_with_mode_000_can_be_made_readable_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked");
    fs::write(&path, "x").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let (sender, _receiver) = mpsc::unbounded_channel();
    let outcome = change_permissions(
        std::slice::from_ref(&path),
        &PermissionPlan {
            edit: PermissionEdit {
                set: 0o004,
                clear: 0,
                read_only: None,
            },
            recurse: None,
        },
        &sender,
    );
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    assert_eq!(mode, 0o004);
}

#[test]
fn a_file_with_mode_000_in_a_folder_whose_bits_already_fit_is_no_failure() {
    let dir = tempfile::tempdir().unwrap();
    let top = dir.path().join("top");
    fs::create_dir(&top).unwrap();
    fs::set_permissions(&top, fs::Permissions::from_mode(0o755)).unwrap();
    let file = top.join("locked");
    fs::write(&file, "x").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
    let (sender, _receiver) = mpsc::unbounded_channel();
    let outcome = change_permissions(
        std::slice::from_ref(&top),
        &PermissionPlan {
            edit: PermissionEdit {
                set: 0o111,
                clear: 0,
                read_only: None,
            },
            recurse: Some((ForFiles::WithoutExecute, ForFolders::AsAbove)),
        },
        &sender,
    );
    let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o7777;
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    assert_eq!(mode, 0o000);
}
