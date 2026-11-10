use std::os::unix::fs::{symlink, PermissionsExt as _};

use super::*;

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
}

fn put(path: &Path, bits: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(bits)).unwrap();
}

fn run(chosen: &[PathBuf], plan: PermissionPlan) -> Outcome {
    let (sender, _receiver) = mpsc::unbounded_channel();
    change_permissions(chosen, &plan, &sender)
}

fn edit(set: u32, clear: u32) -> PermissionEdit {
    PermissionEdit {
        set,
        clear,
        read_only: None,
    }
}

fn file_with(dir: &Path, name: &str, bits: u32) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, "x").unwrap();
    put(&path, bits);
    path
}

#[test]
fn sets_and_clears_only_the_named_bits() {
    let dir = tempfile::tempdir().unwrap();
    let a = file_with(dir.path(), "a", 0o640);
    let b = file_with(dir.path(), "b", 0o666);
    let outcome = run(
        &[a.clone(), b.clone()],
        PermissionPlan {
            edit: edit(0o100, 0o002),
            recurse: None,
        },
    );
    assert_eq!(mode(&a), 0o740);
    assert_eq!(mode(&b), 0o764);
    assert_eq!(outcome.changed, 2);
    assert!(outcome.failures.is_empty());
}

#[test]
fn leaves_an_entry_alone_when_the_mode_already_fits() {
    let dir = tempfile::tempdir().unwrap();
    let a = file_with(dir.path(), "a", 0o644);
    let outcome = run(
        std::slice::from_ref(&a),
        PermissionPlan {
            edit: edit(0o004, 0o002),
            recurse: None,
        },
    );
    assert_eq!(outcome.changed, 0);
    assert_eq!(mode(&a), 0o644);
}

fn tree() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let top = dir.path().join("top");
    fs::create_dir(&top).unwrap();
    put(&top, 0o700);
    let inner = top.join("inner");
    fs::create_dir(&inner).unwrap();
    put(&inner, 0o700);
    let file = file_with(&inner, "f", 0o600);
    (dir, top, inner, file)
}

#[test]
fn recursion_applies_to_files_and_folders_and_without_execute_clears_x() {
    let (_dir, top, inner, file) = tree();
    put(&file, 0o700);
    let plan = |files, folders| PermissionPlan {
        edit: edit(0o044, 0),
        recurse: Some((files, folders)),
    };
    let outcome = run(
        std::slice::from_ref(&top),
        plan(ForFiles::WithoutExecute, ForFolders::AsAbove),
    );
    assert_eq!(mode(&top), 0o744);
    assert_eq!(mode(&inner), 0o744);
    assert_eq!(mode(&file), 0o644);
    assert_eq!(outcome.changed, 3);
}

#[test]
fn unchanged_files_and_folders_are_skipped_but_still_walked() {
    let (_dir, top, inner, file) = tree();
    let outcome = run(
        std::slice::from_ref(&top),
        PermissionPlan {
            edit: edit(0o004, 0),
            recurse: Some((ForFiles::AsAbove, ForFolders::Unchanged)),
        },
    );
    assert_eq!(mode(&top), 0o704);
    assert_eq!(mode(&inner), 0o700);
    assert_eq!(mode(&file), 0o604);
    assert_eq!(outcome.changed, 2);

    let second = run(
        std::slice::from_ref(&top),
        PermissionPlan {
            edit: edit(0o040, 0),
            recurse: Some((ForFiles::Unchanged, ForFolders::AsAbove)),
        },
    );
    assert_eq!(mode(&inner), 0o740);
    assert_eq!(mode(&file), 0o604);
    assert_eq!(second.changed, 2);
}

#[test]
fn links_are_neither_followed_nor_changed() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let hidden = file_with(&outside, "f", 0o600);
    put(&outside, 0o700);
    let top = dir.path().join("top");
    fs::create_dir(&top).unwrap();
    put(&top, 0o700);
    let folder_link = top.join("to_folder");
    let file_link = top.join("to_file");
    symlink(&outside, &folder_link).unwrap();
    symlink(&hidden, &file_link).unwrap();

    let plan = PermissionPlan {
        edit: edit(0o004, 0),
        recurse: Some((ForFiles::AsAbove, ForFolders::AsAbove)),
    };
    run(&[top.clone(), folder_link.clone()], plan);
    assert_eq!(mode(&hidden), 0o600);
    assert_eq!(mode(&outside), 0o700);
    assert_eq!(mode(&top), 0o704);
}

#[test]
fn a_folder_swapped_for_a_link_after_the_count_is_not_followed() {
    let (dir, top, inner, _file) = tree();
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let hidden = file_with(&outside, "f", 0o600);
    put(&outside, 0o700);
    let plan = PermissionPlan {
        edit: edit(0o004, 0),
        recurse: Some((ForFiles::AsAbove, ForFolders::AsAbove)),
    };
    let chosen = [top.clone()];
    let total = count_work(&chosen, &plan);
    assert_eq!(total, 3);

    fs::remove_dir_all(&inner).unwrap();
    symlink(&outside, &inner).unwrap();
    let (sender, _receiver) = mpsc::unbounded_channel();
    let outcome = apply_counted(&chosen, &plan, total, &sender);

    assert_eq!(mode(&hidden), 0o600);
    assert_eq!(mode(&outside), 0o700);
    assert_eq!(mode(&top), 0o704);
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
}
#[test]
fn a_failure_is_listed_with_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone");
    let outcome = run(
        std::slice::from_ref(&missing),
        PermissionPlan {
            edit: edit(0o004, 0),
            recurse: None,
        },
    );
    assert_eq!(outcome.changed, 0);
    assert_eq!(outcome.failures.len(), 1);
    assert_eq!(outcome.failures[0].path, missing);
}

#[test]
fn dropping_the_receiver_cancels() {
    let dir = tempfile::tempdir().unwrap();
    let a = file_with(dir.path(), "a", 0o600);
    let (sender, receiver) = mpsc::unbounded_channel();
    drop(receiver);
    let outcome = change_permissions(
        std::slice::from_ref(&a),
        &PermissionPlan {
            edit: edit(0o004, 0),
            recurse: None,
        },
        &sender,
    );
    assert!(outcome.cancelled);
    assert_eq!(mode(&a), 0o600);
}

#[test]
fn only_the_owner_read_and_execute_bits_are_cleared_after_the_walk() {
    let (now, later) = edit(0o004, 0o577).split_set_and_clear();
    assert_eq!((now.set, now.clear), (0o004, 0o077));
    assert_eq!((later.set, later.clear), (0, 0o500));

    let (_, owner_read_kept) = edit(0o400, 0o700).split_set_and_clear();
    assert_eq!(owner_read_kept.clear, 0o100);
}

#[test]
fn closing_a_tree_for_group_and_others_closes_every_folder() {
    let (_dir, top, inner, file) = tree();
    put(&top, 0o755);
    put(&inner, 0o755);
    put(&file, 0o644);
    let outcome = run(
        std::slice::from_ref(&top),
        PermissionPlan {
            edit: edit(0, 0o077),
            recurse: Some((ForFiles::AsAbove, ForFolders::AsAbove)),
        },
    );
    assert_eq!(mode(&top), 0o700);
    assert_eq!(mode(&inner), 0o700);
    assert_eq!(mode(&file), 0o600);
    assert_eq!(outcome.changed, 3);
}
