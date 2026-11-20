use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use gix::diff::tree_with_rewrites::Change;

use crate::diff::{blob_at, line_counts};
use crate::repo_access::{open, parse_id, short_hash};
use crate::status::check_stop;
use crate::{status, FileState, GitError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    /// Where a renamed file was before.
    pub old_path: Option<String>,
    pub kind: ChangeKind,
    pub added: u32,
    pub removed: u32,
    /// No line counts: binary or too large.
    pub binary: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitDetail {
    pub id: String,
    pub short: String,
    pub parents: Vec<String>,
    pub author: String,
    pub time: i64,
    /// Title, empty line and body as written.
    pub message: String,
    pub files: Vec<ChangedFile>,
}

/// The message and the files a commit changed against its first parent.
pub fn commit_detail(
    root: &Path,
    sha: &str,
    stop: &Arc<AtomicBool>,
) -> Result<CommitDetail, GitError> {
    check_stop(stop)?;
    let repository = open(root)?;
    let id = parse_id(sha)?;
    let commit = repository.find_commit(id).map_err(GitError::read)?;
    let tree = commit.tree().map_err(GitError::read)?;
    let parents: Vec<gix::ObjectId> = commit.parent_ids().map(|parent| parent.detach()).collect();
    let parent_tree = match parents.first() {
        Some(parent) => repository
            .find_commit(*parent)
            .and_then(|parent| parent.tree())
            .map_err(GitError::read)?,
        None => repository.empty_tree(),
    };
    let changes = repository
        .diff_tree_to_tree(&parent_tree, &tree, None)
        .map_err(GitError::read)?;

    let mut files = Vec::new();
    for change in &changes {
        check_stop(stop)?;
        let Some((path, old_path, kind)) = describe(change) else {
            continue;
        };
        let old = blob_at(
            &repository,
            &parent_tree,
            old_path.as_deref().unwrap_or(&path),
        );
        let new = blob_at(&repository, &tree, &path);
        let counts = line_counts(&old, &new);
        files.push(ChangedFile {
            path,
            old_path,
            kind,
            added: counts.map_or(0, |counts| counts.0),
            removed: counts.map_or(0, |counts| counts.1),
            binary: counts.is_none(),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(CommitDetail {
        short: short_hash(&id),
        id: id.to_string(),
        parents: parents.iter().map(ToString::to_string).collect(),
        author: commit
            .author()
            .map(|signature| signature.name.to_string())
            .unwrap_or_default(),
        time: commit.time().map(|time| time.seconds).unwrap_or_default(),
        message: commit
            .message_raw()
            .map(|message| message.to_string().trim_end().to_string())
            .unwrap_or_default(),
        files,
    })
}

/// Path, previous path and kind of a changed file; `None` for folders and submodules.
fn describe(change: &Change) -> Option<(String, Option<String>, ChangeKind)> {
    let (mode, path, old_path, kind) = match change {
        Change::Addition {
            location,
            entry_mode,
            ..
        } => (*entry_mode, location.to_string(), None, ChangeKind::Added),
        Change::Deletion {
            location,
            entry_mode,
            ..
        } => (*entry_mode, location.to_string(), None, ChangeKind::Deleted),
        Change::Modification {
            location,
            entry_mode,
            ..
        } => (
            *entry_mode,
            location.to_string(),
            None,
            ChangeKind::Modified,
        ),
        Change::Rewrite {
            source_location,
            location,
            entry_mode,
            ..
        } => (
            *entry_mode,
            location.to_string(),
            Some(source_location.to_string()),
            ChangeKind::Renamed,
        ),
    };
    (!mode.is_tree() && !mode.is_commit()).then_some((path, old_path, kind))
}

/// The files changed in the working copy against HEAD, for the "changes" entry of a worktree.
pub fn working_changes(root: &Path, stop: &Arc<AtomicBool>) -> Result<Vec<ChangedFile>, GitError> {
    let current = status(root, stop)?;
    let repository = open(root)?;
    let head_tree = match repository.head_commit() {
        Ok(commit) => commit.tree().map_err(GitError::read)?,
        Err(_) => repository.empty_tree(),
    };
    let mut files = Vec::new();
    for (relative, state) in &current.entries {
        check_stop(stop)?;
        let kind = match state {
            FileState::Ignored => continue,
            FileState::Added | FileState::Untracked => ChangeKind::Added,
            FileState::Deleted => ChangeKind::Deleted,
            FileState::Modified | FileState::Renamed | FileState::Conflicted => {
                ChangeKind::Modified
            }
        };
        for path in files_under(root, relative) {
            let old = blob_at(&repository, &head_tree, &path);
            let new = std::fs::read(root.join(&path)).unwrap_or_default();
            let counts = line_counts(&old, &new);
            files.push(ChangedFile {
                path,
                old_path: None,
                kind,
                added: counts.map_or(0, |counts| counts.0),
                removed: counts.map_or(0, |counts| counts.1),
                binary: counts.is_none(),
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The path itself, or every file below it when an untracked folder stands as one entry.
fn files_under(root: &Path, relative: &Path) -> Vec<String> {
    let full = root.join(relative);
    if !full.is_dir() {
        return vec![slash_path(relative)];
    }
    let mut found = Vec::new();
    let mut pending = vec![full];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if let Ok(inner) = path.strip_prefix(root) {
                found.push(slash_path(inner));
            }
        }
    }
    found
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
