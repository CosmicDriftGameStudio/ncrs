//! `repository_config_runs_commands` on real repositories, includes and all.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use git_cli::{repo, with_origin, write};
use ncrs_git::Git;

#[test]
fn a_fresh_repository_runs_nothing() {
    let repo = repo();
    assert!(!repo.git.repository_config_runs_commands());
}

#[test]
fn a_helper_in_the_repository_config_is_found() {
    let repo = repo();
    repo.sh(&["config", "credential.helper", "!echo evil"]);
    assert!(repo.git.repository_config_runs_commands());
}

#[test]
fn a_helper_in_an_included_file_is_found() {
    let repo = repo();
    write(repo.path(), "extra.cfg", "[core]\n\tsshCommand = evil\n");
    let include = repo.path().join("extra.cfg");
    repo.sh(&["config", "include.path", include.to_str().unwrap()]);
    assert!(repo.git.repository_config_runs_commands());
}

#[test]
fn a_command_transport_remote_is_found() {
    let repo = repo();
    repo.sh(&["remote", "add", "evil", "ext::sh -c touch% x"]);
    assert!(repo.git.repository_config_runs_commands());
}

#[test]
fn an_equals_sign_in_the_name_hides_nothing() {
    let repo = repo();
    repo.sh(&["config", "credential.https://a=b@x.test.helper", "!echo evil"]);
    assert!(repo.git.repository_config_runs_commands());
}

#[test]
fn an_unattended_fetch_is_refused_when_the_repository_config_runs_commands() {
    let (_origin, repo) = with_origin();
    let unattended = Git::open_unattended(repo.path()).unwrap();
    unattended.fetch(Some("origin")).unwrap();

    repo.sh(&["config", "credential.helper", "!echo evil"]);
    unattended.fetch(Some("origin")).unwrap_err();
    unattended.pull(None).unwrap_err();
    unattended.push().unwrap_err();
    repo.git.fetch(Some("origin")).unwrap();
}

#[test]
fn content_filters_of_the_repository_are_found_and_the_users_own_are_not() {
    let repo = repo();
    assert!(!repo.git.has_content_filters().unwrap());
    repo.sh(&["config", "filter.lfs.required", "true"]);
    assert!(!repo.git.has_content_filters().unwrap());
    repo.sh(&["config", "filter.git-crypt.smudge", "git-crypt smudge"]);
    assert!(repo.git.has_content_filters().unwrap());
    let unattended = Git::open_unattended(repo.path()).unwrap();
    assert!(unattended.has_content_filters().unwrap());
}
