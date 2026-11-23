//! Merge, rebase, conflict resolution and the stash.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use git_cli::{read, repo, write, Repo};
use ncrs_git::{ActionError, ConflictChoice, ConflictKind, IntegrateOutcome, Integration};

fn lines(first: &str, last: &str) -> String {
    let middle: String = (2..=9).map(|n| format!("mid {n}\n")).collect();
    format!("{first}\n{middle}{last}\n")
}

/// `main` and `feature` both changed the first and the last line of `c.txt`.
fn diverged() -> Repo {
    let repo = repo();
    repo.commit_file("c.txt", &lines("start", "end"), "base");
    repo.git.create_branch("feature", "main", true).unwrap();
    repo.commit_file("c.txt", &lines("F1", "F10"), "feature side");
    repo.git.checkout("main").unwrap();
    repo.commit_file("c.txt", &lines("M1", "M10"), "main side");
    repo
}

fn conflicted_files(repo: &Repo) -> Vec<ncrs_git::ConflictedFile> {
    repo.git.conflicts().unwrap()
}

#[test]
fn clean_merge_completes() {
    let repo = repo();
    repo.git.create_branch("side", "main", true).unwrap();
    repo.commit_file("s.txt", "s\n", "side");
    repo.git.checkout("main").unwrap();
    assert_eq!(repo.git.merge("side").unwrap(), IntegrateOutcome::Completed);
    assert_eq!(read(repo.path(), "s.txt"), "s\n");
}

#[test]
fn merge_with_conflicts_resolved_per_block_then_continued() {
    let repo = diverged();
    let IntegrateOutcome::Conflicts(files) = repo.git.merge("feature").unwrap() else {
        panic!("expected conflicts");
    };
    assert_eq!(repo.git.integration_in_progress(), Some(Integration::Merge));
    assert_eq!(files.len(), 1);
    let file = &files[0];
    assert_eq!(file.path, "c.txt");
    assert_eq!(file.kind, ConflictKind::BothModified);
    assert_eq!(file.blocks.len(), 2);
    assert_eq!(file.blocks[0].ours, "M1\n");
    assert_eq!(file.blocks[0].theirs, "F1\n");
    assert_eq!(file.blocks[1].ours_label, "HEAD");
    assert_eq!(file.blocks[1].theirs_label, "feature");

    assert!(matches!(
        repo.git.continue_integration(),
        Err(ActionError::UnresolvedConflicts { count: 1 })
    ));
    assert!(matches!(
        repo.git.mark_resolved("c.txt"),
        Err(ActionError::UnresolvedConflicts { count: 2 })
    ));

    repo.git
        .resolve_block("c.txt", 0, ConflictChoice::Ours)
        .unwrap();
    let rest = conflicted_files(&repo);
    assert_eq!(rest[0].blocks.len(), 1);
    repo.git
        .resolve_block("c.txt", 0, ConflictChoice::Theirs)
        .unwrap();
    repo.git.mark_resolved("c.txt").unwrap();
    assert!(conflicted_files(&repo).is_empty());

    assert_eq!(
        repo.git.continue_integration().unwrap(),
        IntegrateOutcome::Completed
    );
    assert_eq!(repo.git.integration_in_progress(), None);
    assert_eq!(read(repo.path(), "c.txt"), lines("M1", "F10"));
    assert_eq!(
        repo.sh(&["rev-list", "--parents", "-n", "1", "HEAD"])
            .split(' ')
            .count(),
        3
    );
}

#[test]
fn diff3_style_conflicts_show_the_base() {
    let repo = diverged();
    repo.sh(&["config", "merge.conflictStyle", "diff3"]);
    repo.git.merge("feature").unwrap();
    let files = conflicted_files(&repo);
    assert_eq!(files[0].blocks[0].base.as_deref(), Some("start\n"));
    repo.git
        .resolve_file("c.txt", ConflictChoice::Both)
        .unwrap();
    assert_eq!(
        read(repo.path(), "c.txt"),
        format!(
            "M1\nF1\n{}M10\nF10\n",
            (2..=9).map(|n| format!("mid {n}\n")).collect::<String>()
        )
    );
}

#[test]
fn resolve_whole_file_as_theirs_and_reset_brings_the_markers_back() {
    let repo = diverged();
    repo.git.merge("feature").unwrap();
    repo.git
        .resolve_file("c.txt", ConflictChoice::Theirs)
        .unwrap();
    assert_eq!(read(repo.path(), "c.txt"), lines("F1", "F10"));
    repo.git.mark_resolved("c.txt").unwrap();
    assert!(conflicted_files(&repo).is_empty());

    repo.git.reset_file("c.txt").unwrap();
    assert_eq!(conflicted_files(&repo)[0].blocks.len(), 2);
}

#[test]
fn abort_returns_to_before_the_merge() {
    let repo = diverged();
    let before = repo.sh(&["rev-parse", "HEAD"]);
    repo.git.merge("feature").unwrap();
    repo.git.abort_integration().unwrap();
    assert_eq!(repo.git.integration_in_progress(), None);
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), before);
    assert_eq!(read(repo.path(), "c.txt"), lines("M1", "M10"));
}

