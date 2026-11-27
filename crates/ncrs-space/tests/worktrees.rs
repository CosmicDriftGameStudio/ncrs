//! Merged worktrees: which are listed, and that removing one goes through git.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use common::Fake;
use std::collections::HashSet;

use ncrs_space::{
    merged_worktrees, remove_worktrees, scan_worktrees, worktree_roots, KeptWhy, Skip, UnknownUse,
};
use tempfile::TempDir;

fn git(folder: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(folder)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// A repository on `main` with a linked worktree on `feat` whose commit is merged.
fn merged() -> (TempDir, PathBuf, PathBuf) {
    let dir = TempDir::new().unwrap();
    let repo = std::fs::canonicalize(dir.path()).unwrap().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let linked = repo.parent().unwrap().join("linked");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat",
            linked.to_str().unwrap(),
        ],
    );
    std::fs::write(linked.join("b.txt"), "b\n").unwrap();
    git(&linked, &["add", "."]);
    git(&linked, &["commit", "-q", "-m", "work"]);
    git(&repo, &["merge", "-q", "--no-ff", "-m", "merge", "feat"]);
    (dir, repo, linked)
}

fn stop() -> AtomicBool {
    AtomicBool::new(false)
}

#[test]
fn a_clean_merged_linked_worktree_is_listed_once_with_its_size() {
    let (_dir, repo, linked) = merged();

    let found = merged_worktrees(&[repo.clone(), linked.clone()], &stop());

    assert_eq!(
        found.len(),
        1,
        "the repo and its worktree listed twice count once"
    );
    assert_eq!(found[0].branch, "feat");
    assert_eq!(found[0].repo, repo);
    assert_eq!(
        std::fs::canonicalize(&found[0].path).unwrap(),
        std::fs::canonicalize(&linked).unwrap()
    );
    assert!(found[0].size.bytes > 0);
}

#[test]
fn a_merged_worktree_with_a_dirty_file_is_not_listed() {
    let (_dir, repo, linked) = merged();
    std::fs::write(linked.join("scratch.txt"), "x\n").unwrap();
    assert!(merged_worktrees(&[repo], &stop()).is_empty());
}

#[test]
fn an_unmerged_worktree_is_not_listed() {
    let (_dir, repo, linked) = merged();
    std::fs::write(linked.join("c.txt"), "c\n").unwrap();
    git(&linked, &["add", "."]);
    git(&linked, &["commit", "-q", "-m", "more"]);
    assert!(merged_worktrees(&[repo], &stop()).is_empty());
}

#[test]
fn removing_goes_through_git_and_takes_the_branch_with_it() {
    let (_dir, repo, linked) = merged();
    let found = merged_worktrees(&[repo.clone()], &stop());

    let log = remove_worktrees(&found, &Fake::quiet(), UnknownUse::Skip);

    assert_eq!(log.rows[0].outcome, Ok(found[0].size.bytes));
    assert!(!linked.exists());
    let branches = Command::new("git")
        .args(["branch"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&branches.stdout).contains("feat"));
}

#[test]
fn a_worktree_that_gained_a_file_after_the_scan_stays() {
    let (_dir, repo, linked) = merged();
    let found = merged_worktrees(&[repo], &stop());
    std::fs::write(linked.join("late.txt"), "late\n").unwrap();

    let log = remove_worktrees(&found, &Fake::quiet(), UnknownUse::Skip);

    assert_eq!(log.rows[0].outcome, Err(Skip::Changed));
    assert!(linked.join("late.txt").exists());
}

#[test]
fn a_worktree_in_use_stays() {
    let (_dir, repo, linked) = merged();
    let found = merged_worktrees(&[repo], &stop());
    let probes = Fake {
        folder: ncrs_space::FolderUse::Held(vec![ncrs_space::Holder {
            pid: 1,
            name: "editor".to_owned(),
        }]),
        ..Fake::quiet()
    };

    let log = remove_worktrees(&found, &probes, UnknownUse::Skip);

    assert!(matches!(log.rows[0].outcome, Err(Skip::FolderOpen(_))));
    assert!(linked.exists());
}

#[test]
fn an_unmerged_worktree_is_counted_as_open_and_a_dirty_one_as_changes() {
    let (_dir, repo, linked) = merged();
    std::fs::write(linked.join("c.txt"), "c\n").unwrap();
    git(&linked, &["add", "."]);
    git(&linked, &["commit", "-q", "-m", "more"]);

    let found = scan_worktrees(&[repo.clone()], &stop());

    assert!(found.merged.is_empty());
    assert_eq!(found.kept.len(), 1);
    assert_eq!(found.kept[0].why, KeptWhy::Open);
    assert_eq!(found.kept[0].branch.as_deref(), Some("feat"));
    assert!(found.kept[0].size.bytes > 0);

    let (_dir2, repo2, linked2) = merged();
    std::fs::write(linked2.join("scratch.txt"), "x\n").unwrap();
    let dirty = scan_worktrees(&[repo2], &stop());
    assert_eq!(dirty.kept.len(), 1);
    assert_eq!(dirty.kept[0].why, KeptWhy::Changes);
}

#[test]
fn the_roots_count_merged_kept_and_in_use_per_folder() {
    let (_dir, repo, linked) = merged();
    let found = scan_worktrees(&[repo], &stop());
    assert_eq!(found.merged.len(), 1);

    let free = worktree_roots(&found.merged, &HashSet::new(), &found.kept);
    assert_eq!(free.len(), 1);
    assert_eq!(free[0].folder, linked.parent().unwrap());
    assert_eq!(free[0].total.count, 1);
    assert_eq!(free[0].merged.count, 1);
    assert_eq!(free[0].kept.count, 0);

    let held: HashSet<PathBuf> = [found.merged[0].path.clone()].into();
    let busy = worktree_roots(&found.merged, &held, &found.kept);
    assert_eq!(busy[0].merged.count, 0);
    assert_eq!(busy[0].kept.count, 1);
    assert_eq!(busy[0].kept_because(KeptWhy::InUse).count, 1);
    assert_eq!(busy[0].kept_because(KeptWhy::Open).count, 0);
    assert_eq!(busy[0].total.bytes, found.merged[0].size.bytes);
}

#[test]
fn a_removed_worktree_log_row_says_the_check_answered() {
    let (_dir, repo, _linked) = merged();
    let found = merged_worktrees(&[repo], &stop());

    let log = remove_worktrees(&found, &Fake::quiet(), UnknownUse::Skip);

    assert!(log.rows[0].use_checked);
}
