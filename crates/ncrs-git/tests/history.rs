#![allow(
    clippy::tests_outside_test_module,
    reason = "an integration test file: the tests are the file"
)]

mod common;

use std::fs;

use common::{
    commit_files, current_branch, not_stopped, point_head_at, repository_with, track_upstream,
    write_branch, write_file,
};
use ncrs_git::{
    commit_detail, file_diff, log, refs, ChangeKind, DiffTarget, GitError, RefKind, RowKind,
    SegmentHalf,
};

#[test]
fn local_branches_know_their_upstream_distance_and_whether_main_has_them() {
    let (dir, repository, first) = repository_with(&[("a.txt", "a\n")]);
    let main = current_branch(&repository);
    let second = commit_files(&repository, dir.path(), &[("a.txt", "a\nb\n")], &[first], 2);
    track_upstream(&repository, &main, first);
    write_branch(&repository, "done", first);
    write_branch(&repository, "ahead-of-main", second);
    write_branch(&repository, "other", first);

    let overview = refs(dir.path(), &not_stopped()).expect("refs");
    let branch = |name: &str| {
        overview
            .local
            .iter()
            .find(|branch| branch.name == name)
            .unwrap_or_else(|| panic!("no branch {name}"))
            .clone()
    };
    assert_eq!(overview.main_branch.as_deref(), Some("main"));
    let current = branch(&main);
    assert!(current.is_current && current.has_upstream);
    assert_eq!((current.ahead, current.behind), (1, 0));
    assert!(!current.merged);
    assert!(branch("done").merged);
    assert!(!branch("done").has_upstream);
}

#[test]
fn tags_remote_branches_and_stashes_are_listed() {
    let (dir, repository, first) = repository_with(&[("a.txt", "a\n")]);
    let main = current_branch(&repository);
    track_upstream(&repository, &main, first);
    repository
        .tag_reference("v1.0", first, gix::refs::transaction::PreviousValue::Any)
        .expect("tag");
    let logs = repository.git_dir().join("logs/refs");
    fs::create_dir_all(&logs).expect("logs");
    fs::write(logs.join("stash"), "0 1 Test <t@e> 1 +0000\tWIP\n").expect("stash log");

    let overview = refs(dir.path(), &not_stopped()).expect("refs");
    assert_eq!(overview.tags, vec!["v1.0".to_string()]);
    assert_eq!(overview.remote, vec![format!("origin/{main}")]);
    assert_eq!(overview.stashes, 1);
}

#[test]
fn a_worktree_counts_its_changes_and_its_distance_to_main() {
    let (dir, repository, first) = repository_with(&[("a.txt", "a\n")]);
    let second = commit_files(&repository, dir.path(), &[("a.txt", "a\nb\n")], &[first], 2);
    write_branch(&repository, "feature", first);
    let main = current_branch(&repository);
    write_branch(&repository, &main, second);

    // A linked worktree on `feature`, laid out the way git does it.
    let linked = tempfile::TempDir::new().expect("tempdir");
    let admin = repository.git_dir().join("worktrees/linked");
    fs::create_dir_all(&admin).expect("admin folder");
    fs::write(
        admin.join("gitdir"),
        format!("{}\n", linked.path().join(".git").display()),
    )
    .expect("gitdir");
    fs::write(admin.join("HEAD"), "ref: refs/heads/feature\n").expect("HEAD");
    fs::write(admin.join("commondir"), "../..\n").expect("commondir");
    fs::write(
        linked.path().join(".git"),
        format!("gitdir: {}\n", admin.display()),
    )
    .expect("git file");
    write_file(linked.path(), "a.txt", "a\n");
    let linked_repository = gix::open(linked.path()).expect("open linked");
    let tree = linked_repository
        .find_commit(first)
        .expect("commit")
        .tree()
        .expect("tree")
        .id;
    let mut index = linked_repository.index_from_tree(&tree).expect("index");
    index.write(Default::default()).expect("write index");
    write_file(linked.path(), "new.txt", "new\n");

    let overview = refs(dir.path(), &not_stopped()).expect("refs");
    assert_eq!(overview.worktrees.len(), 2);
    let main_tree = overview
        .worktrees
        .iter()
        .find(|worktree| worktree.is_main)
        .expect("main worktree");
    assert!(main_tree.is_current);
    let feature = overview
        .worktrees
        .iter()
        .find(|worktree| !worktree.is_main)
        .expect("linked worktree");
    assert_eq!(feature.branch.as_deref(), Some("feature"));
    assert_eq!(feature.wip, 1);
    assert_eq!((feature.ahead, feature.behind), (0, 1));
    assert!(feature.merged);
}