#[test]
fn rebase_with_conflicts_can_be_aborted() {
    let repo = diverged();
    repo.git.checkout("feature").unwrap();
    let before = repo.sh(&["rev-parse", "HEAD"]);
    let IntegrateOutcome::Conflicts(files) = repo.git.rebase("main").unwrap() else {
        panic!("expected conflicts");
    };
    assert_eq!(files[0].blocks.len(), 2);
    assert_eq!(
        repo.git.integration_in_progress(),
        Some(Integration::Rebase)
    );

    repo.git.abort_integration().unwrap();
    assert_eq!(repo.git.integration_in_progress(), None);
    assert_eq!(repo.git.current_branch().as_deref(), Some("feature"));
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), before);
}

#[test]
fn rebase_continues_after_resolving() {
    let repo = diverged();
    repo.git.checkout("feature").unwrap();
    repo.git.rebase("main").unwrap();
    repo.git
        .resolve_file("c.txt", ConflictChoice::Both)
        .unwrap();
    repo.git.mark_resolved("c.txt").unwrap();
    assert_eq!(
        repo.git.continue_integration().unwrap(),
        IntegrateOutcome::Completed
    );
    assert_eq!(repo.git.integration_in_progress(), None);
    assert_eq!(repo.sh(&["rev-list", "--count", "main..feature"]), "1");
}

#[test]
fn delete_modify_conflicts_resolve_to_either_side() {
    let repo = repo();
    repo.commit_file("d.txt", "d\n", "add d");
    repo.git.create_branch("edit", "main", true).unwrap();
    repo.commit_file("d.txt", "d edited\n", "edit d");
    repo.git.checkout("main").unwrap();
    repo.sh(&["rm", "-q", "d.txt"]);
    repo.sh(&["commit", "-q", "-m", "remove d"]);

    repo.git.merge("edit").unwrap();
    let files = conflicted_files(&repo);
    assert_eq!(files[0].kind, ConflictKind::DeletedByUs);
    assert!(files[0].blocks.is_empty());

    repo.git
        .resolve_file("d.txt", ConflictChoice::Theirs)
        .unwrap();
    assert_eq!(read(repo.path(), "d.txt"), "d edited\n");
    assert!(conflicted_files(&repo).is_empty());
    repo.git.abort_integration().unwrap();

    repo.git.merge("edit").unwrap();
    repo.git
        .resolve_file("d.txt", ConflictChoice::Ours)
        .unwrap();
    assert!(!repo.path().join("d.txt").exists());
    assert_eq!(
        repo.git.continue_integration().unwrap(),
        IntegrateOutcome::Completed
    );
}

#[test]
fn stash_is_addressed_by_sha_after_the_list_shifted() {
    let repo = repo();
    write(repo.path(), "a.txt", "first stash\n");
    let first = repo
        .git
        .stash_push("first", false)
        .unwrap()
        .expect("stashed");
    assert_eq!(read(repo.path(), "a.txt"), "one\ntwo\nthree\n");
    write(repo.path(), "a.txt", "second stash\n");
    repo.git
        .stash_push("second", false)
        .unwrap()
        .expect("stashed");
    write(repo.path(), "u.txt", "untracked\n");
    let third = repo
        .git
        .stash_push("third", true)
        .unwrap()
        .expect("stashed");
    assert!(!repo.path().join("u.txt").exists());

    let listed = repo.git.stashes().unwrap();
    assert_eq!(listed.len(), 3);
    assert_eq!(listed[0].sha, third.sha);
    assert_eq!(listed[2].sha, first.sha);
    assert!(listed[2].message.contains("first"));

    // `first` sits at stash@{2}; dropping the top one shifts it to stash@{1}.
    repo.git.stash_drop(&third.sha).unwrap();
    assert_eq!(repo.git.stashes().unwrap().len(), 2);

    assert_eq!(
        repo.git.stash_apply(&first.sha).unwrap(),
        IntegrateOutcome::Completed
    );
    assert_eq!(read(repo.path(), "a.txt"), "first stash\n");
    assert_eq!(repo.git.stashes().unwrap().len(), 2);

    repo.sh(&["checkout", "--", "."]);
    assert_eq!(
        repo.git.stash_pop(&first.sha).unwrap(),
        IntegrateOutcome::Completed
    );
    let left = repo.git.stashes().unwrap();
    assert_eq!(left.len(), 1);
    assert!(left[0].message.contains("second"));
    assert!(matches!(
        repo.git.stash_drop(&first.sha),
        Err(ActionError::Other { .. })
    ));
}

#[test]
fn stash_push_with_nothing_to_stash_is_none() {
    let repo = repo();
    assert_eq!(repo.git.stash_push("nothing", false).unwrap(), None);
}

#[test]
fn stash_pop_with_conflicts_keeps_the_stash() {
    let repo = repo();
    write(repo.path(), "a.txt", "one\nstashed\nthree\n");
    let stash = repo
        .git
        .stash_push("work", false)
        .unwrap()
        .expect("stashed");
    repo.commit_file("a.txt", "one\ncommitted\nthree\n", "meanwhile");

    let IntegrateOutcome::Conflicts(files) = repo.git.stash_pop(&stash.sha).unwrap() else {
        panic!("expected conflicts");
    };
    assert_eq!(files[0].path, "a.txt");
    assert_eq!(repo.git.stashes().unwrap().len(), 1);
}
