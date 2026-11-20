//! Fixture repositories are built with gix itself, so no git binary is needed.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gix::actor::SignatureRef;
use gix::bstr::BStr;
use gix::object::tree::EntryKind;
use gix::ObjectId;
use ncrs_git::{
    is_repository_root, repository_root_of, state_of_entry, status, FileState, GitError, RepoStatus,
};
use tempfile::TempDir;

fn not_stopped() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn write_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("create folder");
    fs::write(path, content).expect("write file");
}

/// Writes the files to disk and commits exactly them on HEAD, with the index
/// matching the commit.
fn commit_files(
    repository: &gix::Repository,
    root: &Path,
    files: &[(&str, &str)],
    parents: &[ObjectId],
) -> ObjectId {
    let mut editor = repository
        .edit_tree(repository.empty_tree().id)
        .expect("edit tree");
    for (relative, content) in files {
        write_file(root, relative, content);
        let blob = repository.write_blob(content).expect("blob").detach();
        editor
            .upsert(*relative, EntryKind::Blob, blob)
            .expect("upsert");
    }
    let tree = editor.write().expect("write tree").detach();
    let signature = SignatureRef {
        name: BStr::new("Test"),
        email: BStr::new("test@example.com"),
        time: "1700000000 +0000",
    };
    let commit = repository
        .commit_as(
            signature,
            signature,
            "HEAD",
            "commit",
            tree,
            parents.iter().copied(),
        )
        .expect("commit")
        .detach();
    let mut index = repository.index_from_tree(&tree).expect("index");
    index.write(Default::default()).expect("write index");
    commit
}

fn repository_with(files: &[(&str, &str)]) -> (TempDir, gix::Repository, ObjectId) {
    let dir = TempDir::new().expect("tempdir");
    let repository = gix::init(dir.path()).expect("init");
    let commit = commit_files(&repository, dir.path(), files, &[]);
    (dir, repository, commit)
}

fn state_in(dir: &TempDir, relative: &str, is_dir: bool) -> Option<FileState> {
    let current = status(dir.path(), &not_stopped()).expect("status");
    state_of_entry(&current, dir.path(), &dir.path().join(relative), is_dir)
}

#[test]
fn a_folder_without_git_is_no_repository() {
    let dir = TempDir::new().expect("tempdir");
    assert!(!is_repository_root(dir.path()));
    assert_eq!(repository_root_of(dir.path()), None);
    assert!(matches!(
        status(dir.path(), &not_stopped()),
        Err(GitError::Open(_))
    ));
}

#[test]
fn the_root_is_found_from_a_nested_folder() {
    let (dir, _repository, _) = repository_with(&[("sub/a.txt", "a")]);
    assert!(is_repository_root(dir.path()));
    assert_eq!(
        repository_root_of(&dir.path().join("sub")),
        Some(dir.path().to_path_buf())
    );
    assert_eq!(
        repository_root_of(&dir.path().join("sub/a.txt")),
        Some(dir.path().to_path_buf())
    );
}

#[test]
fn a_dot_git_file_marks_a_linked_worktree() {
    let dir = TempDir::new().expect("tempdir");
    fs::write(
        dir.path().join(".git"),
        "gitdir: /elsewhere/.git/worktrees/w\n",
    )
    .expect("git file");
    fs::create_dir(dir.path().join("src")).expect("folder");
    assert!(is_repository_root(dir.path()));
    assert_eq!(
        repository_root_of(&dir.path().join("src")),
        Some(dir.path().to_path_buf())
    );
}

#[test]
fn a_clean_repository_has_a_branch_and_no_entries() {
    let (dir, repository, commit) = repository_with(&[("a.txt", "a")]);
    let current = status(dir.path(), &not_stopped()).expect("status");
    let expected_branch = repository
        .head_name()
        .expect("head")
        .map(|name| name.shorten().to_string());
    assert_eq!(current.branch, expected_branch);
    assert_eq!(
        current.head_short,
        Some(commit.to_string().chars().take(7).collect())
    );
    assert_eq!((current.ahead, current.behind), (0, 0));
    assert!(current.entries.is_empty(), "{:?}", current.entries);
}

