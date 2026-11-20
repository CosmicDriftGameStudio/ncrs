use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gix::bstr::BString;
use gix::dir::entry::Status as DirStatus;
use gix::dir::walk::EmissionMode;
use gix::status::index_worktree::{self, iter::Summary};
use gix::status::{Item, UntrackedFiles};

use crate::repo_access::{open, short_hash};
use crate::{FileState, GitError};

/// Branch, distance to the upstream and the changed files of one repository.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoStatus {
    /// `None` when HEAD is detached.
    pub branch: Option<String>,
    /// The first characters of the commit HEAD points to; `None` before the first commit.
    pub head_short: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Changed, untracked and ignored paths relative to the repository root.
    /// An untracked or ignored folder is one entry; the files in it have none.
    pub entries: HashMap<PathBuf, FileState>,
}

/// Reads the state of the repository at `root`, the folder that contains `.git`.
/// Blocks, so call it from the blocking pool. Raising `stop` ends it early with
/// [`GitError::Interrupted`].
pub fn status(root: &Path, stop: &Arc<AtomicBool>) -> Result<RepoStatus, GitError> {
    check_stop(stop)?;
    let repository = open(root)?;

    let head = repository.head().map_err(GitError::read)?;
    let branch = head.referent_name().map(|name| name.shorten().to_string());
    let head_id = head.id().map(|id| id.detach());
    let head_short = head_id.map(|id| short_hash(&id));
    let (ahead, behind) = match (head.referent_name(), head_id) {
        (Some(name), Some(id)) => distance_to_upstream(&repository, name, id, stop)?,
        _ => (0, 0),
    };

    let entries = file_states(&repository, stop)?;
    Ok(RepoStatus {
        branch,
        head_short,
        ahead,
        behind,
        entries,
    })
}

/// The state to show for a panel row. `entry_path` must lie under `root`.
/// A folder shows its most important change inside; a file inside an untracked
/// or ignored folder shows the state of that folder.
pub fn state_of_entry(
    status: &RepoStatus,
    root: &Path,
    entry_path: &Path,
    is_dir: bool,
) -> Option<FileState> {
    let relative = entry_path.strip_prefix(root).ok()?;
    if let Some(state) = status.entries.get(relative) {
        return Some(*state);
    }
    let inherited = relative
        .ancestors()
        .skip(1)
        .take_while(|ancestor| !ancestor.as_os_str().is_empty())
        .find_map(|ancestor| match status.entries.get(ancestor) {
            Some(state @ (FileState::Untracked | FileState::Ignored)) => Some(*state),
            _ => None,
        });
    if inherited.is_some() || !is_dir {
        return inherited;
    }
    status
        .entries
        .iter()
        .filter(|(path, _)| path.starts_with(relative))
        .filter_map(|(_, state)| state.as_folder_state())
        .reduce(FileState::strongest)
}

pub fn check_stop(stop: &AtomicBool) -> Result<(), GitError> {
    if stop.load(Ordering::Relaxed) {
        Err(GitError::Interrupted)
    } else {
        Ok(())
    }
}

pub fn distance_to_upstream(
    repository: &gix::Repository,
    branch: &gix::refs::FullNameRef,
    head_id: gix::ObjectId,
    stop: &AtomicBool,
) -> Result<(u32, u32), GitError> {
    let Some(Ok(tracking_name)) =
        repository.branch_remote_tracking_ref_name(branch, gix::remote::Direction::Fetch)
    else {
        return Ok((0, 0));
    };
    let Ok(tracking) = repository.find_reference(&tracking_name) else {
        return Ok((0, 0));
    };
    let Ok(peeled) = tracking.into_fully_peeled_id() else {
        return Ok((0, 0));
    };
    let upstream_id = peeled.detach();
    let ahead = count_commits(repository, head_id, upstream_id, stop)?;
    let behind = count_commits(repository, upstream_id, head_id, stop)?;
    Ok((ahead, behind))
}

/// Commits reachable from `tip` but not from `hidden`. A history that cannot be
/// walked to the end (a shallow clone) counts as no distance rather than failing
/// the whole status.
pub fn count_commits(
    repository: &gix::Repository,
    tip: gix::ObjectId,
    hidden: gix::ObjectId,
    stop: &AtomicBool,
) -> Result<u32, GitError> {
    let Ok(walk) = repository.rev_walk([tip]).with_hidden([hidden]).all() else {
        return Ok(0);
    };
    let mut count: u32 = 0;
    for commit in walk {
        check_stop(stop)?;
        if commit.is_err() {
            return Ok(0);
        }
        count = count.saturating_add(1);
    }
    Ok(count)
}

fn file_states(
    repository: &gix::Repository,
    stop: &Arc<AtomicBool>,
) -> Result<HashMap<PathBuf, FileState>, GitError> {
    let items = repository
        .status(gix::progress::Discard)
        .map_err(GitError::read)?
        .untracked_files(UntrackedFiles::Collapsed)
        .dirwalk_options(|options| options.emit_ignored(Some(EmissionMode::CollapseDirectory)))
        .should_interrupt_owned(Arc::clone(stop))
        .into_iter(std::iter::empty::<BString>())
        .map_err(GitError::read)?;

    let mut entries: HashMap<PathBuf, FileState> = HashMap::new();
    for next in items {
        check_stop(stop)?;
        let item = next.map_err(GitError::read)?;
        let Some(state) = state_of_item(&item) else {
            continue;
        };
        let path = gix::path::from_bstr(item.location())
            .map_err(GitError::read)?
            .into_owned();
        entries
            .entry(path)
            .and_modify(|known| *known = known.strongest(state))
            .or_insert(state);
    }
    // The iterator ends quietly when interrupted, so a cut-off list must not pass as complete.
    check_stop(stop)?;
    Ok(entries)
}

fn state_of_item(item: &Item) -> Option<FileState> {
    match item {
        Item::IndexWorktree(change) => match change {
            index_worktree::Item::DirectoryContents { entry, .. } => match &entry.status {
                DirStatus::Untracked => Some(FileState::Untracked),
                DirStatus::Ignored(_) => Some(FileState::Ignored),
                DirStatus::Tracked | DirStatus::Pruned => None,
            },
            index_worktree::Item::Modification { .. } | index_worktree::Item::Rewrite { .. } => {
                change.summary().map(state_of_summary)
            }
        },
        Item::TreeIndex(change) => Some(match change {
            gix::diff::index::Change::Addition { .. } => FileState::Added,
            gix::diff::index::Change::Deletion { .. } => FileState::Deleted,
            gix::diff::index::Change::Modification { .. } => FileState::Modified,
            gix::diff::index::Change::Rewrite { .. } => FileState::Renamed,
        }),
    }
}

fn state_of_summary(summary: Summary) -> FileState {
    match summary {
        Summary::Removed => FileState::Deleted,
        Summary::Added | Summary::IntentToAdd | Summary::Copied => FileState::Added,
        Summary::Modified | Summary::TypeChange => FileState::Modified,
        Summary::Renamed => FileState::Renamed,
        Summary::Conflict => FileState::Conflicted,
    }
}
