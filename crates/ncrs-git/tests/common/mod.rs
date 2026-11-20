//! Fixture repositories built with gix itself, so no git binary is needed.

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use gix::actor::SignatureRef;
use gix::bstr::BStr;
use gix::object::tree::EntryKind;
use gix::ObjectId;
use tempfile::TempDir;

pub fn not_stopped() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

pub fn write_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("create folder");
    fs::write(path, content).expect("write file");
}

/// Writes the files to disk and commits exactly them on HEAD, with the index
/// matching the commit. `tick` orders the commits in time.
pub fn commit_files(
    repository: &gix::Repository,
    root: &Path,
    files: &[(&str, &str)],
    parents: &[ObjectId],
    tick: u32,
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
    let time = format!("{} +0000", 1_700_000_000 + tick);
    let signature = SignatureRef {
        name: BStr::new("Test"),
        email: BStr::new("test@example.com"),
        time: &time,
    };
    let commit = repository
        .commit_as(
            signature,
            signature,
            "HEAD",
            format!("commit {tick}"),
            tree,
            parents.iter().copied(),
        )
        .expect("commit")
        .detach();
    let mut index = repository.index_from_tree(&tree).expect("index");
    index.write(Default::default()).expect("write index");
    commit
}

pub fn repository_with(files: &[(&str, &str)]) -> (TempDir, gix::Repository, ObjectId) {
    let dir = TempDir::new().expect("tempdir");
    let repository = gix::init(dir.path()).expect("init");
    // The default branch name comes from the user's git config; the tests need a known one.
    point_head_at(&repository, "main");
    let commit = commit_files(&repository, dir.path(), files, &[], 1);
    (dir, repository, commit)
}

pub fn current_branch(repository: &gix::Repository) -> String {
    repository
        .head_name()
        .expect("head")
        .expect("branch")
        .shorten()
        .to_string()
}

/// Points HEAD at a branch that may not exist yet, or at an existing one.
pub fn point_head_at(repository: &gix::Repository, branch: &str) {
    fs::write(
        repository.git_dir().join("HEAD"),
        format!("ref: refs/heads/{branch}\n"),
    )
    .expect("write HEAD");
}

pub fn write_branch(repository: &gix::Repository, branch: &str, commit: ObjectId) {
    let path = repository.git_dir().join("refs/heads").join(branch);
    fs::create_dir_all(path.parent().expect("parent")).expect("branch folder");
    fs::write(path, format!("{commit}\n")).expect("write branch");
}

/// Configures `origin` for the branch and puts its remote-tracking ref at `commit`.
pub fn track_upstream(repository: &gix::Repository, branch: &str, commit: ObjectId) {
    let git_dir = repository.git_dir().to_path_buf();
    let config = fs::read_to_string(git_dir.join("config")).expect("config");
    fs::write(
        git_dir.join("config"),
        format!(
            "{config}\n[remote \"origin\"]\n\turl = ./nowhere\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n\
             [branch \"{branch}\"]\n\tremote = origin\n\tmerge = refs/heads/{branch}\n"
        ),
    )
    .expect("write config");
    let remote_ref = git_dir.join("refs/remotes/origin").join(branch);
    fs::create_dir_all(remote_ref.parent().expect("parent")).expect("remote folder");
    fs::write(remote_ref, format!("{commit}\n")).expect("remote ref");
}
