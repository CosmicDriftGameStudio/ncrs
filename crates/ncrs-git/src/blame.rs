use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use gix::bstr::{BStr, ByteSlice as _};

use crate::repo_access::{open, parse_id, path_as_in_tree, short_hash};
use crate::status::check_stop;
use crate::GitError;

/// Larger files are not blamed: it would walk the history for every line.
const MAX_BLAMED_BYTES: u64 = 4 * 1024 * 1024;

/// A run of consecutive lines that one commit introduced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameGroup {
    /// The first line, counted from 1.
    pub first_line: u32,
    pub len: u32,
    pub sha: String,
    pub short: String,
    pub author: String,
    /// Seconds since the Unix epoch.
    pub time: i64,
    pub summary: String,
    /// The file's name in that commit, when it was another one.
    pub path_then: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameLine {
    /// Counted from 1.
    pub line_no: u32,
    pub text: String,
    /// Index into `Blame::groups`.
    pub group: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BlameLimit {
    #[default]
    None,
    /// Contains NUL bytes (binary, or a large-file pointer): no lines to blame.
    Binary,
    TooLarge,
    /// A symlink or a submodule.
    NotAFile,
    /// The commit does not have the file: new or ignored in the working copy.
    NotCommitted,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Blame {
    pub lines: Vec<BlameLine>,
    pub groups: Vec<BlameGroup>,
    /// Why there are no lines for a file that exists.
    pub refused: BlameLimit,
}

/// Who last changed each line of `path` as committed at `at` (HEAD when `None`).
/// Changes in the working copy are not part of it. Renames are followed.
pub fn blame(
    root: &Path,
    path: &str,
    at: Option<&str>,
    stop: &Arc<AtomicBool>,
) -> Result<Blame, GitError> {
    check_stop(stop)?;
    let repository = open(root)?;
    let suspect = match at {
        Some(sha) => parse_id(sha)?,
        None => match repository.head().map_err(GitError::read)?.id() {
            Some(head) => head.detach(),
            None => {
                return Ok(Blame {
                    refused: BlameLimit::NotCommitted,
                    ..Blame::default()
                })
            }
        },
    };
    let tree = repository
        .find_commit(suspect)
        .and_then(|commit| commit.tree())
        .map_err(GitError::read)?;
    let path = path_as_in_tree(&repository, &tree, path);
    let refused = refusal(&repository, &tree, &path)?;
    if refused != BlameLimit::None {
        return Ok(Blame {
            refused,
            ..Blame::default()
        });
    }
    check_stop(stop)?;
    let outcome = repository
        .blame_file(
            BStr::new(path.as_str()),
            suspect,
            gix::repository::blame_file::Options {
                rewrites: Some(gix::diff::Rewrites::default()),
                ..Default::default()
            },
        )
        .map_err(GitError::read)?;
    check_stop(stop)?;
    let texts: Vec<String> = outcome
        .blob
        .lines_with_terminator()
        .map(|line| {
            line.to_str_lossy()
                .trim_end_matches(['\n', '\r'])
                .to_string()
        })
        .collect();
    let mut result = Blame::default();
    for entry in &outcome.entries {
        check_stop(stop)?;
        let commit = repository
            .find_commit(entry.commit_id)
            .map_err(GitError::read)?;
        let message = commit
            .message_raw()
            .map(ToString::to_string)
            .unwrap_or_default();
        let group = result.groups.len();
        result.groups.push(BlameGroup {
            first_line: entry.start_in_blamed_file + 1,
            len: entry.len.get(),
            sha: entry.commit_id.to_string(),
            short: short_hash(&entry.commit_id),
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
            path_then: entry.source_file_name.as_ref().map(|name| name.to_string()),
        });
        for offset in 0..entry.len.get() {
            let index = entry.start_in_blamed_file + offset;
            result.lines.push(BlameLine {
                line_no: index + 1,
                text: texts.get(index as usize).cloned().unwrap_or_default(),
                group,
            });
        }
    }
    Ok(result)
}

/// Why the file is not blamed, known before the history is walked for it.
fn refusal(
    repository: &gix::Repository,
    tree: &gix::Tree<'_>,
    path: &str,
) -> Result<BlameLimit, GitError> {
    let Some(entry) = tree.lookup_entry_by_path(path).map_err(GitError::read)? else {
        return Ok(BlameLimit::NotCommitted);
    };
    if !entry.mode().is_blob() {
        return Ok(BlameLimit::NotAFile);
    }
    let size = repository
        .find_header(entry.id())
        .map_err(GitError::read)?
        .size();
    if size > MAX_BLAMED_BYTES {
        return Ok(BlameLimit::TooLarge);
    }
    let blob = repository.find_blob(entry.id()).map_err(GitError::read)?;
    Ok(if blob.data.contains(&0) {
        BlameLimit::Binary
    } else {
        BlameLimit::None
    })
}
