//! Comparing, containing branches, the history and blame of a file, and the
//! upstream data of branches and worktrees, read from real repositories.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::tests_outside_test_module,
    reason = "test code: a failing assertion is the signal; an integration test file: the tests are the file"
)]

mod git_cli;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use git_cli::{clone_of, repo, sh, with_origin, write, Repo};
use ncrs_git::{
    blame, branches_containing, commit_detail, compare_diff, compare_files, file_history, refs,
    BlameLimit, ChangeKind, RefKind,
};

fn go() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn head(repo: &Repo) -> String {
    repo.sh(&["rev-parse", "HEAD"])
}

fn paths(files: &[ncrs_git::ChangedFile]) -> Vec<(String, ChangeKind, Option<String>)> {
    files
        .iter()
        .map(|file| (file.path.clone(), file.kind, file.old_path.clone()))
        .collect()
}

#[test]
fn two_commits_are_compared_with_a_rename() {
    let repo = repo();
    let from = head(&repo);
    repo.sh(&["mv", "a.txt", "renamed.txt"]);
    repo.sh(&["commit", "-q", "-m", "rename"]);
    repo.commit_file("new.txt", "n\n", "add");
    let to = head(&repo);

    let files = compare_files(repo.path(), &from, Some(&to), &go()).unwrap();
    assert_eq!(
        paths(&files),
        vec![
            ("new.txt".to_string(), ChangeKind::Added, None),
            ("renamed.txt".to_string(), ChangeKind::Renamed, Some("a.txt".to_string())),
        ]
    );
    let diff = compare_diff(repo.path(), &from, Some(&to), "new.txt", None, false, &go()).unwrap();
    assert_eq!((diff.added, diff.removed), (1, 0));
}

#[test]
fn a_commit_is_compared_with_the_working_copy() {
    let repo = repo();
    let from = head(&repo);
    repo.commit_file("b.txt", "b\n", "add b");
    write(repo.path(), "a.txt", "one\nCHANGED\nthree\n");
    repo.sh(&["mv", "b.txt", "moved.txt"]);
    write(repo.path(), "loose.txt", "x\n");

    let files = compare_files(repo.path(), &from, None, &go()).unwrap();
    assert_eq!(
        paths(&files),
        vec![
            ("a.txt".to_string(), ChangeKind::Modified, None),
            ("loose.txt".to_string(), ChangeKind::Added, None),
            ("moved.txt".to_string(), ChangeKind::Added, None),
        ],
        "b.txt did not exist at `from`, so moved.txt is simply new"
    );
    let diff = compare_diff(repo.path(), &from, None, "a.txt", None, false, &go()).unwrap();
    assert_eq!((diff.added, diff.removed), (1, 1));

    let back = compare_files(repo.path(), &head(&repo), None, &go()).unwrap();
    assert!(back.iter().any(|file| file.kind == ChangeKind::Renamed
        && file.path == "moved.txt"
        && file.old_path.as_deref() == Some("b.txt")));
}

#[test]
fn the_branches_that_contain_a_commit_are_listed() {
    let (origin, repo) = with_origin();
    let base = head(&repo);
    repo.git.create_branch("side", "main", true).unwrap();
    repo.commit_file("s.txt", "s\n", "side");
    let side_commit = head(&repo);
    repo.git.checkout("main").unwrap();
    repo.sh(&["push", "-q", "origin", "side"]);
    let _ = origin;

    let in_base = branches_containing(repo.path(), &base, &go()).unwrap();
    let names: Vec<_> = in_base.iter().map(|label| (label.name.as_str(), label.kind)).collect();
    assert_eq!(
        names,
        vec![
            ("main", RefKind::Branch),
            ("side", RefKind::Branch),
            ("origin/main", RefKind::Remote),
            ("origin/side", RefKind::Remote),
        ]
    );
    let in_side = branches_containing(repo.path(), &side_commit, &go()).unwrap();
    assert_eq!(
        in_side.iter().map(|label| label.name.as_str()).collect::<Vec<_>>(),
        vec!["side", "origin/side"]
    );
}

#[test]
fn branches_know_their_upstream_tip_and_when_it_is_gone() {
    let (origin, repo) = with_origin();
    repo.git.create_branch("feat", "main", true).unwrap();
    repo.commit_file("f.txt", "f\n", "feat");
    repo.sh(&["push", "-q", "-u", "origin", "feat"]);
    repo.git.checkout("main").unwrap();
    repo.sh(&["branch", "plain"]);

    let overview = refs(repo.path(), &go()).unwrap();
    let find = |name: &str| overview.local.iter().find(|branch| branch.name == name).unwrap().clone();
    assert_eq!(find("feat").upstream.as_deref(), Some("origin/feat"));
    assert!(!find("feat").upstream_gone);
    assert_eq!(find("feat").tip, repo.sh(&["rev-parse", "feat"]));
    assert!(find("feat").tip_time > 0);
    assert_eq!(find("plain").upstream, None);
    assert_eq!(overview.main_tip, Some(repo.sh(&["rev-parse", "main"])));

    let other = clone_of(&origin);
    other.sh(&["push", "-q", "origin", "--delete", "feat"]);
    repo.git.fetch(None).unwrap();
    let gone = refs(repo.path(), &go()).unwrap();
    let feat = gone.local.iter().find(|branch| branch.name == "feat").unwrap();
    assert!(feat.upstream_gone && feat.has_upstream);
}

