use std::ops::Range;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use gix::diff::blob::{sources, Algorithm, Diff, InternedInput};

use crate::repo_access::{open, parse_id};
use crate::status::check_stop;
use crate::GitError;

const CONTEXT_LINES: u32 = 3;
const SECTION_CHARS: usize = 80;
/// Larger files are not diffed line by line.
const MAX_DIFFED_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Context,
    Removed,
    Added,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRow {
    pub kind: RowKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub text: String,
}

/// A stretch of changes with up to three lines of context around it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffHunk {
    pub old_start: u32,
    pub new_start: u32,
    pub rows: Vec<DiffRow>,
    /// The line above the hunk that opens a function or section, as git
    /// shows it after the `@@`; empty if there is none.
    pub context: String,
}

impl DiffHunk {
    /// How many lines the hunk covers in the old file.
    pub fn old_len(&self) -> usize {
        self.rows.iter().filter(|row| row.old_line.is_some()).count()
    }

    /// How many lines the hunk covers in the new file.
    pub fn new_len(&self) -> usize {
        self.rows.iter().filter(|row| row.new_line.is_some()).count()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub hunks: Vec<DiffHunk>,
    pub added: u32,
    pub removed: u32,
    /// Contains NUL bytes: no text to show.
    pub binary: bool,
    /// Too large to compare line by line.
    pub too_large: bool,
    /// Lines of the old and the new file; `None` where there is no text to count.
    pub old_total: Option<u32>,
    pub new_total: Option<u32>,
}

/// Which file of which commit, or of the working copy when `commit` is `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffTarget {
    pub commit: Option<String>,
    pub path: String,
    /// Where the file was before a rename.
    pub old_path: Option<String>,
}

/// The text diff of one file against its parent (or the working copy against HEAD).
pub fn file_diff(
    root: &Path,
    target: &DiffTarget,
    ignore_whitespace: bool,
    stop: &Arc<AtomicBool>,
) -> Result<FileDiff, GitError> {
    check_stop(stop)?;
    let repository = open(root)?;
    let old_path = target.old_path.as_deref().unwrap_or(&target.path);
    let (old, new) = match &target.commit {
        Some(sha) => {
            let commit = repository
                .find_commit(parse_id(sha)?)
                .map_err(GitError::read)?;
            let parent_tree = match commit.parent_ids().next() {
                Some(parent) => repository
                    .find_commit(parent.detach())
                    .and_then(|found| found.tree())
                    .map_err(GitError::read)?,
                None => repository.empty_tree(),
            };
            let tree = commit.tree().map_err(GitError::read)?;
            (
                blob_at(&repository, &parent_tree, old_path),
                blob_at(&repository, &tree, &target.path),
            )
        }
        None => {
            let head_tree = match repository.head_commit() {
                Ok(commit) => commit.tree().map_err(GitError::read)?,
                Err(_) => repository.empty_tree(),
            };
            (
                blob_at(&repository, &head_tree, old_path),
                std::fs::read(root.join(&target.path)).unwrap_or_default(),
            )
        }
    };
    check_stop(stop)?;
    Ok(diff_bytes(&target.path, &old, &new, ignore_whitespace))
}

pub fn blob_at(repository: &gix::Repository, tree: &gix::Tree<'_>, path: &str) -> Vec<u8> {
    tree.lookup_entry_by_path(path)
        .ok()
        .flatten()
        .and_then(|entry| repository.find_blob(entry.id().detach()).ok())
        .map(|blob| blob.data.clone())
        .unwrap_or_default()
}

/// Added and removed line counts, or `None` for a binary or oversized file.
pub fn line_counts(old: &[u8], new: &[u8]) -> Option<(u32, u32)> {
    let diff = diff_bytes("", old, new, false);
    (!diff.binary && !diff.too_large).then_some((diff.added, diff.removed))
}

pub fn diff_bytes(path: &str, old: &[u8], new: &[u8], ignore_whitespace: bool) -> FileDiff {
    let mut result = FileDiff {
        path: path.to_string(),
        ..FileDiff::default()
    };
    if old.contains(&0) || new.contains(&0) {
        result.binary = true;
        return result;
    }
    if old.len() > MAX_DIFFED_BYTES || new.len() > MAX_DIFFED_BYTES {
        result.too_large = true;
        return result;
    }
    let old_text = String::from_utf8_lossy(old);
    let new_text = String::from_utf8_lossy(new);
    let old_lines: Vec<&str> = old_text.lines().collect();
    let new_lines: Vec<&str> = new_text.lines().collect();
    result.old_total = u32::try_from(old_lines.len()).ok();
    result.new_total = u32::try_from(new_lines.len()).ok();

    let before = comparable(&old_lines, ignore_whitespace);
    let after = comparable(&new_lines, ignore_whitespace);
    let input = InternedInput::new(sources::lines(&before), sources::lines(&after));
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    result.added = diff.count_additions();
    result.removed = diff.count_removals();
    let changes: Vec<(Range<u32>, Range<u32>)> = diff
        .hunks()
        .map(|hunk| (hunk.before.clone(), hunk.after.clone()))
        .collect();
    result.hunks = hunks_with_context(&changes, &old_lines, &new_lines);
    result
}

