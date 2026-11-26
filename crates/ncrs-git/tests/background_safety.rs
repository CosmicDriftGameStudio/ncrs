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
use tempfile::TempDir;
#[cfg(unix)]
use {git_cli::Repo, std::path::PathBuf, std::sync::atomic::AtomicBool, std::sync::Arc};

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
    repo.sh(&[
        "config",
        "credential.https://a=b@x.test.helper",
        "!echo evil",
    ]);
    assert!(repo.git.repository_config_runs_commands());
}

#[test]
fn an_unattended_fetch_is_refused_when_the_repository_config_runs_commands() {
    let (_origin, repo) = with_origin();
    let unattended = Git::open_unattended(repo.path()).unwrap();
    unattended.fetch(Some("origin")).unwrap();

    repo.sh(&["config", "credential.helper", "!echo evil"]);
    unattended.fetch(Some("origin")).unwrap_err();
    unattended.fast_forward().unwrap_err();
    unattended.push().unwrap_err();
    unattended.switch_to_default().unwrap_err();
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

#[cfg(unix)]
#[test]
fn an_unattended_fast_forward_leaves_submodules_alone() {
    let (_origin, repo) = with_origin();
    let library = git_cli::repo();
    let marker = library.path().join("program-ran");
    repo.sh(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        library.path().to_str().unwrap(),
        "library",
    ]);
    repo.sh(&["commit", "-q", "-m", "library"]);
    repo.sh(&["push", "-q", "origin", "HEAD"]);
    repo.sh(&["config", "submodule.recurse", "true"]);
    repo.sh(&["config", "submodule.library.fetchRecurseSubmodules", "true"]);
    let command = format!("touch {}; git-upload-pack", marker.display());
    repo.sh(&[
        "-C",
        "library",
        "config",
        "remote.origin.uploadpack",
        &command,
    ]);

    let unattended = Git::open_unattended(repo.path()).unwrap();
    unattended.fetch(Some("origin")).unwrap();
    unattended.fetch(None).unwrap();
    unattended.fast_forward().unwrap();
    assert!(!marker.exists());

    let _attended = repo.git.pull(None);
    assert!(marker.exists(), "the program of the submodule would run");
}

/// A repository with a submodule whose own config names a filter that makes
/// the returned file, and with a changed file in the submodule.
#[cfg(unix)]
fn with_hostile_submodule() -> (Repo, TempDir, PathBuf) {
    let repo = repo();
    let library = git_cli::repo();
    let outside = TempDir::new().unwrap();
    let marker = outside.path().join("filter-ran");
    repo.sh(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        library.path().to_str().unwrap(),
        "library",
    ]);
    repo.sh(&["commit", "-q", "-m", "library"]);
    repo.sh(&["branch", "feature"]);
    let clean = format!("touch {}; cat", marker.display());
    repo.sh(&["-C", "library", "config", "filter.evil.clean", &clean]);
    write(
        repo.path(),
        ".git/modules/library/info/attributes",
        "* filter=evil\n",
    );
    write(repo.path(), "library/a.txt", "ONE\ntwo\nthree\n");
    (repo, outside, marker)
}

#[cfg(unix)]
#[test]
fn showing_a_repository_runs_no_filter_of_a_submodule() {
    let (repo, _outside, marker) = with_hostile_submodule();

    Git::open_reading(repo.path()).unwrap().changes().unwrap();
    ncrs_git::status(repo.path(), &Arc::new(AtomicBool::new(false))).unwrap();
    assert!(!marker.exists());

    repo.sh(&["status"]);
    assert!(marker.exists(), "git itself runs it");
}

#[cfg(unix)]
#[test]
fn an_unattended_switch_runs_no_filter_of_a_submodule() {
    let (repo, _outside, marker) = with_hostile_submodule();
    repo.sh(&["checkout", "-q", "feature"]);
    assert!(!marker.exists());

    let unattended = Git::open_unattended(repo.path()).unwrap();
    assert_eq!(unattended.switch_to_default().unwrap(), "main");
    assert!(!marker.exists());

    repo.git.checkout("feature").unwrap();
    assert!(marker.exists(), "git itself runs it");
}

