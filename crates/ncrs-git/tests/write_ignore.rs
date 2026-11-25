//! The Ignore action: a line in the root .gitignore, with undo.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use git_cli::{read, repo, write};
use ncrs_git::literal_pattern;

#[test]
fn creates_the_file_and_hides_the_untracked_file() {
    let repo = repo();
    write(repo.path(), "build.log", "x\n");
    assert!(repo
        .git
        .changes()
        .unwrap()
        .unstaged
        .iter()
        .any(|file| file.path == "build.log" && file.untracked));

    let undo = repo.git.ignore_pattern("*.log").unwrap();
    assert!(undo.is_some());
    assert_eq!(read(repo.path(), ".gitignore"), "*.log\n");
    assert!(repo
        .git
        .changes()
        .unwrap()
        .unstaged
        .iter()
        .all(|file| file.path != "build.log"));

    repo.git.undo(&undo.unwrap()).unwrap();
    assert!(!repo.path().join(".gitignore").exists());
}

#[test]
fn appends_after_a_missing_line_break_and_restores_the_old_text() {
    let repo = repo();
    write(repo.path(), ".gitignore", "target");
    let undo = repo.git.ignore_pattern("/notes.txt").unwrap().unwrap();
    assert_eq!(read(repo.path(), ".gitignore"), "target\n/notes.txt\n");
    repo.git.undo(&undo).unwrap();
    assert_eq!(read(repo.path(), ".gitignore"), "target");
}

#[test]
fn a_known_line_is_a_no_op() {
    let repo = repo();
    write(repo.path(), ".gitignore", "a\r\nb\r\n");
    assert_eq!(repo.git.ignore_pattern("a").unwrap(), None);
    repo.git.ignore_pattern("c").unwrap();
    assert_eq!(read(repo.path(), ".gitignore"), "a\r\nb\r\nc\r\n");
}

#[test]
fn literal_patterns_escape_what_git_would_read_as_a_pattern() {
    assert_eq!(literal_pattern("src/main.rs"), "/src/main.rs");
    assert_eq!(
        literal_pattern("dir\\file #1[x]!.txt"),
        "/dir/file \\#1\\[x]\\!.txt"
    );
    assert_eq!(literal_pattern("a*b?.txt"), "/a\\*b\\?.txt");
}

#[test]
fn an_escaped_name_really_matches_only_that_file() {
    let repo = repo();
    write(repo.path(), "a*b.txt", "x\n");
    write(repo.path(), "aXb.txt", "x\n");
    repo.git
        .ignore_pattern(&literal_pattern("a*b.txt"))
        .unwrap();
    let names: Vec<String> = repo
        .git
        .changes()
        .unwrap()
        .unstaged
        .into_iter()
        .map(|file| file.path)
        .collect();
    assert!(names.contains(&"aXb.txt".to_string()));
    assert!(!names.contains(&"a*b.txt".to_string()));
}

#[cfg(unix)]
#[test]
fn a_symlink_as_gitignore_is_refused() {
    let repo = repo();
    write(repo.path(), "real", "keep\n");
    std::os::unix::fs::symlink(repo.path().join("real"), repo.path().join(".gitignore")).unwrap();
    repo.git.ignore_pattern("x").unwrap_err();
    assert_eq!(read(repo.path(), "real"), "keep\n");
}
