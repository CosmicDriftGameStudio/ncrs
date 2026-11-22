//! What a hostile repository (an unzipped download) must not get the actions to do.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use git_cli::{numbered, repo, write};
use ncrs_git::{ActionError, ConflictChoice, Git, HunkSide};

#[cfg(unix)]
#[test]
fn a_symlinked_undo_folder_is_refused_and_nothing_behind_it_is_pruned() {
    let repo = repo();
    let outside = tempfile::tempdir().unwrap();
    for number in 0..15 {
        let folder = outside.path().join(format!("{number:030}"));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("keep"), "x").unwrap();
    }
    let git_dir = repo.path().join(".git");
    std::os::unix::fs::symlink(outside.path(), git_dir.join("ncrs-undo")).unwrap();
    write(repo.path(), "a.txt", "changed\n");

    repo.git.discard_file("a.txt").unwrap_err();
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 15);
}

#[cfg(unix)]
#[test]
fn pruning_leaves_symlinks_and_foreign_names_alone() {
    let repo = repo();
    let outside = tempfile::tempdir().unwrap();
    let undo_dir = repo.path().join(".git/ncrs-undo");
    std::fs::create_dir_all(&undo_dir).unwrap();
    std::os::unix::fs::symlink(outside.path(), undo_dir.join(format!("{:030}", 0))).unwrap();
    std::fs::create_dir_all(undo_dir.join("not-a-backup")).unwrap();
    for number in 1..=12 {
        std::fs::create_dir_all(undo_dir.join(format!("{number:030}"))).unwrap();
    }
    write(repo.path(), "a.txt", "changed\n");
    repo.git.discard_file("a.txt").unwrap();
    assert!(outside.path().exists());
    assert!(undo_dir.join("not-a-backup").exists());
}

#[cfg(unix)]
#[test]
fn undo_of_a_discard_does_not_write_through_a_symlink() {
    let repo = repo();
    write(repo.path(), "a.txt", "mine\n");
    let undo = repo.git.discard_file("a.txt").unwrap();
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join("victim.txt");
    std::fs::write(&victim, "precious\n").unwrap();
    std::fs::remove_file(repo.path().join("a.txt")).unwrap();
    std::os::unix::fs::symlink(&victim, repo.path().join("a.txt")).unwrap();

    repo.git.undo(&undo).unwrap_err();
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "precious\n");
}

#[test]
fn resolving_a_block_of_a_file_that_is_not_in_conflict_is_refused() {
    let repo = repo();
    write(
        repo.path(),
        "a.txt",
        "<<<<<<< x\na\n=======\nb\n>>>>>>> y\n",
    );
    repo.git
        .resolve_block("a.txt", 0, ConflictChoice::Ours)
        .unwrap_err();
}

#[cfg(unix)]
#[test]
fn a_hunk_of_a_file_with_a_quote_in_its_name_is_refused() {
    let repo = repo();
    let original = numbered(5, "\n");
    repo.commit_file("q\"x.txt", &original, "quoted");
    write(
        repo.path(),
        "q\"x.txt",
        &original.replace("line 3", "line 3 edited"),
    );
    let diff = repo.git.hunks("q\"x.txt", HunkSide::Unstaged).unwrap();
    assert!(matches!(
        repo.git.stage_hunk("q\"x.txt", &diff.hunks[0]),
        Err(ActionError::Other { .. })
    ));
}

#[cfg(unix)]
#[test]
fn reading_the_changes_runs_neither_hooks_nor_filters_of_the_repository() {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;

    let repo = repo();
    repo.commit_file("b.txt", "b\n", "second");
    let markers = tempfile::tempdir().unwrap();
    let filter_marker = markers.path().join("filter");
    let process_marker = markers.path().join("process");
    let hook_marker = markers.path().join("hook");
    write(repo.path(), ".gitattributes", "a.txt filter=x\n");
    let mut config = std::fs::OpenOptions::new()
        .append(true)
        .open(repo.path().join(".git/config"))
        .unwrap();
    write!(
        config,
        "[filter \"x\"]\n\tclean = touch '{}'\n\tprocess = touch '{}'\n",
        filter_marker.display(),
        process_marker.display()
    )
    .unwrap();
    let hook = repo.path().join(".git/hooks/post-index-change");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        format!("#!/bin/sh\ntouch '{}'\n", hook_marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    // The same size with other content, so git has to run the clean filter to compare; a tracked
    // file rewritten with the same content, so the index wants a refresh.
    write(repo.path(), "a.txt", "ONE\ntwo\nthree\n");
    write(repo.path(), "b.txt", "b\n");

    let reading = Git::open_reading(repo.path()).unwrap();
    reading.changes().unwrap();
    assert!(!filter_marker.exists());
    assert!(!process_marker.exists());
    assert!(!hook_marker.exists());
}