#[test]
fn a_repository_before_the_first_commit_has_no_head() {
    let dir = TempDir::new().expect("tempdir");
    gix::init(dir.path()).expect("init");
    write_file(dir.path(), "new.txt", "new");
    let current = status(dir.path(), &not_stopped()).expect("status");
    assert_eq!(current.head_short, None);
    assert_eq!(
        state_of_entry(&current, dir.path(), &dir.path().join("new.txt"), false),
        Some(FileState::Untracked)
    );
}

#[test]
fn a_detached_head_has_no_branch() {
    let (dir, repository, commit) = repository_with(&[("a.txt", "a")]);
    fs::write(repository.git_dir().join("HEAD"), format!("{commit}\n")).expect("detach");
    let current = status(dir.path(), &not_stopped()).expect("status");
    assert_eq!(current.branch, None);
    assert!(current.head_short.is_some());
}

#[test]
fn changed_deleted_and_untracked_files_are_told_apart() {
    let (dir, _repository, _) = repository_with(&[
        ("kept.txt", "kept"),
        ("changed.txt", "old"),
        ("gone.txt", "gone"),
    ]);
    write_file(dir.path(), "changed.txt", "new and longer");
    fs::remove_file(dir.path().join("gone.txt")).expect("remove");
    write_file(dir.path(), "fresh.txt", "fresh");

    let current = status(dir.path(), &not_stopped()).expect("status");
    let state = |name: &str| state_of_entry(&current, dir.path(), &dir.path().join(name), false);
    assert_eq!(state("kept.txt"), None);
    assert_eq!(state("changed.txt"), Some(FileState::Modified));
    assert_eq!(state("gone.txt"), Some(FileState::Deleted));
    assert_eq!(state("fresh.txt"), Some(FileState::Untracked));
}

#[test]
fn gitignore_marks_files_and_folders_as_ignored() {
    let (dir, _repository, _) =
        repository_with(&[(".gitignore", "*.log\nbuild/\n"), ("a.txt", "a")]);
    write_file(dir.path(), "debug.log", "log");
    write_file(dir.path(), "build/out.bin", "bin");

    let current = status(dir.path(), &not_stopped()).expect("status");
    let state =
        |name: &str, is_dir| state_of_entry(&current, dir.path(), &dir.path().join(name), is_dir);
    assert_eq!(state("debug.log", false), Some(FileState::Ignored));
    assert_eq!(state("build", true), Some(FileState::Ignored));
    assert_eq!(state("build/out.bin", false), Some(FileState::Ignored));
    assert_eq!(state("a.txt", false), None);
}

#[test]
fn a_folder_shows_the_strongest_change_inside() {
    let (dir, _repository, _) = repository_with(&[
        (".gitignore", "*.log\n"),
        ("changed/a.txt", "old"),
        ("changed/b.txt", "b"),
        ("untouched/c.txt", "c"),
        ("only_ignored/d.txt", "d"),
    ]);
    write_file(dir.path(), "changed/a.txt", "new and longer");
    write_file(dir.path(), "changed/extra.txt", "extra");
    write_file(dir.path(), "untouched/extra.txt", "extra");
    write_file(dir.path(), "only_ignored/x.log", "log");
    write_file(dir.path(), "brand_new/e.txt", "e");

    assert_eq!(state_in(&dir, "changed", true), Some(FileState::Modified));
    assert_eq!(
        state_in(&dir, "untouched", true),
        Some(FileState::Untracked)
    );
    assert_eq!(state_in(&dir, "only_ignored", true), None);
    assert_eq!(
        state_in(&dir, "brand_new", true),
        Some(FileState::Untracked)
    );
    assert_eq!(
        state_in(&dir, "brand_new/e.txt", false),
        Some(FileState::Untracked)
    );
}

