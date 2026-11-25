//! `repository_config_runs_commands` on real repositories, includes and all.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use git_cli::{repo, write};

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
