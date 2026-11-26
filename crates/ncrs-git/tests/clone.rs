//! Cloning into a folder: the new repository is there, and bad addresses never reach git.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use git_cli::with_origin;
use ncrs_git::ActionError;

#[test]
fn a_clone_lands_in_a_folder_named_after_the_address() {
    let (origin, repo) = with_origin();
    repo.commit_file("a.txt", "a\n", "first");
    repo.sh(&["push", "-q"]);
    let parent = tempfile::tempdir().unwrap();
    let url = origin.path().to_string_lossy().into_owned();

    let folder = ncrs_git::clone(&url, parent.path()).unwrap();

    assert_eq!(folder.parent(), Some(parent.path()));
    assert!(folder.join(".git").exists());
    assert!(folder.join("a.txt").exists());
}

#[test]
fn an_address_that_looks_like_an_option_is_refused() {
    let parent = tempfile::tempdir().unwrap();
    for url in ["", "--upload-pack=touch x", "https://host/.git"] {
        assert!(
            matches!(
                ncrs_git::clone(url, parent.path()),
                Err(ActionError::InvalidName { .. })
            ),
            "{url:?}"
        );
    }
}

#[test]
fn a_clone_of_nothing_fails_without_a_prompt() {
    let parent = tempfile::tempdir().unwrap();
    // Not inside `parent`: Git for Windows makes the target first, and a target
    // that is the missing address itself is then cloned as an empty repository.
    let elsewhere = tempfile::tempdir().unwrap();
    let missing = elsewhere.path().join("nowhere");
    ncrs_git::clone(&missing.to_string_lossy(), parent.path()).unwrap_err();
}