#[test]
fn the_lanes_fork_and_merge_back() {
    let (dir, repository, first) = repository_with(&[("a.txt", "a\n")]);
    let main = current_branch(&repository);
    let second = commit_files(
        &repository,
        dir.path(),
        &[("a.txt", "a\nmain\n")],
        &[first],
        2,
    );
    write_branch(&repository, "feature", first);
    point_head_at(&repository, "feature");
    let third = commit_files(
        &repository,
        dir.path(),
        &[("a.txt", "a\nfeature\n")],
        &[first],
        3,
    );
    point_head_at(&repository, &main);
    let merge = commit_files(
        &repository,
        dir.path(),
        &[("a.txt", "a\nboth\n")],
        &[second, third],
        4,
    );

    let history = log(dir.path(), 100, "", &not_stopped()).expect("log");
    let ids: Vec<String> = history
        .commits
        .iter()
        .map(|commit| commit.id.clone())
        .collect();
    let expected: Vec<String> = [merge, third, second, first]
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(ids, expected);
    assert!(!history.has_more);
    assert_eq!(history.lane_count, 2);

    let lane = |index: usize| history.commits[index].graph.lane;
    assert_eq!([lane(0), lane(1), lane(2), lane(3)], [0, 1, 0, 1]);
    assert!(history.commits[0].is_merge);
    let lower = |index: usize| -> Vec<(usize, usize)> {
        history.commits[index]
            .graph
            .segments
            .iter()
            .filter(|segment| segment.half == SegmentHalf::Lower)
            .map(|segment| (segment.from, segment.to))
            .collect()
    };
    assert_eq!(lower(0), vec![(0, 0), (0, 1)]);
    assert_eq!(lower(2), vec![(0, 1)]);
    assert!(history.commits[1]
        .refs
        .iter()
        .any(|label| label.name == "feature" && label.kind == RefKind::Branch));
}

#[test]
fn the_log_comes_in_pages_and_can_be_filtered() {
    let (dir, repository, first) = repository_with(&[("a.txt", "a\n")]);
    let second = commit_files(&repository, dir.path(), &[("a.txt", "b\n")], &[first], 2);
    let third = commit_files(&repository, dir.path(), &[("a.txt", "c\n")], &[second], 3);

    let page = log(dir.path(), 2, "", &not_stopped()).expect("log");
    assert_eq!(page.commits.len(), 2);
    assert!(page.has_more);
    assert_eq!(page.commits[0].id, third.to_string());
    assert_eq!(page.commits[0].summary, "commit 3");

    let by_message = log(dir.path(), 10, "commit 2", &not_stopped()).expect("log");
    assert_eq!(by_message.commits.len(), 1);
    let by_author = log(dir.path(), 10, "TEST", &not_stopped()).expect("log");
    assert_eq!(by_author.commits.len(), 3);
    let by_sha = log(dir.path(), 10, &first.to_string()[..6], &not_stopped()).expect("log");
    assert_eq!(by_sha.commits.len(), 1);
    assert_eq!(by_sha.commits[0].id, first.to_string());
    assert_eq!(by_sha.commits[0].graph.segments.len(), 0);
}

#[test]
fn a_raised_stop_flag_ends_the_history() {
    let (dir, _repository, _) = repository_with(&[("a.txt", "a\n")]);
    let stop = not_stopped();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(log(dir.path(), 10, "", &stop), Err(GitError::Interrupted));
    assert_eq!(refs(dir.path(), &stop), Err(GitError::Interrupted));
}

