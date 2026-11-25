//! What the clean up offers, keeps and deletes, against real repositories.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use git_cli::{clone_of, repo, sh, with_origin, write, Repo};
use ncrs_git::{
    cleanup_list, refs, remote_state, remote_tips, ActionError, CleanupList, CleanupPlan, CleanupReason, CleanupTarget,
    KeepReason, RemoteState,
};
use tempfile::TempDir;

fn go() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn scan(repo: &Repo, tips: &BTreeMap<String, String>) -> CleanupList {
    let state = remote_state(repo.path(), true, &AtomicBool::new(false)).unwrap();
    cleanup_list(&refs(repo.path(), &go()).unwrap(), tips, &state)
}

fn tips_of(repo: &Repo) -> BTreeMap<String, String> {
    remote_tips(repo.path(), &AtomicBool::new(false)).unwrap()
}

fn offered(list: &CleanupList) -> Vec<(&str, CleanupReason)> {
    list.branches
        .iter()
        .map(|branch| (branch.name.as_str(), branch.reason))
        .collect()
}

fn kept<'a>(list: &'a CleanupList, name: &str) -> Option<&'a KeepReason> {
    list.kept
        .iter()
        .find(|row| row.name == name)
        .map(|row| &row.why)
}

fn kept_worktree(list: &CleanupList) -> Option<&KeepReason> {
    list.kept
        .iter()
        .find(|row| row.is_worktree)
        .map(|row| &row.why)
}

/// A pushed branch `feat` with one commit.
fn with_pushed_feature() -> (TempDir, Repo) {
    let (origin, repo) = with_origin();
    repo.git.create_branch("feat", "main", true).unwrap();
    repo.commit_file("f.txt", "f\n", "feature work");
    repo.sh(&["push", "-q", "-u", "origin", "feat"]);
    repo.git.checkout("main").unwrap();
    (origin, repo)
}

#[test]
fn a_branch_merged_on_the_remote_counts_although_local_main_was_not_pulled() {
    let (origin, repo) = with_pushed_feature();
    let other = clone_of(&origin);
    other.sh(&["merge", "-q", "--no-ff", "-m", "merge feat", "origin/feat"]);
    other.sh(&["push", "-q"]);
    repo.git.fetch(None).unwrap();

    let list = scan(&repo, &tips_of(&repo));
    assert_eq!(offered(&list), vec![("feat", CleanupReason::InDefault)]);
    assert_eq!(kept(&list, "main"), Some(&KeepReason::DefaultBranch));
}

#[test]
fn a_squash_merged_branch_with_its_remote_deleted_is_offered_as_gone() {
    let (origin, repo) = with_pushed_feature();
    let other = clone_of(&origin);
    other.sh(&["merge", "-q", "--squash", "origin/feat"]);
    other.sh(&["commit", "-q", "-m", "squash feat"]);
    other.sh(&["push", "-q"]);
    other.sh(&["push", "-q", "origin", "--delete", "feat"]);
    let before = tips_of(&repo);
    repo.git.fetch(None).unwrap();

    let list = scan(&repo, &before);
    assert_eq!(offered(&list), vec![("feat", CleanupReason::UpstreamGone)]);
}

#[test]
fn a_gone_branch_with_a_commit_after_the_last_push_stays() {
    let (origin, repo) = with_pushed_feature();
    repo.git.checkout("feat").unwrap();
    repo.commit_file("later.txt", "l\n", "after the push");
    repo.git.checkout("main").unwrap();
    let other = clone_of(&origin);
    other.sh(&["push", "-q", "origin", "--delete", "feat"]);
    let before = tips_of(&repo);
    repo.git.fetch(None).unwrap();

    let list = scan(&repo, &before);
    assert!(list.branches.is_empty());
    assert_eq!(kept(&list, "feat"), Some(&KeepReason::AfterLastPush));
    assert_eq!(
        kept(&scan(&repo, &BTreeMap::new()), "feat"),
        Some(&KeepReason::AfterLastPush),
        "without the tips from before the fetch nothing is assumed"
    );
}