/// One token per line; whitespace differences vanish when they are to be ignored.
fn comparable(lines: &[&str], ignore_whitespace: bool) -> String {
    let mut text = String::new();
    for line in lines {
        if ignore_whitespace {
            text.push_str(&line.split_whitespace().collect::<Vec<_>>().join(" "));
        } else {
            text.push_str(line);
        }
        text.push('\n');
    }
    text
}

fn hunks_with_context(
    changes: &[(Range<u32>, Range<u32>)],
    old_lines: &[&str],
    new_lines: &[&str],
) -> Vec<DiffHunk> {
    let mut groups: Vec<Vec<&(Range<u32>, Range<u32>)>> = Vec::new();
    for change in changes {
        match groups.last_mut() {
            Some(group)
                if group.last().is_some_and(|previous| {
                    change.0.start.saturating_sub(previous.0.end) <= 2 * CONTEXT_LINES
                }) =>
            {
                group.push(change);
            }
            _ => groups.push(vec![change]),
        }
    }
    groups
        .iter()
        .filter_map(|group| hunk_of(group, old_lines, new_lines))
        .collect()
}

fn hunk_of(
    group: &[&(Range<u32>, Range<u32>)],
    old_lines: &[&str],
    new_lines: &[&str],
) -> Option<DiffHunk> {
    let first = group.first()?;
    let last = group.last()?;
    let lead = CONTEXT_LINES.min(first.0.start).min(first.1.start);
    let mut rows = Vec::new();
    let context = |old_from: u32, new_from: u32, count: u32, out: &mut Vec<DiffRow>| {
        for offset in 0..count {
            let old_index = old_from + offset;
            out.push(DiffRow {
                kind: RowKind::Context,
                old_line: Some(old_index + 1),
                new_line: Some(new_from + offset + 1),
                text: line_at(old_lines, old_index),
            });
        }
    };
    context(first.0.start - lead, first.1.start - lead, lead, &mut rows);
    let mut previous_end: Option<(u32, u32)> = None;
    for (before, after) in group.iter().map(|change| (&change.0, &change.1)) {
        if let Some((old_end, new_end)) = previous_end {
            context(old_end, new_end, before.start - old_end, &mut rows);
        }
        for index in before.clone() {
            rows.push(DiffRow {
                kind: RowKind::Removed,
                old_line: Some(index + 1),
                new_line: None,
                text: line_at(old_lines, index),
            });
        }
        for index in after.clone() {
            rows.push(DiffRow {
                kind: RowKind::Added,
                old_line: None,
                new_line: Some(index + 1),
                text: line_at(new_lines, index),
            });
        }
        previous_end = Some((before.end, after.end));
    }
    let trail = CONTEXT_LINES
        .min(
            u32::try_from(old_lines.len())
                .unwrap_or(u32::MAX)
                .saturating_sub(last.0.end),
        )
        .min(
            u32::try_from(new_lines.len())
                .unwrap_or(u32::MAX)
                .saturating_sub(last.1.end),
        );
    context(last.0.end, last.1.end, trail, &mut rows);
    let first_old = first.0.start - lead;
    Some(DiffHunk {
        old_start: first_old + 1,
        new_start: first.1.start - lead + 1,
        rows,
        context: section_above(old_lines, first_old),
    })
}

/// The nearest line above `index` that starts at the left edge with a letter or
/// `_`, like git's default for function lines, cut to a readable length.
fn section_above(lines: &[&str], index: u32) -> String {
    let above = lines.get(..index as usize).unwrap_or_default();
    above
        .iter()
        .rev()
        .find(|line| {
            line.chars()
                .next()
                .is_some_and(|first| first.is_alphabetic() || first == '_' || first == '$')
        })
        .map(|line| line.trim_end().chars().take(SECTION_CHARS).collect())
        .unwrap_or_default()
}

fn line_at(lines: &[&str], index: u32) -> String {
    lines
        .get(index as usize)
        .map(ToString::to_string)
        .unwrap_or_default()
}