#[test]
fn a_commit_lists_its_files_with_line_counts() {
    let (dir, repository, first) = repository_with(&[("a.txt", "one\n"), ("b.txt", "x\n")]);
    let second = commit_files(
        &repository,
        dir.path(),
        &[("a.txt", "one\ntwo\n"), ("c.txt", "p\nq\n")],
        &[first],
        2,
    );
    let detail = commit_detail(dir.path(), &second.to_string(), &not_stopped()).expect("detail");
    assert_eq!(detail.parents, vec![first.to_string()]);
    assert_eq!(detail.message, "commit 2");
    let summary: Vec<_> = detail
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.kind, file.added, file.removed))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("a.txt", ChangeKind::Modified, 1, 0),
            ("b.txt", ChangeKind::Deleted, 0, 1),
            ("c.txt", ChangeKind::Added, 2, 0),
        ]
    );
    let root = commit_detail(dir.path(), &first.to_string(), &not_stopped()).expect("root");
    assert!(root.parents.is_empty());
    assert_eq!(root.files.len(), 2);
}

fn numbered(replace_fifth: Option<&str>) -> String {
    (1..=10)
        .map(|number| match (number, replace_fifth) {
            (5, Some(text)) => format!("{text}\n"),
            _ => format!("{number}\n"),
        })
        .collect()
}

#[test]
fn a_diff_has_one_hunk_with_three_lines_of_context() {
    let before = numbered(None);
    let after = numbered(Some("five"));
    let (dir, repository, first) = repository_with(&[("a.txt", before.as_str())]);
    let second = commit_files(
        &repository,
        dir.path(),
        &[("a.txt", after.as_str())],
        &[first],
        2,
    );
    let target = DiffTarget {
        commit: Some(second.to_string()),
        path: "a.txt".to_string(),
        old_path: None,
    };
    let diff = file_diff(dir.path(), &target, false, &not_stopped()).expect("diff");
    assert_eq!((diff.added, diff.removed), (1, 1));
    assert_eq!(diff.hunks.len(), 1);
    let hunk = &diff.hunks[0];
    assert_eq!((hunk.old_start, hunk.new_start), (2, 2));
    assert_eq!(hunk.rows.len(), 8);
    let removed: Vec<_> = hunk
        .rows
        .iter()
        .filter(|row| row.kind == RowKind::Removed)
        .collect();
    let added: Vec<_> = hunk
        .rows
        .iter()
        .filter(|row| row.kind == RowKind::Added)
        .collect();
    assert_eq!(
        (removed[0].text.as_str(), removed[0].old_line),
        ("5", Some(5))
    );
    assert_eq!(
        (added[0].text.as_str(), added[0].new_line),
        ("five", Some(5))
    );
    assert_eq!(hunk.rows[0].old_line, Some(2));
    assert_eq!(hunk.rows[7].new_line, Some(8));
}

#[test]
fn ignoring_whitespace_hides_a_pure_reindent() {
    let (dir, repository, first) = repository_with(&[("a.txt", "a\n  b\nc\n")]);
    let second = commit_files(
        &repository,
        dir.path(),
        &[("a.txt", "a\nb\nc\n")],
        &[first],
        2,
    );
    let target = DiffTarget {
        commit: Some(second.to_string()),
        path: "a.txt".to_string(),
        old_path: None,
    };
    let strict = file_diff(dir.path(), &target, false, &not_stopped()).expect("diff");
    assert_eq!(strict.hunks.len(), 1);
    let loose = file_diff(dir.path(), &target, true, &not_stopped()).expect("diff");
    assert!(loose.hunks.is_empty());
    assert_eq!((loose.added, loose.removed), (0, 0));
}

#[test]
fn the_working_copy_is_diffed_against_head() {
    let (dir, _repository, _) = repository_with(&[("a.txt", "a\nb\n")]);
    write_file(dir.path(), "a.txt", "a\nb\nc\n");
    let target = DiffTarget {
        commit: None,
        path: "a.txt".to_string(),
        old_path: None,
    };
    let diff = file_diff(dir.path(), &target, false, &not_stopped()).expect("diff");
    assert_eq!((diff.added, diff.removed), (1, 0));
    assert_eq!(
        diff.hunks[0].rows.last().map(|row| row.kind),
        Some(RowKind::Added)
    );
}

#[test]
fn a_binary_file_has_no_hunks() {
    let (dir, _repository, _) = repository_with(&[("a.txt", "a\n")]);
    fs::write(dir.path().join("bin.dat"), b"\x00\x01\x02").expect("binary");
    let target = DiffTarget {
        commit: None,
        path: "bin.dat".to_string(),
        old_path: None,
    };
    let diff = file_diff(dir.path(), &target, false, &not_stopped()).expect("diff");
    assert!(diff.binary);
    assert!(diff.hunks.is_empty());
}