#[test]
fn unmerged_work_stays_with_the_number_of_commits_at_risk() {
    let repo = repo();
    repo.git.create_branch("wip", "main", true).unwrap();
    repo.commit_file("w1.txt", "1\n", "one");
    repo.commit_file("w2.txt", "2\n", "two");
    repo.git.checkout("main").unwrap();
    let list = scan(&repo, &BTreeMap::new());
    assert_eq!(
        kept(&list, "wip"),
        Some(&KeepReason::Unpushed { commits: 2 })
    );
}

#[test]
fn a_pushed_but_unmerged_branch_stays() {
    let (_origin, repo) = with_pushed_feature();
    let list = scan(&repo, &tips_of(&repo));
    assert_eq!(kept(&list, "feat"), Some(&KeepReason::NotMerged));
}

#[test]
fn a_fresh_empty_branch_stays_but_a_merged_one_goes() {
    let repo = repo();
    repo.git.create_branch("done", "main", true).unwrap();
    repo.commit_file("d.txt", "d\n", "done work");
    repo.git.checkout("main").unwrap();
    repo.sh(&["merge", "-q", "done"]);
    repo.sh(&["branch", "fresh"]);
    repo.sh(&["branch", "-m", "done", "done-old"]);
    repo.sh(&["branch", "-m", "done-old", "done"]);
    let list = scan(&repo, &BTreeMap::new());
    assert_eq!(kept(&list, "fresh"), Some(&KeepReason::Empty));
    assert_eq!(
        kept(&list, "done"),
        Some(&KeepReason::Empty),
        "same tip as main, no upstream"
    );
    repo.commit_file("next.txt", "n\n", "main moves on");
    let later = scan(&repo, &BTreeMap::new());
    assert_eq!(
        offered(&later),
        vec![
            ("done", CleanupReason::InDefault),
            ("fresh", CleanupReason::InDefault)
        ],
        "once main moved on they are plain merged branches"
    );
}

#[test]
fn the_current_and_the_default_branch_stay_and_master_works_as_default() {
    let repo = repo();
    repo.sh(&["branch", "-m", "main", "master"]);
    repo.git.create_branch("topic", "master", true).unwrap();
    let list = scan(&repo, &BTreeMap::new());
    assert_eq!(kept(&list, "master"), Some(&KeepReason::DefaultBranch));
    assert_eq!(kept(&list, "topic"), Some(&KeepReason::CurrentBranch));
}

fn merged_worktree(repo: &Repo) -> std::path::PathBuf {
    let folder = repo.path().with_extension("wt");
    repo.sh(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "wt-branch",
        folder.to_str().unwrap(),
    ]);
    write(&folder, "wt.txt", "w\n");
    sh(&folder, &["add", "wt.txt"]);
    sh(&folder, &["commit", "-q", "-m", "worktree work"]);
    repo.sh(&["merge", "-q", "--no-ff", "-m", "merge wt", "wt-branch"]);
    folder
}

#[test]
fn a_clean_merged_worktree_goes_with_its_folder_and_its_branch() {
    let repo = repo();
    let folder = merged_worktree(&repo);
    let list = scan(&repo, &BTreeMap::new());
    assert!(
        list.branches.is_empty(),
        "its branch is part of the worktree row"
    );
    assert_eq!(list.worktrees.len(), 1);

    let plan = CleanupPlan {
        worktrees: list.worktrees.clone(),
        ..CleanupPlan::default()
    };
    let report = repo.git.clean_up(&plan).unwrap();
    assert!(report.all_done(), "{report:?}");
    assert!(!folder.exists());
    assert!(!repo.sh(&["branch"]).contains("wt-branch"));
}

#[test]
fn a_dirty_or_locked_worktree_stays_with_its_branch() {
    let repo = repo();
    let folder = merged_worktree(&repo);
    write(&folder, "scratch.txt", "x\n");
    let list = scan(&repo, &BTreeMap::new());
    assert!(list.worktrees.is_empty());
    assert!(matches!(
        kept_worktree(&list),
        Some(KeepReason::WorktreeChanges { count: 1 })
    ));
    assert_eq!(kept(&list, "wt-branch"), Some(&KeepReason::InKeptWorktree));

    std::fs::remove_file(folder.join("scratch.txt")).unwrap();
    repo.sh(&["worktree", "lock", folder.to_str().unwrap()]);
    let locked = scan(&repo, &BTreeMap::new());
    assert_eq!(kept_worktree(&locked), Some(&KeepReason::Locked));
}

