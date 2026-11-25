//! The quick summary of a repository, the default branch and the last fetch.
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

use git_cli::{clone_of, repo, with_origin, write};
use ncrs_git::{last_fetch, refs, repo_summary};

fn go() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[test]
fn counts_distance_to_the_upstream_and_changed_files() {
    let (origin, repo) = with_origin();
    let other = clone_of(&origin);
    other.commit_file("theirs.txt", "t\n", "theirs");
    other.sh(&["push", "-q"]);
    repo.git.fetch(None).unwrap();
    repo.commit_file("m1.txt", "m\n", "mine 1");
    repo.commit_file("m2.txt", "m\n", "mine 2");
    write(repo.path(), "a.txt", "changed\n");
    write(repo.path(), "loose.txt", "x\n");
    write(repo.path(), ".gitignore", "ignored.txt\n");
    write(repo.path(), "ignored.txt", "x\n");

    let summary = repo_summary(repo.path(), &go()).unwrap();
    assert_eq!(summary.branch.as_deref(), Some("main"));
    assert_eq!((summary.ahead, summary.behind), (2, 1));
    assert_eq!(
        summary.changed, 3,
        "a.txt, loose.txt and .gitignore; the ignored file is not counted"
    );
    assert!(summary.fetched_at.is_some());
    assert_eq!(summary.main_worktree, None);
}

#[test]
fn the_default_branch_follows_origin_head_else_main_else_master() {
    let (origin, repo) = with_origin();
    assert_eq!(
        repo_summary(repo.path(), &go())
            .unwrap()
            .default_branch
            .as_deref(),
        Some("main"),
        "no origin/HEAD yet: main"
    );
    repo.sh(&["branch", "develop"]);
    repo.sh(&["push", "-q", "origin", "develop"]);
    repo.sh(&["remote", "set-head", "origin", "develop"]);
    assert_eq!(
        repo_summary(repo.path(), &go())
            .unwrap()
            .default_branch
            .as_deref(),
        Some("develop")
    );
    assert_eq!(
        refs(repo.path(), &go()).unwrap().main_branch.as_deref(),
        Some("develop")
    );

    git_cli::sh(
        origin.path(),
        &["symbolic-ref", "HEAD", "refs/heads/develop"],
    );
    let clone = clone_of(&origin);
    assert_eq!(
        repo_summary(clone.path(), &go())
            .unwrap()
            .default_branch
            .as_deref(),
        Some("develop"),
        "a clone learns it from the remote"
    );

    let plain = repo_with_master();
    assert_eq!(
        repo_summary(plain.path(), &go())
            .unwrap()
            .default_branch
            .as_deref(),
        Some("master")
    );
}

fn repo_with_master() -> git_cli::Repo {
    let repo = repo();
    repo.sh(&["branch", "-m", "main", "master"]);
    repo
}

#[test]
fn a_linked_worktree_belongs_to_its_repository() {
    let repo = repo();
    let linked = tempfile::TempDir::new().unwrap();
    let folder = linked.path().join("wt");
    repo.sh(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feature",
        folder.to_str().unwrap(),
    ]);

    let main = repo_summary(repo.path(), &go()).unwrap();
    assert_eq!(main.worktrees.len(), 1);
    assert_eq!(main.worktrees[0].branch.as_deref(), Some("feature"));

    let from_linked = repo_summary(&folder, &go()).unwrap();
    assert_eq!(from_linked.branch.as_deref(), Some("feature"));
    let owner = from_linked.main_worktree.unwrap();
    assert_eq!(
        std::fs::canonicalize(owner).unwrap(),
        std::fs::canonicalize(repo.path()).unwrap()
    );
}

#[test]
fn a_repository_that_never_fetched_has_no_fetch_time() {
    let repo = repo();
    assert_eq!(last_fetch(repo.path()), None);
    assert_eq!(repo_summary(repo.path(), &go()).unwrap().fetched_at, None);
}

#[test]
fn a_stopped_summary_says_so() {
    let repo = repo();
    let stop = Arc::new(AtomicBool::new(true));
    assert_eq!(
        repo_summary(repo.path(), &stop),
        Err(ncrs_git::GitError::Interrupted)
    );
}
