//! Files larger than what is read for a diff: shown as too large, and no
//! action writes back the part that was read.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use git_cli::{read, repo, write, Repo};
use ncrs_git::{ConflictChoice, HunkSide, IntegrateOutcome};

const FIVE_MIB_OF_LINES: usize = 5 * 1024 * 1024 / 8;

fn large(first: &str, last: &str) -> String {
    let middle = "0123456\n".repeat(FIVE_MIB_OF_LINES);
    format!("{first}\n{middle}{last}\n")
}

/// A merge that stopped at `c.txt`, with the conflict at the very end of it.
fn large_conflict() -> Repo {
    let repo = repo();
    repo.commit_file("c.txt", &large("top", "end"), "base");
    repo.git.create_branch("feature", "main", true).unwrap();
    repo.commit_file("c.txt", &large("top", "F"), "feature side");
    repo.git.checkout("main").unwrap();
    repo.commit_file("c.txt", &large("top", "M"), "main side");
    assert!(matches!(
        repo.git.merge("feature").unwrap(),
        IntegrateOutcome::Conflicts(_)
    ));
    repo
}

#[test]
fn a_large_file_shows_as_too_large_and_its_hunks_are_refused() {
    let repo = repo();
    repo.commit_file("big.txt", &large("top", "end"), "big");
    let changed = large("TOP", "end");
    write(repo.path(), "big.txt", &changed);
    let shown = repo.git.hunks("big.txt", HunkSide::Unstaged).unwrap();
    assert!(shown.too_large);

    write(repo.path(), "a.txt", "ONE\ntwo\nthree\n");
    let other = repo.git.hunks("a.txt", HunkSide::Unstaged).unwrap();
    let hunk = &other.hunks[0];
    repo.git.stage_hunk("big.txt", hunk).unwrap_err();
    repo.git.discard_hunk("big.txt", hunk).unwrap_err();
    assert_eq!(read(repo.path(), "big.txt"), changed);
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "");

    repo.git.stage(&["big.txt"]).unwrap();
    repo.git.unstage_hunk("big.txt", hunk).unwrap_err();
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "big.txt");
}

#[test]
fn a_large_file_in_conflict_is_left_to_the_editor() {
    let repo = large_conflict();
    let before = read(repo.path(), "c.txt");
    assert!(before.contains("<<<<<<< "));
    repo.git
        .resolve_block("c.txt", 0, ConflictChoice::Ours)
        .unwrap_err();
    repo.git
        .resolve_file("c.txt", ConflictChoice::Ours)
        .unwrap_err();
    repo.git.mark_resolved("c.txt").unwrap_err();
    assert_eq!(read(repo.path(), "c.txt"), before, "nothing was cut off");
    assert_eq!(repo.git.conflicts().unwrap().len(), 1);

    write(repo.path(), "c.txt", &large("top", "both"));
    repo.git.mark_resolved("c.txt").unwrap();
    assert!(repo.git.conflicts().unwrap().is_empty());
    assert_eq!(read(repo.path(), "c.txt"), large("top", "both"));
}

#[test]
fn the_text_of_a_large_file_is_not_handed_out_in_part() {
    let repo = repo();
    write(repo.path(), "big.txt", &large("top", "end"));
    assert_eq!(ncrs_git::read_working_text(repo.path(), "big.txt"), None);
    assert_eq!(
        ncrs_git::read_working_text(repo.path(), "a.txt").as_deref(),
        Some("one\ntwo\nthree\n")
    );
}
