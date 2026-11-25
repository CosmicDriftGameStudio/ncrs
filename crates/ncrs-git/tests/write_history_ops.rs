//! Tag, cherry-pick, revert, reset and detached checkout, with their undo.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use git_cli::{read, repo, write, Repo};
use ncrs_git::{ActionError, IntegrateOutcome, Integration, ResetMode};

/// `main` has a.txt, and a branch `side` carries one extra commit that adds s.txt.
fn with_side_commit() -> (Repo, String) {
    let repo = repo();
    repo.git.create_branch("side", "main", true).unwrap();
    repo.commit_file("s.txt", "side\n", "side work");
    let sha = repo.sh(&["rev-parse", "HEAD"]);
    repo.git.checkout("main").unwrap();
    (repo, sha)
}

#[test]
fn checkout_detached_leaves_the_branch() {
    let (repo, sha) = with_side_commit();
    repo.git.checkout_detached(&sha).unwrap();
    assert_eq!(repo.git.current_branch(), None);
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), sha);
}

#[test]
fn a_tag_is_made_and_undone_and_a_bad_name_is_refused() {
    let (repo, sha) = with_side_commit();
    let undo = repo.git.create_tag("v1", &sha).unwrap();
    assert_eq!(repo.sh(&["rev-parse", "v1"]), sha);
    repo.git.undo(&undo).unwrap();
    assert_eq!(repo.sh(&["tag"]), "");

    repo.git.create_tag("bad name", &sha).unwrap_err();
    repo.git.create_tag("a..b", &sha).unwrap_err();
    repo.git.create_tag("-x", &sha).unwrap_err();
    repo.git.create_tag("v1", &sha).unwrap();
    assert!(
        repo.git.create_tag("v1", &sha).is_err(),
        "an existing tag stays"
    );
}

#[test]
fn undoing_a_tag_that_moved_is_refused() {
    let (repo, sha) = with_side_commit();
    let undo = repo.git.create_tag("v1", &sha).unwrap();
    repo.sh(&["tag", "-f", "v1", "main"]);
    assert!(matches!(
        repo.git.undo(&undo),
        Err(ActionError::UndoNotSafe(_))
    ));
}

#[test]
fn a_clean_cherry_pick_makes_a_commit_and_undoes() {
    let (repo, sha) = with_side_commit();
    let before = repo.sh(&["rev-parse", "HEAD"]);
    let picked = repo.git.cherry_pick(&sha).unwrap();
    assert_eq!(picked.outcome, IntegrateOutcome::Completed);
    assert_eq!(read(repo.path(), "s.txt"), "side\n");
    assert_eq!(repo.sh(&["log", "-1", "--format=%s"]), "side work");

    repo.git.undo(&picked.undo.unwrap()).unwrap();
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), before);
}

#[test]
fn a_conflicting_cherry_pick_stops_and_can_be_aborted_or_continued() {
    let repo = repo();
    repo.git.create_branch("side", "main", true).unwrap();
    repo.commit_file("a.txt", "one\nSIDE\nthree\n", "side edit");
    let sha = repo.sh(&["rev-parse", "HEAD"]);
    repo.git.checkout("main").unwrap();
    repo.commit_file("a.txt", "one\nMAIN\nthree\n", "main edit");
    let before = repo.sh(&["rev-parse", "HEAD"]);

    let picked = repo.git.cherry_pick(&sha).unwrap();
    assert!(matches!(picked.outcome, IntegrateOutcome::Conflicts(_)));
    assert!(picked.undo.is_none());
    assert_eq!(
        repo.git.integration_detail().map(|found| found.kind),
        Some(Integration::CherryPick)
    );

    repo.git.abort_integration().unwrap();
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), before);
    assert!(repo.git.integration_detail().is_none());

    repo.git.cherry_pick(&sha).unwrap();
    write(repo.path(), "a.txt", "one\nBOTH\nthree\n");
    repo.sh(&["add", "a.txt"]);
    assert_eq!(
        repo.git.continue_integration().unwrap(),
        IntegrateOutcome::Completed
    );
    assert_eq!(read(repo.path(), "a.txt"), "one\nBOTH\nthree\n");
}

#[test]
fn a_merge_commit_is_refused_for_pick_and_revert() {
    let (repo, _) = with_side_commit();
    repo.sh(&["merge", "--no-ff", "-q", "-m", "merge side", "side"]);
    let merge = repo.sh(&["rev-parse", "HEAD"]);
    repo.git.cherry_pick(&merge).unwrap_err();
    repo.git.revert(&merge).unwrap_err();
}