#[test]
fn the_folder_summary_ranks_conflicts_above_changes_above_untracked() {
    let entries = [
        ("a/conflict.txt", FileState::Conflicted),
        ("a/changed.txt", FileState::Modified),
        ("b/changed.txt", FileState::Deleted),
        ("b/new.txt", FileState::Untracked),
        ("c/new.txt", FileState::Untracked),
        ("c/skipped.log", FileState::Ignored),
        ("d/skipped.log", FileState::Ignored),
    ]
    .into_iter()
    .map(|(path, state)| (path.into(), state))
    .collect();
    let current = RepoStatus {
        entries,
        ..RepoStatus::default()
    };
    let root = Path::new("/repo");
    let folder = |name: &str| state_of_entry(&current, root, &root.join(name), true);
    assert_eq!(folder("a"), Some(FileState::Conflicted));
    assert_eq!(folder("b"), Some(FileState::Modified));
    assert_eq!(folder("c"), Some(FileState::Untracked));
    assert_eq!(folder("d"), None);
    assert_eq!(
        state_of_entry(&current, root, Path::new("/elsewhere"), true),
        None
    );
}

#[test]
fn ahead_and_behind_are_counted_against_the_upstream() {
    let (dir, repository, first) = repository_with(&[("a.txt", "a")]);
    let branch = repository
        .head_name()
        .expect("head")
        .expect("branch")
        .shorten()
        .to_string();
    let second = commit_files(
        &repository,
        dir.path(),
        &[("a.txt", "a"), ("b.txt", "b")],
        &[first],
    );

    let git_dir = repository.git_dir().to_path_buf();
    fs::write(
        git_dir.join("config"),
        format!(
            "{}\n[remote \"origin\"]\n\turl = ./nowhere\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n\
             [branch \"{branch}\"]\n\tremote = origin\n\tmerge = refs/heads/{branch}\n",
            fs::read_to_string(git_dir.join("config")).expect("config")
        ),
    )
    .expect("write config");
    let remote_ref = git_dir.join("refs/remotes/origin").join(&branch);
    fs::create_dir_all(remote_ref.parent().expect("parent")).expect("remote folder");
    fs::write(&remote_ref, format!("{second}\n")).expect("remote ref");
    let local_ref = git_dir.join("refs/heads").join(&branch);

    // Local at the first commit, upstream one commit further.
    fs::write(&local_ref, format!("{first}\n")).expect("rewind");
    let behind = status(dir.path(), &not_stopped()).expect("status");
    assert_eq!((behind.ahead, behind.behind), (0, 1));

    // Local has a commit of its own on top of the first one, the upstream another.
    let diverged_repository = gix::open(dir.path()).expect("open");
    commit_files(
        &diverged_repository,
        dir.path(),
        &[("c.txt", "c")],
        &[first],
    );
    let diverged = status(dir.path(), &not_stopped()).expect("status");
    assert_eq!((diverged.ahead, diverged.behind), (1, 1));

    // Local is two commits ahead of the first, upstream still at the first.
    fs::write(&remote_ref, format!("{first}\n")).expect("move upstream");
    let head = diverged_repository.head_id().expect("head").detach();
    commit_files(&diverged_repository, dir.path(), &[("d.txt", "d")], &[head]);
    let ahead = status(dir.path(), &not_stopped()).expect("status");
    assert_eq!((ahead.ahead, ahead.behind), (2, 0));
}

#[test]
fn a_branch_without_upstream_has_no_distance() {
    let (dir, _repository, _) = repository_with(&[("a.txt", "a")]);
    let current = status(dir.path(), &not_stopped()).expect("status");
    assert_eq!((current.ahead, current.behind), (0, 0));
}

#[test]
fn a_raised_stop_flag_ends_the_status() {
    let (dir, _repository, _) = repository_with(&[("a.txt", "a")]);
    let stop = not_stopped();
    stop.store(true, Ordering::Relaxed);
    assert_eq!(status(dir.path(), &stop), Err(GitError::Interrupted));
}