#[test]
fn a_worktree_shows_its_distance_to_its_own_upstream() {
    let (_origin, repo) = with_origin();
    repo.git.create_branch("feat", "main", false).unwrap();
    let folder = repo.path().with_extension("wt");
    repo.sh(&["worktree", "add", "-q", folder.to_str().unwrap(), "feat"]);
    sh(&folder, &["push", "-q", "-u", "origin", "feat"]);
    write(&folder, "w.txt", "w\n");
    sh(&folder, &["add", "w.txt"]);
    sh(&folder, &["commit", "-q", "-m", "w1"]);
    write(&folder, "w2.txt", "w\n");
    sh(&folder, &["add", "w2.txt"]);
    sh(&folder, &["commit", "-q", "-m", "w2"]);

    let overview = refs(repo.path(), &go()).unwrap();
    let linked = overview.worktrees.iter().find(|found| !found.is_main).unwrap();
    assert_eq!(linked.upstream.as_deref(), Some("origin/feat"));
    assert_eq!((linked.upstream_ahead, linked.upstream_behind), (2, 0));
    assert_eq!((linked.ahead, linked.behind), (2, 0), "the distance to main stays as it was");
    assert!(!linked.locked);
    assert!(linked.head.is_some());
}

#[test]
fn a_commit_detail_has_author_and_committer_apart() {
    let repo = repo();
    write(repo.path(), "x.txt", "x\n");
    repo.sh(&["add", "x.txt"]);
    repo.sh(&[
        "-c", "user.name=Committer", "-c", "user.email=c@example.com",
        "commit", "-q", "-m", "by two people",
        "--author=Writer <w@example.com>", "--date=2020-01-01T00:00:00+0000",
    ]);
    let detail = commit_detail(repo.path(), &head(&repo), &go()).unwrap();
    assert_eq!(detail.author, "Writer");
    assert_eq!(detail.committer, "Committer");
    assert_eq!(detail.author_time, 1_577_836_800);
    assert!(detail.committer_time > detail.author_time);
    assert_eq!(detail.time, detail.committer_time);
}

#[test]
fn a_file_history_follows_the_file_across_a_rename_and_pages() {
    let repo = repo();
    repo.commit_file("a.txt", "one\ntwo\nthree\nfour\n", "edit a");
    repo.sh(&["mv", "a.txt", "b.txt"]);
    repo.sh(&["commit", "-q", "-m", "rename to b"]);
    repo.commit_file("other.txt", "o\n", "unrelated");
    repo.commit_file("b.txt", "one\ntwo\nthree\nfour\nfive\n", "edit b");

    let history = file_history(repo.path(), "b.txt", 10, &go()).unwrap();
    let rows: Vec<_> = history
        .commits
        .iter()
        .map(|row| (row.commit.summary.as_str(), row.path_at_commit.as_str(), row.old_path.as_deref()))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("edit b", "b.txt", None),
            ("rename to b", "b.txt", Some("a.txt")),
            ("edit a", "a.txt", None),
            ("first", "a.txt", None),
        ]
    );
    assert!(!history.has_more);

    let page = file_history(repo.path(), "b.txt", 2, &go()).unwrap();
    assert_eq!(page.commits.len(), 2);
    assert!(page.has_more);
    assert_eq!(page.commits[..], history.commits[..2]);
    assert!(file_history(repo.path(), "nope.txt", 10, &go()).unwrap().commits.is_empty());
}

#[test]
fn blame_names_the_commit_of_every_line_and_follows_renames() {
    let repo = repo();
    let first = head(&repo);
    repo.commit_file("a.txt", "one\nTWO\nthree\nfour\n", "change line two");
    let second = head(&repo);
    repo.sh(&["mv", "a.txt", "b.txt"]);
    repo.sh(&["commit", "-q", "-m", "rename"]);

    let result = blame(repo.path(), "b.txt", None, &go()).unwrap();
    assert_eq!(result.refused, BlameLimit::None);
    let by_line: Vec<_> = result
        .lines
        .iter()
        .map(|line| (line.line_no, line.text.as_str(), result.groups[line.group].sha.as_str()))
        .collect();
    assert_eq!(by_line[0], (1, "one", first.as_str()));
    assert_eq!(by_line[1], (2, "TWO", second.as_str()));
    assert_eq!(by_line[2], (3, "three", first.as_str()));
    assert_eq!(by_line.len(), 4, "the last line was added in the second commit");
    assert_eq!(by_line[3].2, second);
    let group = &result.groups[result.lines[0].group];
    assert_eq!(group.path_then.as_deref(), Some("a.txt"));
    assert_eq!(group.author, "Test");
    assert_eq!(group.summary, "first");

    let earlier = blame(repo.path(), "a.txt", Some(&first), &go()).unwrap();
    assert!(earlier.groups.iter().all(|earlier_group| earlier_group.sha == first));
}

#[test]
fn blame_refuses_binary_files_and_honours_stop() {
    let repo = repo();
    std::fs::write(repo.path().join("blob.bin"), b"\0\x01\x02\0").unwrap();
    repo.sh(&["add", "blob.bin"]);
    repo.sh(&["commit", "-q", "-m", "binary"]);
    let result = blame(repo.path(), "blob.bin", None, &go()).unwrap();
    assert_eq!(result.refused, BlameLimit::Binary);
    assert!(result.lines.is_empty());
    blame(repo.path(), "a.txt", None, &Arc::new(AtomicBool::new(true))).unwrap_err();
}
