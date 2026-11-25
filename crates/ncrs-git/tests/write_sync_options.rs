//! Fast-forward-only pull, force push with lease, commit and push, and a hanging remote.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use std::net::TcpListener;
use std::time::{Duration, Instant};

use git_cli::{clone_of, read, with_origin};
use ncrs_git::{ActionError, CommitMessage, Git, IntegrateOutcome, PullStrategy};

fn message(title: &str) -> CommitMessage {
    CommitMessage {
        title: title.to_string(),
        body: String::new(),
    }
}

/// `repo` has a local commit, the origin has another one: the branches diverged.
fn diverged() -> (tempfile::TempDir, git_cli::Repo) {
    let (origin, repo) = with_origin();
    let other = clone_of(&origin);
    other.commit_file("theirs.txt", "t\n", "theirs");
    other.sh(&["push", "-q"]);
    repo.commit_file("mine.txt", "m\n", "mine");
    repo.git.fetch(None).unwrap();
    (origin, repo)
}

#[test]
fn fast_forward_only_moves_a_branch_that_is_only_behind() {
    let (origin, repo) = with_origin();
    let other = clone_of(&origin);
    other.commit_file("theirs.txt", "t\n", "theirs");
    other.sh(&["push", "-q"]);
    let outcome = repo.git.pull(Some(PullStrategy::FastForwardOnly)).unwrap();
    assert_eq!(outcome, IntegrateOutcome::Completed);
    assert_eq!(read(repo.path(), "theirs.txt"), "t\n");
}

#[test]
fn fast_forward_only_refuses_diverged_branches_and_changes_nothing() {
    let (_origin, repo) = diverged();
    let head = repo.sh(&["rev-parse", "HEAD"]);
    let result = repo.git.pull(Some(PullStrategy::FastForwardOnly));
    assert!(matches!(result, Err(ActionError::NotFastForward { .. })), "{result:?}");
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), head);
    assert_eq!(repo.git.integration_in_progress(), None);
}

#[test]
fn the_chosen_strategy_beats_pull_settings_in_the_config() {
    let (_origin, repo) = diverged();
    repo.sh(&["config", "pull.ff", "only"]);
    assert_eq!(
        repo.git.pull(Some(PullStrategy::Merge)).unwrap(),
        IntegrateOutcome::Completed
    );
    assert_eq!(repo.sh(&["rev-list", "--count", "--merges", "HEAD"]), "1");
}

#[test]
fn force_push_shows_what_it_replaces_and_pushes_with_a_lease() {
    let (origin, repo) = with_origin();
    let other = clone_of(&origin);
    other.commit_file("theirs.txt", "t\n", "theirs one");
    other.sh(&["push", "-q"]);
    repo.git.fetch(None).unwrap();
    repo.commit_file("mine.txt", "m\n", "mine");

    let plan = repo.git.prepare_force_push().unwrap();
    assert_eq!((plan.remote.as_str(), plan.target.as_str()), ("origin", "main"));
    assert_eq!(plan.lost, 1);
    assert_eq!(plan.lost_titles, vec!["theirs one".to_string()]);
    assert!(matches!(repo.git.push(), Err(ActionError::Rejected { .. })));

    repo.git.force_push_with_lease(&plan).unwrap();
    assert_eq!(
        origin_head(&origin),
        repo.sh(&["rev-parse", "HEAD"]),
        "the remote now has the local commit"
    );
}

fn origin_head(origin: &tempfile::TempDir) -> String {
    git_cli::sh(origin.path(), &["rev-parse", "main"])
}

#[test]
fn a_lease_is_lost_when_the_remote_moved_after_the_plan_even_after_a_fetch() {
    let (origin, repo) = with_origin();
    repo.commit_file("mine.txt", "m\n", "mine");
    let other = clone_of(&origin);
    other.commit_file("theirs.txt", "t\n", "theirs");
    other.sh(&["push", "-q"]);
    repo.git.fetch(None).unwrap();
    let plan = repo.git.prepare_force_push().unwrap();

    let third = clone_of(&origin);
    third.commit_file("late.txt", "l\n", "late");
    third.sh(&["push", "-q"]);
    // The background fetch moves the tracking ref; the lease must not follow it.
    repo.git.fetch(None).unwrap();

    assert_eq!(
        repo.git.force_push_with_lease(&plan),
        Err(ActionError::LeaseRejected)
    );
    assert_eq!(origin_head(&origin), third.sh(&["rev-parse", "HEAD"]));
}

#[test]
fn commit_and_push_pushes_what_it_committed() {
    let (origin, repo) = with_origin();
    git_cli::write(repo.path(), "n.txt", "n\n");
    repo.sh(&["add", "n.txt"]);
    let done = repo.git.commit_and_push(&message("add n"), false).unwrap();
    assert!(done.undo.is_some());
    assert_eq!(done.push, Ok(()));
    assert_eq!(origin_head(&origin), repo.sh(&["rev-parse", "HEAD"]));
}

#[test]
fn a_rejected_push_after_the_commit_keeps_the_commit() {
    let (origin, repo) = with_origin();
    let other = clone_of(&origin);
    other.commit_file("theirs.txt", "t\n", "theirs");
    other.sh(&["push", "-q"]);
    repo.git.fetch(None).unwrap();
    git_cli::write(repo.path(), "n.txt", "n\n");
    repo.sh(&["add", "n.txt"]);
    let done = repo.git.commit_and_push(&message("add n"), false).unwrap();
    assert!(matches!(done.push, Err(ActionError::Rejected { .. })));
    assert_eq!(repo.sh(&["log", "-1", "--format=%s"]), "add n");
    assert!(done.undo.is_some());
}

#[test]
fn a_hanging_remote_is_killed_at_the_timeout() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (accepted, _keep_open) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = accepted.send(stream);
        }
    });
    let (_origin, repo) = with_origin();
    repo.sh(&["remote", "set-url", "origin", &format!("git://127.0.0.1:{port}/repo")]);
    let git = Git::open_unattended(repo.path())
        .unwrap()
        .with_timeout(Duration::from_millis(600));
    let started = Instant::now();
    let result = git.fetch(None);
    assert!(
        matches!(result, Err(ActionError::NetworkFailed { .. })),
        "{result:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(20));
}
