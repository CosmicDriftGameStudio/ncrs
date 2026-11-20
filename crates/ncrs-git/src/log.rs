use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use gix::traverse::commit::simple::CommitTimeOrder;
use gix::ObjectId;

use crate::graph::{GraphRow, LaneLayout};
use crate::refs::{labels_by_commit, RefLabel};
use crate::repo_access::{open, short_hash};
use crate::status::check_stop;
use crate::GitError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogCommit {
    pub id: String,
    pub short: String,
    pub parents: Vec<String>,
    pub author: String,
    /// Seconds since the Unix epoch.
    pub time: i64,
    pub summary: String,
    pub refs: Vec<RefLabel>,
    pub is_merge: bool,
    /// Empty (lane 0, no lines) while a filter is set: a graph over a subset lies.
    pub graph: GraphRow,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Log {
    pub commits: Vec<LogCommit>,
    /// More commits follow; ask again with a larger limit.
    pub has_more: bool,
    pub lane_count: usize,
    pub head: Option<String>,
}

/// The first `limit` commits of all branches and tags, newest first, that match
/// `filter` (message, author or the start of the SHA; empty matches all).
/// Growing `limit` yields the next page and keeps the first one unchanged.
pub fn log(
    root: &Path,
    limit: usize,
    filter: &str,
    stop: &Arc<AtomicBool>,
) -> Result<Log, GitError> {
    check_stop(stop)?;
    let repository = open(root)?;
    let labels = labels_by_commit(&repository);
    let head = repository
        .head()
        .map_err(GitError::read)?
        .id()
        .map(|id| id.detach());
    let mut seen = HashSet::new();
    let tips: Vec<ObjectId> = labels
        .keys()
        .copied()
        .chain(head)
        .filter(|id| seen.insert(*id))
        .collect();
    let mut result = Log {
        head: head.map(|id| id.to_string()),
        ..Log::default()
    };
    if tips.is_empty() {
        return Ok(result);
    }

    let needle = filter.trim().to_lowercase();
    let filtered = !needle.is_empty();
    let walk = repository
        .rev_walk(tips)
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            CommitTimeOrder::NewestFirst,
        ))
        .all()
        .map_err(GitError::read)?;
    let mut layout = LaneLayout::default();

    for info in walk {
        check_stop(stop)?;
        let info = info.map_err(GitError::read)?;
        let parents: Vec<ObjectId> = info.parent_ids.iter().copied().collect();
        let graph = layout.place(info.id, &parents);
        let commit = info.object().map_err(GitError::read)?;
        let message = commit
            .message_raw()
            .map(ToString::to_string)
            .unwrap_or_default();
        let author = commit
            .author()
            .map(|signature| signature.name.to_string())
            .unwrap_or_default();
        let id = info.id.to_string();
        if filtered
            && !(message.to_lowercase().contains(&needle)
                || author.to_lowercase().contains(&needle)
                || id.starts_with(&needle))
        {
            continue;
        }
        if result.commits.len() >= limit {
            result.has_more = true;
            break;
        }
        result.commits.push(LogCommit {
            short: short_hash(&info.id),
            id,
            is_merge: parents.len() > 1,
            parents: parents.iter().map(ToString::to_string).collect(),
            author,
            time: commit.time().map(|time| time.seconds).unwrap_or_default(),
            summary: message
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .to_string(),
            refs: labels.get(&info.id).cloned().unwrap_or_default(),
            graph: if filtered { GraphRow::default() } else { graph },
        });
    }
    result.lane_count = if filtered {
        1
    } else {
        layout.lane_count.max(1)
    };
    Ok(result)
}
