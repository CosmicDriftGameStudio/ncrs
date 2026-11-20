//! How the lines of a diff are laid out in rows, for the two ways to show it.

use ncrs_git::{DiffHunk, DiffRow, FileDiff, RowKind};

/// One row of the side by side view; an empty side has no line there.
#[derive(Debug, Clone, Copy)]
pub struct Pair<'a> {
    pub left: Option<&'a DiffRow>,
    pub right: Option<&'a DiffRow>,
}

fn flush_changes<'a>(
    out: &mut Vec<Pair<'a>>,
    removed: &mut Vec<&'a DiffRow>,
    added: &mut Vec<&'a DiffRow>,
) {
    for index in 0..removed.len().max(added.len()) {
        out.push(Pair {
            left: removed.get(index).copied(),
            right: added.get(index).copied(),
        });
    }
    removed.clear();
    added.clear();
}

/// Removed and added lines of one change next to each other, context on both sides.
pub fn pairs(hunk: &DiffHunk) -> Vec<Pair<'_>> {
    let mut out = Vec::new();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for row in &hunk.rows {
        match row.kind {
            RowKind::Removed => removed.push(row),
            RowKind::Added => added.push(row),
            RowKind::Context => {
                flush_changes(&mut out, &mut removed, &mut added);
                out.push(Pair {
                    left: Some(row),
                    right: Some(row),
                });
            }
        }
    }
    flush_changes(&mut out, &mut removed, &mut added);
    out
}

/// How many rows each hunk takes, its header line included.
pub fn hunk_rows(diff: &FileDiff, unified: bool) -> Vec<usize> {
    diff.hunks
        .iter()
        .map(|hunk| {
            1 + if unified {
                hunk.rows.len()
            } else {
                pairs(hunk).len()
            }
        })
        .collect()
}

/// The rows above the start of hunk `index`.
pub fn rows_before(diff: &FileDiff, unified: bool, index: usize) -> usize {
    hunk_rows(diff, unified).iter().take(index).sum()
}
