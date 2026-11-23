//! Names and paths that start with "-" never become options, and branch names are checked.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use git_cli::{repo, with_origin, write};
use ncrs_git::{ActionError, IntegrateOutcome, PullStrategy};

fn invalid(name: &str) -> Result<(), ActionError> {
    Err(ActionError::InvalidName {
        name: name.to_string(),
    })
}

#[test]
fn a_branch_name_that_git_would_refuse_is_refused_before_git_runs() {
    let repo = repo();
    for name in [
        "", "-d", "--detach", "a b", "x..y", "x~1", "@{-1}", "end.lock", "/x",
    ] {
        assert_eq!(repo.git.check_branch_name(name), invalid(name), "{name:?}");
        assert_eq!(
            repo.git.create_branch(name, "main", true),
            invalid(name),
            "{name:?}"
        );
    }
    assert_eq!(repo.git.check_branch_name("feature/ok-1"), Ok(()));
    assert_eq!(repo.sh(&["branch", "--format=%(refname:short)"]), "main");
    assert_eq!(repo.git.current_branch().as_deref(), Some("main"));
}

#[test]
fn refs_that_read_as_options_do_nothing() {
    let repo = repo();
    repo.git.create_branch("side", "main", false).unwrap();
    for option in ["--detach", "-D", "--orphan"] {
        assert_eq!(repo.git.checkout(option), invalid(option));
        assert_eq!(repo.git.create_branch("x", option, true), invalid(option));
        assert_eq!(repo.git.rename_branch(option, "y"), invalid(option));
        assert_eq!(repo.git.rename_branch("side", option), invalid(option));
        assert!(matches!(
            repo.git.delete_branch(option, true),
            Err(ActionError::InvalidName { .. })
        ));
        assert_eq!(
            repo.git.merge(option),
            Err(ActionError::InvalidName {
                name: option.to_string()
            })
        );
        assert_eq!(
            repo.git.rebase(option),
            Err(ActionError::InvalidName {
                name: option.to_string()
            })
        );
        assert_eq!(
            repo.git.fetch(Some(option)),
            Err(ActionError::InvalidName {
                name: option.to_string()
            })
        );
        assert_eq!(
            repo.git.delete_remote_branch(option, "x"),
            Err(ActionError::InvalidName {
                name: option.to_string()
            })
        );
    }
    assert_eq!(repo.git.current_branch().as_deref(), Some("main"));
    assert_eq!(
        repo.sh(&["branch", "--format=%(refname:short)"]),
        "main\nside"
    );
}

#[test]
fn a_file_named_like_an_option_is_staged_unstaged_and_discarded_as_a_file() {
    let repo = repo();
    write(repo.path(), "-n", "dash\n");
    write(repo.path(), "--all", "more\n");
    repo.git.stage(&["-n"]).unwrap();
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "-n");
    repo.git.unstage(&["-n"]).unwrap();
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "");
    repo.git.discard_file("--all").unwrap();
    assert!(!repo.path().join("--all").exists());
    assert!(repo.path().join("-n").exists());
}

#[test]
fn a_pull_strategy_is_given_for_one_call_and_not_written_to_the_config() {
    let (origin, repo) = with_origin();
    let other = git_cli::clone_of(&origin);
    other.commit_file("o.txt", "o\n", "other");
    other.sh(&["push", "-q"]);
    repo.commit_file("m.txt", "m\n", "mine");
    repo.sh(&["config", "--unset", "pull.rebase"]);
    assert!(matches!(
        repo.git.pull(None),
        Err(ActionError::NeedsPullStrategy { .. })
    ));
    assert_eq!(
        repo.git.pull(Some(PullStrategy::Rebase)).unwrap(),
        IntegrateOutcome::Completed
    );
    assert_eq!(repo.sh(&["log", "--format=%s", "-3"]), "mine\nother\nfirst");
    assert!(
        !repo.sh(&["config", "--list"]).contains("pull.rebase"),
        "the choice is not kept"
    );
}

#[test]
fn the_lists_of_changes_split_staged_from_unstaged() {
    let repo = repo();
    write(repo.path(), "a.txt", "one\ntwo\nthree\nfour\n");
    write(repo.path(), "new.txt", "n1\nn2\n");
    repo.git.stage(&["a.txt"]).unwrap();
    write(repo.path(), "a.txt", "ONE\ntwo\nthree\nfour\n");
    let lists = repo.git.changes().unwrap();
    let staged: Vec<_> = lists
        .staged
        .iter()
        .map(|file| (file.path.as_str(), file.added, file.removed))
        .collect();
    let unstaged: Vec<_> = lists
        .unstaged
        .iter()
        .map(|file| (file.path.as_str(), file.added, file.removed))
        .collect();
    assert_eq!(staged, [("a.txt", 1, 0)]);
    assert_eq!(unstaged, [("a.txt", 1, 1), ("new.txt", 2, 0)]);
}