#[test]
fn revert_takes_a_change_back_and_undoes() {
    let repo = repo();
    repo.commit_file("b.txt", "b\n", "add b");
    let sha = repo.sh(&["rev-parse", "HEAD"]);
    let before = sha.clone();
    let reverted = repo.git.revert(&sha).unwrap();
    assert_eq!(reverted.outcome, IntegrateOutcome::Completed);
    assert!(!repo.path().join("b.txt").exists());
    repo.git.undo(&reverted.undo.unwrap()).unwrap();
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), before);
}

#[test]
fn a_conflicting_revert_can_be_aborted() {
    let repo = repo();
    repo.commit_file("a.txt", "one\nTWO\nthree\n", "edit two");
    let sha = repo.sh(&["rev-parse", "HEAD"]);
    repo.commit_file("a.txt", "one\nOTHER\nthree\n", "edit again");
    let before = repo.sh(&["rev-parse", "HEAD"]);
    let reverted = repo.git.revert(&sha).unwrap();
    assert!(matches!(reverted.outcome, IntegrateOutcome::Conflicts(_)));
    assert_eq!(
        repo.git.integration_detail().map(|found| found.kind),
        Some(Integration::Revert)
    );
    repo.git.abort_integration().unwrap();
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), before);
}

/// Two more commits on main, with a staged new file and an edit to a tracked one.
fn busy_repo() -> (Repo, String) {
    let repo = repo();
    let base = repo.sh(&["rev-parse", "HEAD"]);
    repo.commit_file("b.txt", "b\n", "add b");
    repo.commit_file("a.txt", "one\ntwo\nthree\nfour\n", "grow a");
    write(repo.path(), "a.txt", "one\nEDITED\nthree\nfour\n");
    write(repo.path(), "new.txt", "new\n");
    repo.sh(&["add", "new.txt"]);
    write(repo.path(), "loose.txt", "untracked\n");
    (repo, base)
}

#[test]
fn soft_reset_keeps_files_and_index_and_undoes() {
    let (repo, base) = busy_repo();
    let head = repo.sh(&["rev-parse", "HEAD"]);
    let undo = repo.git.reset(ResetMode::Soft, &base).unwrap();
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), base);
    assert_eq!(
        repo.sh(&["diff", "--cached", "--name-only"]),
        "a.txt\nb.txt\nnew.txt"
    );
    repo.git.undo(&undo).unwrap();
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), head);
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "new.txt");
}

#[test]
fn mixed_reset_unstages_and_undoes() {
    let (repo, base) = busy_repo();
    let head = repo.sh(&["rev-parse", "HEAD"]);
    let undo = repo.git.reset(ResetMode::Mixed, &base).unwrap();
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "");
    assert_eq!(read(repo.path(), "a.txt"), "one\nEDITED\nthree\nfour\n");
    repo.git.undo(&undo).unwrap();
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), head);
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "new.txt");
}

#[test]
fn hard_reset_throws_changes_away_and_the_undo_brings_them_back() {
    let (repo, base) = busy_repo();
    let head = repo.sh(&["rev-parse", "HEAD"]);
    let loss = repo.git.reset_loss(&base).unwrap();
    assert_eq!(
        (loss.commits, loss.changed_files),
        (2, 2),
        "a.txt edited and new.txt staged; loose.txt is untracked"
    );

    let undo = repo.git.reset(ResetMode::Hard, &base).unwrap();
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), base);
    assert!(
        !repo.path().join("new.txt").exists(),
        "a staged-only file is deleted by --hard"
    );
    assert_eq!(read(repo.path(), "loose.txt"), "untracked\n");

    repo.git.undo(&undo).unwrap();
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), head);
    assert_eq!(read(repo.path(), "a.txt"), "one\nEDITED\nthree\nfour\n");
    assert_eq!(read(repo.path(), "new.txt"), "new\n");
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "new.txt");
}

#[test]
fn the_undo_of_a_reset_is_refused_once_head_moved() {
    let (repo, base) = busy_repo();
    let undo = repo.git.reset(ResetMode::Mixed, &base).unwrap();
    repo.commit_file("c.txt", "c\n", "moved on");
    assert!(matches!(
        repo.git.undo(&undo),
        Err(ActionError::UndoNotSafe(_))
    ));
}

#[cfg(unix)]
#[test]
fn a_hard_reset_will_not_follow_a_symlink() {
    let repo = repo();
    let base = repo.sh(&["rev-parse", "HEAD"]);
    repo.commit_file("b.txt", "b\n", "add b");
    write(repo.path(), "outside.txt", "outside\n");
    std::fs::remove_file(repo.path().join("b.txt")).unwrap();
    std::os::unix::fs::symlink(repo.path().join("outside.txt"), repo.path().join("b.txt")).unwrap();
    repo.git.reset(ResetMode::Hard, &base).unwrap_err();
    assert_eq!(read(repo.path(), "outside.txt"), "outside\n");
}
