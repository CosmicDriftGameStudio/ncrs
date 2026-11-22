//! Stage, hunks, discard, commit and the undo of each.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use git_cli::{numbered, read, repo, write};
use ncrs_git::{ActionError, CommitMessage, GitCli, HunkSide, UndoSummary};

#[test]
fn git_is_found_on_the_test_machine() {
    assert!(GitCli::locate().is_available());
}

#[test]
fn stage_and_unstage_with_undo() {
    let repo = repo();
    write(repo.path(), "new.txt", "hi\n");
    write(repo.path(), "a.txt", "changed\n");

    let staged = repo.git.stage(&["new.txt"]).unwrap().expect("undo");
    assert_eq!(staged.summary, UndoSummary::Stage { files: 1 });
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "new.txt");
    repo.git.undo(&staged).unwrap();
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "");

    repo.git.stage_all().unwrap();
    assert_eq!(
        repo.sh(&["diff", "--cached", "--name-only"]),
        "a.txt\nnew.txt"
    );
    let unstaged = repo.git.unstage(&["a.txt"]).unwrap().expect("undo");
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "new.txt");
    repo.git.undo(&unstaged).unwrap();
    assert_eq!(
        repo.sh(&["diff", "--cached", "--name-only"]),
        "a.txt\nnew.txt"
    );
    repo.git.unstage_all().unwrap();
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "");
}

#[test]
fn stage_undo_is_refused_when_the_index_moved_on() {
    let repo = repo();
    write(repo.path(), "x.txt", "x\n");
    write(repo.path(), "y.txt", "y\n");
    let first = repo.git.stage(&["x.txt"]).unwrap().expect("undo");
    repo.git.stage(&["y.txt"]).unwrap();
    assert!(matches!(
        repo.git.undo(&first),
        Err(ActionError::UndoNotSafe(_))
    ));
}

fn two_hunk_file(ending: &str) -> (String, String) {
    let original = numbered(20, ending);
    let changed = original
        .replacen(
            &format!("line 2{ending}"),
            &format!("line 2 edited{ending}"),
            1,
        )
        .replacen(
            &format!("line 18{ending}"),
            &format!("line 18 edited{ending}"),
            1,
        );
    (original, changed)
}

#[test]
fn stage_unstage_and_discard_single_hunks() {
    let repo = repo();
    let (original, changed) = two_hunk_file("\n");
    repo.commit_file("n.txt", &original, "numbers");
    write(repo.path(), "n.txt", &changed);

    let diff = repo.git.hunks("n.txt", HunkSide::Unstaged).unwrap();
    assert_eq!(diff.hunks.len(), 2);
    let (first, second) = (&diff.hunks[0], &diff.hunks[1]);

    repo.git.stage_hunk("n.txt", second).unwrap().expect("undo");
    let cached = repo.sh(&["diff", "--cached", "-U0", "n.txt"]);
    assert!(cached.contains("+line 18 edited"), "{cached}");
    assert!(!cached.contains("line 2 edited"), "{cached}");

    let staged = repo.git.hunks("n.txt", HunkSide::Staged).unwrap();
    assert_eq!(staged.hunks.len(), 1);
    repo.git.unstage_hunk("n.txt", &staged.hunks[0]).unwrap();
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "");

    // The first hunk was computed before the index changed twice; recompute as the view would.
    let fresh = repo.git.hunks("n.txt", HunkSide::Unstaged).unwrap();
    assert_eq!(fresh.hunks.len(), 2);
    assert_eq!(&fresh.hunks[0], first);
    let undo = repo.git.discard_hunk("n.txt", &fresh.hunks[0]).unwrap();
    let after = read(repo.path(), "n.txt");
    assert!(after.contains("line 2\n"), "{after}");
    assert!(after.contains("line 18 edited"), "{after}");
    repo.git.undo(&undo).unwrap();
    assert_eq!(read(repo.path(), "n.txt"), changed);
}

#[test]
fn hunks_keep_windows_line_endings() {
    let repo = repo();
    let (original, changed) = two_hunk_file("\r\n");
    repo.sh(&["config", "core.autocrlf", "false"]);
    repo.commit_file("w.txt", &original, "crlf");
    write(repo.path(), "w.txt", &changed);
    let diff = repo.git.hunks("w.txt", HunkSide::Unstaged).unwrap();
    repo.git.stage_hunk("w.txt", &diff.hunks[0]).unwrap();
    let staged = repo.sh(&["show", ":w.txt"]);
    assert!(staged.contains("line 2 edited\r\n"), "{staged:?}");
    assert!(staged.contains("line 18\r\n"), "{staged:?}");
}

#[test]
fn hunks_work_on_a_file_without_final_newline() {
    let repo = repo();
    let original = numbered(20, "\n").trim_end().to_string();
    repo.commit_file("e.txt", &original, "no newline");
    let changed = original.replace("line 20", "line 20 edited");
    write(repo.path(), "e.txt", &changed);
    let diff = repo.git.hunks("e.txt", HunkSide::Unstaged).unwrap();
    repo.git.stage_hunk("e.txt", &diff.hunks[0]).unwrap();
    assert_eq!(repo.sh(&["show", ":e.txt"]), changed);
}

