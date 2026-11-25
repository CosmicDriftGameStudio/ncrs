use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::detail::{changed_files, working_changes, ChangeKind, ChangedFile};
use crate::diff::{blob_at, diff_bytes, line_counts, FileDiff};
use crate::repo_access::{open, parse_id};
use crate::status::check_stop;
use crate::working_file::read_working_file;
use crate::GitError;

fn tree_of<'r>(repository: &'r gix::Repository, sha: &str) -> Result<gix::Tree<'r>, GitError> {
    repository
        .find_commit(parse_id(sha)?)
        .and_then(|commit| commit.tree())
        .map_err(GitError::read)
}

/// The files that differ between the commit `from` and the commit `to`, or
/// the working copy when `to` is `None`. Against the working copy only exactly
/// equal content counts as a rename; between commits git's similarity applies.
pub fn compare_files(
    root: &Path,
    from: &str,
    to: Option<&str>,
    stop: &Arc<AtomicBool>,
) -> Result<Vec<ChangedFile>, GitError> {
    check_stop(stop)?;
    let repository = open(root)?;
    let from_tree = tree_of(&repository, from)?;
    if let Some(sha) = to {
        let to_tree = tree_of(&repository, sha)?;
        let mut files = changed_files(&repository, &from_tree, &to_tree, stop)?;
        files.sort_by(|a, b| a.path.cmp(&b.path));
        return Ok(files);
    }
    let head_tree = match repository.head_commit() {
        Ok(commit) => commit.tree().map_err(GitError::read)?,
        Err(_) => repository.empty_tree(),
    };
    let mut paths: BTreeSet<String> = BTreeSet::new();
    for file in changed_files(&repository, &from_tree, &head_tree, stop)?
        .into_iter()
        .chain(working_changes(root, stop)?)
    {
        paths.extend(file.old_path);
        paths.insert(file.path);
    }
    let mut files = Vec::new();
    for path in paths {
        check_stop(stop)?;
        let old = blob_at(&repository, &from_tree, &path);
        let in_old = from_tree
            .lookup_entry_by_path(&path)
            .ok()
            .flatten()
            .is_some();
        let new = read_working_file(root, &path);
        let kind = match (in_old, &new) {
            (false, None) => continue,
            (false, Some(_)) => ChangeKind::Added,
            (true, None) => ChangeKind::Deleted,
            (true, Some(bytes)) if *bytes == old => continue,
            (true, Some(_)) => ChangeKind::Modified,
        };
        let new_bytes = new.unwrap_or_default();
        let counts = line_counts(&old, &new_bytes);
        files.push(ChangedFile {
            path,
            old_path: None,
            kind,
            added: counts.map_or(0, |(added, _)| added),
            removed: counts.map_or(0, |(_, removed)| removed),
            binary: counts.is_none(),
            untracked: false,
        });
    }
    pair_renames(&repository, &from_tree, root, &mut files);
    Ok(files)
}

/// An added file whose bytes equal a file of `from` that is gone from the
/// working copy is that file, renamed. Only looked up when something was added.
fn pair_renames(
    repository: &gix::Repository,
    from_tree: &gix::Tree<'_>,
    root: &Path,
    files: &mut Vec<ChangedFile>,
) {
    if !files.iter().any(|file| file.kind == ChangeKind::Added) {
        return;
    }
    let mut recorder = gix::traverse::tree::Recorder::default();
    if from_tree.traverse().breadthfirst(&mut recorder).is_err() {
        return;
    }
    let mut taken: Vec<String> = Vec::new();
    for file in files
        .iter_mut()
        .filter(|file| file.kind == ChangeKind::Added)
    {
        let Some(content) = read_working_file(root, &file.path) else {
            continue;
        };
        let Ok(id) =
            gix::objs::compute_hash(repository.object_hash(), gix::object::Kind::Blob, &content)
        else {
            continue;
        };
        let source = recorder
            .records
            .iter()
            .filter(|record| record.mode.is_blob() && record.oid == id)
            .map(|record| record.filepath.to_string())
            .find(|path| !root.join(path).exists() && !taken.contains(path));
        if let Some(path) = source {
            taken.push(path.clone());
            file.kind = ChangeKind::Renamed;
            file.old_path = Some(path);
            file.added = 0;
            file.removed = 0;
        }
    }
    files.retain(|file| file.kind != ChangeKind::Deleted || !taken.contains(&file.path));
}

/// The text diff of one file between the commit `from` and `to` (or the working copy).
pub fn compare_diff(
    root: &Path,
    from: &str,
    to: Option<&str>,
    path: &str,
    old_path: Option<&str>,
    ignore_whitespace: bool,
    stop: &Arc<AtomicBool>,
) -> Result<FileDiff, GitError> {
    check_stop(stop)?;
    let repository = open(root)?;
    let from_tree = tree_of(&repository, from)?;
    let old = blob_at(&repository, &from_tree, old_path.unwrap_or(path));
    let new = match to {
        Some(sha) => blob_at(&repository, &tree_of(&repository, sha)?, path),
        None => read_working_file(root, path).unwrap_or_default(),
    };
    check_stop(stop)?;
    Ok(diff_bytes(path, &old, &new, ignore_whitespace))
}
