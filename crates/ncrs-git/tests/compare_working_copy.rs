//! Comparing a commit with the working copy reads the disk without following
//! links and without loading what the diff would refuse.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use git_cli::{repo, write};
use ncrs_git::compare_diff;

fn go() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[cfg(unix)]
#[test]
fn a_link_in_the_working_copy_shows_its_target_text_not_the_file_behind_it() {
    let repo = repo();
    repo.commit_file("a.txt", "plain\n", "add a");
    let head = repo.sh(&["rev-parse", "HEAD"]);
    write(repo.path(), "outside.txt", "secret\n");
    std::fs::remove_file(repo.path().join("a.txt")).unwrap();
    std::os::unix::fs::symlink("outside.txt", repo.path().join("a.txt")).unwrap();

    let diff = compare_diff(repo.path(), &head, None, "a.txt", None, false, &go()).unwrap();
    let shown = format!("{diff:?}");
    assert!(
        shown.contains("outside.txt"),
        "the link text is the content"
    );
    assert!(
        !shown.contains("secret"),
        "the file behind the link stays unread"
    );
}

#[cfg(unix)]
#[test]
fn a_path_through_a_linked_folder_has_no_content() {
    let repo = repo();
    let head = repo.sh(&["rev-parse", "HEAD"]);
    let elsewhere = tempfile::tempdir().unwrap();
    write(elsewhere.path(), "x.txt", "secret\n");
    std::os::unix::fs::symlink(elsewhere.path(), repo.path().join("linked")).unwrap();

    let files = ncrs_git::compare_files(repo.path(), &head, None, &go()).unwrap();
    assert!(
        files.iter().all(|file| !file.path.starts_with("linked/")),
        "files behind a linked folder are not listed"
    );
    let diff = compare_diff(repo.path(), &head, None, "linked/x.txt", None, false, &go()).unwrap();
    assert!(!format!("{diff:?}").contains("secret"));
}

#[test]
fn a_file_over_the_limit_is_reported_too_large() {
    let repo = repo();
    let head = repo.sh(&["rev-parse", "HEAD"]);
    write(repo.path(), "big.txt", &"line\n".repeat(1_100_000));

    let diff = compare_diff(repo.path(), &head, None, "big.txt", None, false, &go()).unwrap();
    assert!(diff.too_large);
}
