//! Branches, fetch, pull, push against a local bare "origin".
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use git_cli::{clone_of, read, repo, with_origin};
use ncrs_git::{ActionError, ConflictChoice, IntegrateOutcome};

#[test]
fn create_checkout_rename_and_delete_branches() {
    let repo = repo();
    repo.git.create_branch("side", "main", false).unwrap();
    assert_eq!(repo.git.current_branch().as_deref(), Some("main"));
    repo.git.create_branch("work", "main", true).unwrap();
    assert_eq!(repo.git.current_branch().as_deref(), Some("work"));
    repo.git.checkout("side").unwrap();
    assert_eq!(repo.git.current_branch().as_deref(), Some("side"));
    repo.git.rename_branch("work", "job").unwrap();
    assert_eq!(
        repo.sh(&["branch", "--format=%(refname:short)"]),
        "job\nmain\nside"
    );
    repo.git.checkout("main").unwrap();
    repo.git.delete_branch("job", false).unwrap();
    assert_eq!(
        repo.sh(&["branch", "--format=%(refname:short)"]),
        "main\nside"
    );
}

#[test]
fn deleting_an_unmerged_branch_is_typed_then_forced_and_undoable() {
    let repo = repo();
    repo.git.create_branch("topic", "main", true).unwrap();
    repo.commit_file("t.txt", "t\n", "topic work");
    let tip = repo.sh(&["rev-parse", "topic"]);
    repo.git.checkout("main").unwrap();

    assert_eq!(
        repo.git.delete_branch("topic", false),
        Err(ActionError::BranchNotMerged {
            branch: "topic".to_string()
        })
    );
    let undo = repo.git.delete_branch("topic", true).unwrap();
    assert_eq!(repo.sh(&["branch", "--format=%(refname:short)"]), "main");

    repo.git.undo(&undo).unwrap();
    assert_eq!(repo.sh(&["rev-parse", "topic"]), tip);
}

#[test]
fn remote_branch_becomes_a_tracking_local_branch() {
    let (origin, repo) = with_origin();
    let other = clone_of(&origin);
    other.sh(&["checkout", "-q", "-b", "feature"]);
    other.commit_file("f.txt", "f\n", "feature");
    other.sh(&["push", "-q", "origin", "feature"]);

    repo.git.fetch(Some("origin")).unwrap();
    repo.git.checkout_remote_branch("origin/feature").unwrap();
    assert_eq!(repo.git.current_branch().as_deref(), Some("feature"));
    assert_eq!(
        repo.sh(&["rev-parse", "--abbrev-ref", "@{u}"]),
        "origin/feature"
    );
    assert_eq!(read(repo.path(), "f.txt"), "f\n");
}

#[test]
fn push_branch_sets_upstream_when_missing_and_works_off_checkout() {
    let (origin, repo) = with_origin();
    repo.git.create_branch("side", "main", false).unwrap();
    repo.git.push_branch("side", None).unwrap();
    assert_eq!(repo.sh(&["config", "branch.side.remote"]), "origin");
    assert_eq!(origin_branches(&origin), "main\nside");
    assert_eq!(repo.git.current_branch().as_deref(), Some("main"));
}

fn origin_branches(origin: &tempfile::TempDir) -> String {
    git_cli::sh(origin.path(), &["branch", "--format=%(refname:short)"])
}

#[test]
fn delete_a_branch_on_the_remote() {
    let (origin, repo) = with_origin();
    repo.git.create_branch("gone", "main", false).unwrap();
    repo.git.push_branch("gone", None).unwrap();
    repo.git.delete_remote_branch("origin", "gone").unwrap();
    assert_eq!(origin_branches(&origin), "main");
}

#[test]
fn push_without_any_remote_says_so() {
    let repo = repo();
    assert!(matches!(
        repo.git.push(),
        Err(ActionError::Other { git_output }) if git_output.contains("no remote")
    ));
}

