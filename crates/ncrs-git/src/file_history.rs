use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use gix::diff::tree_with_rewrites::Change;
use gix::traverse::commit::simple::CommitTimeOrder;

use crate::graph::GraphRow;
use crate::log::LogCommit;
use crate::refs::labels_by_commit;
use crate::repo_access::{open, path_as_in_tree, short_hash};
use crate::status::check_stop;
use crate::GitError;

/// A commit that changed the file, and where the file was at that commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHistoryEntry {
    pub commit: LogCommit,
    pub path_at_commit: String,
    /// Where the file came from when this commit moved it.
    pub old_path: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileHistory {
    pub commits: Vec<FileHistoryEntry>,
    /// More commits follow; ask again with a larger limit.
    pub has_more: bool,
}

/// The first `limit` commits from HEAD that changed `path`, newest first,
/// following the file across renames. A tree is looked at per commit, so ask
/// for pages and honour `stop`.
pub fn file_history(
    root: &Path,
    path: &str,
    limit: usize,
    stop: &Arc<AtomicBool>,
) -> Result<FileHistory, GitError> {
    check_stop(stop)?;
    let repository = open(root)?;
    let mut result = FileHistory::default();
    let Some(head) = repository.head().map_err(GitError::read)?.id() else {
        return Ok(result);
    };
    let labels = labels_by_commit(&repository);
    let walk = repository
        .rev_walk([head.detach()])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            CommitTimeOrder::NewestFirst,
        ))
        .all()
        .map_err(GitError::read)?;
    let head_tree = repository
        .find_commit(head)
        .and_then(|commit| commit.tree())
        .map_err(GitError::read)?;
    let mut current = path_as_in_tree(&repository, &head_tree, path);
    for walked in walk {
        check_stop(stop)?;
        let info = walked.map_err(GitError::read)?;
        let commit = info.object().map_err(GitError::read)?;
        let tree = commit.tree().map_err(GitError::read)?;
        let parent_tree = match info.parent_ids.first() {
            Some(parent) => repository
                .find_commit(*parent)
                .and_then(|found| found.tree())
                .map_err(GitError::read)?,
            None => repository.empty_tree(),
        };
        let here = entry_id(&tree, &current);
        let before = entry_id(&parent_tree, &current);
        if here == before {
            continue;
        }
        let old_path = match (here, before) {
            (Some(_), None) => moved_from(&repository, &parent_tree, &tree, &current),
            _ => None,
        };
        if result.commits.len() >= limit {
            result.has_more = true;
            break;
        }
        let message = commit
            .message_raw()
            .map(ToString::to_string)
            .unwrap_or_default();
        let parents: Vec<String> = info.parent_ids.iter().map(ToString::to_string).collect();
        result.commits.push(FileHistoryEntry {
            commit: LogCommit {
                short: short_hash(&info.id),
                id: info.id.to_string(),
                is_merge: parents.len() > 1,
                parents,
                author: commit
                    .author()
                    .map(|signature| signature.name.to_string())
                    .unwrap_or_default(),
                time: commit.time().map(|time| time.seconds).unwrap_or_default(),
                summary: message
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                refs: labels.get(&info.id).cloned().unwrap_or_default(),
                graph: GraphRow::default(),
            },
            path_at_commit: current.clone(),
            old_path: old_path.clone(),
        });
        if let Some(previous) = old_path {
            current = previous;
        }
    }
    Ok(result)
}

fn entry_id(tree: &gix::Tree<'_>, path: &str) -> Option<gix::ObjectId> {
    let entry = tree.lookup_entry_by_path(path).ok().flatten()?;
    (!entry.mode().is_tree()).then(|| entry.id().detach())
}

/// The old path when this commit renamed another file to `path`.
fn moved_from(
    repository: &gix::Repository,
    parent_tree: &gix::Tree<'_>,
    tree: &gix::Tree<'_>,
    path: &str,
) -> Option<String> {
    let changes = repository.diff_tree_to_tree(parent_tree, tree, None).ok()?;
    changes.iter().find_map(|change| match change {
        Change::Rewrite {
            source_location,
            location,
            ..
        } if location.as_slice() == path.as_bytes() => Some(source_location.to_string()),
        _ => None,
    })
}