#[test]
fn a_branch_that_moved_after_the_scan_is_left_alone() {
    let repo = repo();
    repo.git.create_branch("done", "main", false).unwrap();
    repo.git.checkout("done").unwrap();
    repo.commit_file("d.txt", "d\n", "done");
    repo.git.checkout("main").unwrap();
    repo.sh(&["merge", "-q", "--no-ff", "-m", "merge done", "done"]);
    let list = scan(&repo, &BTreeMap::new());
    assert_eq!(list.branches.len(), 1);

    repo.git.checkout("done").unwrap();
    repo.commit_file("more.txt", "m\n", "late work");
    repo.git.checkout("main").unwrap();
    let report = repo
        .git
        .clean_up(&CleanupPlan {
            branches: list.branches,
            ..CleanupPlan::default()
        })
        .unwrap();
    assert!(!report.all_done());
    assert!(repo.sh(&["branch"]).contains("done"));
    assert!(report.undo.is_none());
}

#[test]
fn the_remote_branch_goes_only_when_asked_and_undo_brings_back_the_local_ones() {
    let (origin, repo) = with_pushed_feature();
    repo.sh(&["merge", "-q", "feat"]);
    repo.sh(&["push", "-q"]);
    let feat_tip = repo.sh(&["rev-parse", "feat"]);
    let list = scan(&repo, &tips_of(&repo));
    assert_eq!(offered(&list), vec![("feat", CleanupReason::InDefault)]);

    let keep_remote = repo
        .git
        .clean_up(&CleanupPlan {
            branches: list.branches.clone(),
            delete_remote: false,
            ..CleanupPlan::default()
        })
        .unwrap();
    assert!(keep_remote.all_done());
    assert!(sh(origin.path(), &["branch"]).contains("feat"));

    repo.git.undo(&keep_remote.undo.unwrap()).unwrap();
    assert_eq!(repo.sh(&["rev-parse", "feat"]), feat_tip);
    assert_eq!(
        repo.sh(&["rev-parse", "--abbrev-ref", "feat@{upstream}"]),
        "origin/feat"
    );

    let with_remote = repo
        .git
        .clean_up(&CleanupPlan {
            branches: list.branches,
            delete_remote: true,
            ..CleanupPlan::default()
        })
        .unwrap();
    assert!(with_remote.all_done(), "{with_remote:?}");
    assert!(with_remote
        .rows
        .iter()
        .any(|row| row.target == CleanupTarget::RemoteBranch("origin/feat".to_string())));
    assert!(!sh(origin.path(), &["branch"]).contains("feat"));
}

#[test]
fn a_shallow_clone_does_not_turn_unmerged_branches_into_merged_ones() {
    let (origin, repo) = with_origin();
    repo.commit_file("m2.txt", "2\n", "main two");
    repo.git.create_branch("feat", "main", true).unwrap();
    repo.commit_file("f.txt", "f\n", "feature");
    repo.sh(&["push", "-q", "origin", "main", "feat"]);

    let shallow = TempDir::new().unwrap();
    let url = origin.path().to_str().unwrap().to_string();
    sh(
        shallow.path(),
        &[
            "clone",
            "-q",
            "--depth",
            "1",
            "--no-single-branch",
            &url,
            ".",
        ],
    );
    sh(
        shallow.path(),
        &["checkout", "-q", "-b", "feat", "origin/feat"],
    );
    sh(shallow.path(), &["checkout", "-q", "main"]);
    let overview = refs(shallow.path(), &go()).unwrap();
    let feat = overview
        .local
        .iter()
        .find(|branch| branch.name == "feat")
        .unwrap();
    assert!(!feat.merged);
    assert!(cleanup_list(&overview, &BTreeMap::new(), &RemoteState::default())
        .branches
        .is_empty());
}