#[test]
fn a_filter_that_only_one_branch_includes_is_seen_after_the_switch() {
    let (_origin, repo) = with_origin();
    repo.sh(&["checkout", "-q", "-b", "feature"]);
    write(
        repo.path(),
        ".git/evil.inc",
        "[filter \"evil\"]\n\tsmudge = evil\n",
    );
    repo.sh(&["config", "includeIf.onbranch:main.path", "evil.inc"]);

    let unattended = Git::open_unattended(repo.path()).unwrap();
    assert!(!unattended.has_content_filters().unwrap());
    unattended.switch_to_default().unwrap();
    unattended.fast_forward().unwrap_err();
    unattended.checkout("feature").unwrap_err();
}

#[test]
fn an_unattended_switch_never_writes_outside_the_repository() {
    let repo = repo();
    repo.sh(&["branch", "feature"]);
    let elsewhere = TempDir::new().unwrap();
    let folder = elsewhere.path().to_str().unwrap();
    repo.sh(&["config", "core.worktree", folder]);

    let unattended = Git::open_unattended(repo.path()).unwrap();
    unattended.checkout("feature").unwrap_err();
    assert!(!elsewhere.path().join("a.txt").exists());
}

#[test]
fn a_filter_name_that_cannot_be_switched_off_is_refused() {
    let repo = repo();
    repo.sh(&["config", "filter.a=b.clean", "evil"]);
    Git::open_reading(repo.path()).unwrap_err();
}

#[cfg(unix)]
#[test]
fn an_unattended_push_runs_no_hook_of_a_local_remote() {
    use std::os::unix::fs::PermissionsExt as _;

    let (origin, repo) = with_origin();
    repo.sh(&["push", "-q", "origin", "HEAD:refs/heads/topic"]);
    let marker = origin.path().join("hook-ran");
    let hook = origin.path().join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();

    let unattended = Git::open_unattended(repo.path()).unwrap();
    unattended
        .delete_remote_branch("origin", "topic", None)
        .unwrap_err();
    assert!(!marker.exists());

    repo.git
        .delete_remote_branch("origin", "topic", None)
        .unwrap();
    assert!(marker.exists(), "git itself runs it");
}

#[test]
fn a_fast_forward_is_refused_while_a_merge_has_left_its_stash() {
    let (_origin, repo) = with_origin();
    let unattended = Git::open_unattended(repo.path()).unwrap();
    unattended.fetch(None).unwrap();
    unattended.fast_forward().unwrap();

    let head = repo.sh(&["rev-parse", "HEAD"]);
    for left in [head.as_str(), "refs/stash", &head[..8], ""] {
        write(repo.path(), ".git/MERGE_AUTOSTASH", left);
        unattended.fast_forward().unwrap_err();
    }
}

#[test]
fn a_fast_forward_is_refused_when_the_repository_sets_merge_options() {
    let (_origin, repo) = with_origin();
    let unattended = Git::open_unattended(repo.path()).unwrap();
    for name in ["branch.main.mergeOptions", "pull.twohead"] {
        unattended.fast_forward().unwrap();
        repo.sh(&["config", name, "ours"]);
        unattended.fast_forward().unwrap_err();
        repo.sh(&["config", "--unset", name]);
    }
}

#[test]
fn an_unattended_git_never_pulls() {
    let (_origin, repo) = with_origin();
    let unattended = Git::open_unattended(repo.path()).unwrap();
    unattended.pull(None).unwrap_err();
}

#[cfg(unix)]
#[test]
fn a_filter_is_switched_off_however_its_section_is_spelled() {
    let repo = repo();
    let outside = TempDir::new().unwrap();
    let marker = outside.path().join("filter-ran");
    let config = std::fs::read_to_string(repo.path().join(".git/config")).unwrap();
    let filter = format!("[filter.Evil]\n\tclean = touch {}; cat\n", marker.display());
    write(repo.path(), ".git/config", &(config + &filter));
    write(repo.path(), ".git/info/attributes", "* filter=evil\n");
    write(repo.path(), "a.txt", "ONE\ntwo\nthree\n");

    Git::open_reading(repo.path()).unwrap().changes().unwrap();
    assert!(!marker.exists());

    repo.sh(&["status"]);
    assert!(marker.exists(), "git itself runs it");
}