#[test]
fn push_of_a_stale_branch_is_rejected() {
    let (origin, repo) = with_origin();
    let other = clone_of(&origin);
    other.commit_file("o.txt", "o\n", "from the other clone");
    other.sh(&["push", "-q", "origin", "main"]);

    repo.commit_file("m.txt", "m\n", "local");
    assert_eq!(repo.git.push(), Err(ActionError::Rejected { behind: 0 }));

    repo.git.fetch(None).unwrap();
    assert_eq!(repo.git.push(), Err(ActionError::Rejected { behind: 1 }));
}

#[test]
fn pull_fast_forwards_and_fetch_updates_the_tracking_ref() {
    let (origin, repo) = with_origin();
    let other = clone_of(&origin);
    other.commit_file("o.txt", "o\n", "from the other clone");
    other.sh(&["push", "-q", "origin", "main"]);

    repo.git.fetch(None).unwrap();
    assert_eq!(repo.sh(&["rev-list", "--count", "main..origin/main"]), "1");
    assert_eq!(repo.git.pull(None).unwrap(), IntegrateOutcome::Completed);
    assert_eq!(read(repo.path(), "o.txt"), "o\n");
    repo.git.push().unwrap();
}

#[test]
fn pull_that_conflicts_comes_back_as_conflicts() {
    let (origin, repo) = with_origin();
    let other = clone_of(&origin);
    other.commit_file("a.txt", "one\ntheirs\nthree\n", "theirs");
    other.sh(&["push", "-q", "origin", "main"]);
    repo.commit_file("a.txt", "one\nmine\nthree\n", "mine");

    let IntegrateOutcome::Conflicts(files) = repo.git.pull(None).unwrap() else {
        panic!("expected conflicts");
    };
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "a.txt");
    repo.git
        .resolve_file("a.txt", ConflictChoice::Both)
        .unwrap();
    repo.git.mark_resolved("a.txt").unwrap();
    assert_eq!(
        repo.git.continue_integration().unwrap(),
        IntegrateOutcome::Completed
    );
    assert_eq!(read(repo.path(), "a.txt"), "one\nmine\ntheirs\nthree\n");
}

#[test]
fn fetch_from_a_missing_remote_is_a_typed_failure() {
    let (_origin, repo) = with_origin();
    repo.sh(&["remote", "set-url", "origin", "/definitely/not/here.git"]);
    let error = repo.git.fetch(Some("origin")).unwrap_err();
    assert!(matches!(error, ActionError::Other { .. }), "{error:?}");
}

#[test]
fn switching_to_the_default_branch_uses_origin_head_and_makes_a_missing_local_branch() {
    let (origin, repo) = with_origin();
    repo.git.create_branch("feat", "main", true).unwrap();
    repo.sh(&["push", "-q", "origin", "feat"]);
    git_cli::sh(origin.path(), &["symbolic-ref", "HEAD", "refs/heads/feat"]);

    assert_eq!(
        repo.git.switch_to_default().unwrap(),
        "main",
        "no origin/HEAD here yet: main"
    );
    let clone = clone_of(&origin);
    clone.git.checkout("main").unwrap();
    assert_eq!(clone.git.switch_to_default().unwrap(), "feat");
    assert_eq!(clone.git.current_branch().as_deref(), Some("feat"));

    let fresh = git_cli::TempDir::new().unwrap();
    git_cli::sh(
        fresh.path(),
        &["clone", "-q", origin.path().to_str().unwrap(), "."],
    );
    git_cli::sh(fresh.path(), &["checkout", "-q", "main"]);
    git_cli::sh(fresh.path(), &["branch", "-D", "feat"]);
    let git = ncrs_git::Git::open(fresh.path()).unwrap();
    assert_eq!(git.switch_to_default().unwrap(), "feat");
    assert_eq!(git.current_branch().as_deref(), Some("feat"));
}