/// `feat` merged into main and pushed, so it is offered as `InDefault`.
fn merged_and_pushed() -> (TempDir, Repo) {
    let (origin, repo) = with_pushed_feature();
    repo.sh(&["merge", "-q", "feat"]);
    repo.sh(&["push", "-q"]);
    (origin, repo)
}

fn clean_with_remote(repo: &Repo, list: CleanupList) -> ncrs_git::CleanupReport {
    repo.git
        .clean_up(&CleanupPlan {
            branches: list.branches,
            delete_remote: true,
            ..CleanupPlan::default()
        })
        .unwrap()
}

fn remote_has(origin: &TempDir, name: &str) -> bool {
    sh(origin.path(), &["branch"]).contains(name)
}

#[test]
fn the_remote_branch_is_offered_for_deletion_only_with_its_scanned_tip() {
    let (_origin, repo) = merged_and_pushed();
    let list = scan(&repo, &tips_of(&repo));
    assert_eq!(
        list.branches[0].remote_tip,
        Some(repo.sh(&["rev-parse", "origin/feat"]))
    );
}

#[test]
fn an_upstream_with_another_name_keeps_the_remote_branch() {
    let (origin, repo) = merged_and_pushed();
    repo.sh(&["push", "-q", "origin", "feat:other"]);
    repo.sh(&["branch", "-q", "--set-upstream-to=origin/other", "feat"]);
    repo.git.fetch(None).unwrap();
    let list = scan(&repo, &tips_of(&repo));
    assert_eq!(list.branches[0].remote_tip, None);
    let report = clean_with_remote(&repo, list);
    assert!(report.all_done(), "{report:?}");
    assert!(remote_has(&origin, "other"));
}

#[test]
fn a_remote_branch_with_an_extra_commit_stays() {
    let (origin, repo) = merged_and_pushed();
    let other = clone_of(&origin);
    other.sh(&["checkout", "-q", "feat"]);
    other.commit_file("more.txt", "m\n", "after the merge");
    other.sh(&["push", "-q", "origin", "feat"]);
    repo.git.fetch(None).unwrap();
    let list = scan(&repo, &tips_of(&repo));
    assert_eq!(list.branches[0].remote_tip, None);
    let report = clean_with_remote(&repo, list);
    assert!(report.all_done(), "{report:?}");
    assert!(remote_has(&origin, "feat"));
}

#[test]
fn a_remote_branch_that_is_the_upstream_of_a_second_branch_stays() {
    let (origin, repo) = merged_and_pushed();
    repo.sh(&["branch", "-q", "second", "feat"]);
    repo.sh(&["branch", "-q", "--set-upstream-to=origin/feat", "second"]);
    let list = scan(&repo, &tips_of(&repo));
    assert!(list.branches.iter().all(|row| row.remote_tip.is_none()));
    let report = clean_with_remote(&repo, list);
    assert!(report.all_done(), "{report:?}");
    assert!(remote_has(&origin, "feat"));
}

#[test]
fn nothing_goes_from_the_remote_when_the_fetch_did_not_work() {
    let (_origin, repo) = merged_and_pushed();
    let state = remote_state(repo.path(), false, &AtomicBool::new(false)).unwrap();
    let list = cleanup_list(&refs(repo.path(), &go()).unwrap(), &tips_of(&repo), &state);
    assert_eq!(list.branches[0].remote_tip, None);
}

#[test]
fn a_remote_branch_that_moved_between_the_scan_and_the_delete_stays() {
    let (origin, repo) = merged_and_pushed();
    let list = scan(&repo, &tips_of(&repo));
    let other = clone_of(&origin);
    other.sh(&["checkout", "-q", "feat"]);
    other.commit_file("late.txt", "l\n", "pushed after the scan");
    other.sh(&["push", "-q", "origin", "feat"]);

    let report = clean_with_remote(&repo, list);
    let remote_row = report
        .rows
        .iter()
        .find(|row| row.target == CleanupTarget::RemoteBranch("origin/feat".to_string()))
        .unwrap();
    assert_eq!(remote_row.result, Err(ActionError::LeaseRejected));
    assert!(remote_has(&origin, "feat"));
}