#[test]
fn discard_file_saves_and_undoes_even_for_untracked_files() {
    let repo = repo();
    write(repo.path(), "a.txt", "mine\n");
    write(repo.path(), "sub/new.txt", "fresh\n");

    let tracked = repo.git.discard_file("a.txt").unwrap();
    assert_eq!(read(repo.path(), "a.txt"), "one\ntwo\nthree\n");
    repo.git.undo(&tracked).unwrap();
    assert_eq!(read(repo.path(), "a.txt"), "mine\n");

    let untracked = repo.git.discard_file("sub/new.txt").unwrap();
    assert!(!repo.path().join("sub/new.txt").exists());
    repo.git.undo(&untracked).unwrap();
    assert_eq!(read(repo.path(), "sub/new.txt"), "fresh\n");
}

#[test]
fn discard_keeps_staged_changes_and_discard_all_takes_the_rest() {
    let repo = repo();
    write(repo.path(), "a.txt", "staged\n");
    repo.git.stage(&["a.txt"]).unwrap();
    write(repo.path(), "a.txt", "staged\nand more\n");
    write(repo.path(), "u.txt", "untracked\n");

    let undo = repo.git.discard_all().unwrap();
    assert_eq!(read(repo.path(), "a.txt"), "staged\n");
    assert!(!repo.path().join("u.txt").exists());
    assert_eq!(undo.summary, UndoSummary::Discard { files: 2 });
    repo.git.undo(&undo).unwrap();
    assert_eq!(read(repo.path(), "a.txt"), "staged\nand more\n");
    assert_eq!(read(repo.path(), "u.txt"), "untracked\n");
}

#[test]
fn undo_of_a_discard_is_refused_when_the_file_changed_again() {
    let repo = repo();
    write(repo.path(), "a.txt", "mine\n");
    let undo = repo.git.discard_file("a.txt").unwrap();
    write(repo.path(), "a.txt", "newer work\n");
    assert!(matches!(
        repo.git.undo(&undo),
        Err(ActionError::UndoNotSafe(_))
    ));
    assert_eq!(read(repo.path(), "a.txt"), "newer work\n");
}

fn message(title: &str, body: &str) -> CommitMessage {
    CommitMessage {
        title: title.to_string(),
        body: body.to_string(),
    }
}

#[test]
fn commit_with_body_and_undo() {
    let repo = repo();
    let before = repo.sh(&["rev-parse", "HEAD"]);
    write(repo.path(), "b.txt", "b\n");
    repo.git.stage(&["b.txt"]).unwrap();
    let undo = repo
        .git
        .commit(&message("Add b", "Because b."))
        .unwrap()
        .expect("undo");
    assert_eq!(repo.sh(&["log", "-1", "--format=%s"]), "Add b");
    assert_eq!(repo.sh(&["log", "-1", "--format=%b"]), "Because b.");

    repo.git.undo(&undo).unwrap();
    assert_eq!(repo.sh(&["rev-parse", "HEAD"]), before);
    assert_eq!(repo.sh(&["diff", "--cached", "--name-only"]), "b.txt");
}

#[test]
fn commit_with_nothing_staged_is_typed() {
    let repo = repo();
    assert_eq!(
        repo.git.commit(&message("Empty", "")),
        Err(ActionError::NothingToCommit)
    );
}

#[test]
fn commit_undo_is_refused_once_pushed() {
    let (_origin, repo) = git_cli::with_origin();
    write(repo.path(), "b.txt", "b\n");
    repo.git.stage(&["b.txt"]).unwrap();
    let undo = repo
        .git
        .commit(&message("Add b", ""))
        .unwrap()
        .expect("undo");
    repo.git.push().unwrap();
    assert!(matches!(
        repo.git.undo(&undo),
        Err(ActionError::UndoNotSafe(reason)) if reason.contains("pushed")
    ));
}

#[test]
fn amend_changes_message_or_content() {
    let repo = repo();
    repo.git
        .amend(Some(&message("Better title", "With body")))
        .unwrap();
    assert_eq!(repo.sh(&["log", "-1", "--format=%s"]), "Better title");
    assert_eq!(repo.sh(&["rev-list", "--count", "HEAD"]), "1");

    write(repo.path(), "c.txt", "c\n");
    repo.git.stage(&["c.txt"]).unwrap();
    repo.git.amend(None).unwrap();
    assert_eq!(repo.sh(&["log", "-1", "--format=%s"]), "Better title");
    assert_eq!(
        repo.sh(&["show", "--name-only", "--format=", "HEAD"]),
        "a.txt\nc.txt"
    );
    assert_eq!(repo.sh(&["rev-list", "--count", "HEAD"]), "1");
}
