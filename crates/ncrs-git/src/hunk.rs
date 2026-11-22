use std::fs;

use crate::changes::discard_record;
use crate::cli::Git;
use crate::diff::{diff_bytes, DiffHunk, FileDiff, RowKind};
use crate::undo::{UndoRecord, UndoSummary};
use crate::ActionError;

/// Which two versions of a file a hunk list compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkSide {
    /// Index against working copy: what staging or discarding a hunk acts on.
    Unstaged,
    /// HEAD against index: what unstaging a hunk acts on.
    Staged,
}

const NO_NEWLINE_MARKER: &[u8] = b"\\ No newline at end of file\n";

impl Git {
    /// The hunks of one file for the given side. Hunk actions take a hunk from
    /// exactly this list; one that no longer fits the file is refused by git.
    pub fn hunks(&self, path: &str, side: HunkSide) -> Result<FileDiff, ActionError> {
        let (old, new) = self.sides(path, side)?;
        Ok(diff_bytes(path, &old, &new, false))
    }

    pub fn stage_hunk(
        &self,
        path: &str,
        hunk: &DiffHunk,
    ) -> Result<Option<UndoRecord>, ActionError> {
        self.recording_index(
            UndoSummary::Stage { files: 1 },
            format!("Stage a change in {path}"),
            || {
                let (old, new) = self.sides(path, HunkSide::Unstaged)?;
                let patch = patch_of(path, hunk, &old, &new);
                self.ok_with_input(
                    &["apply", "--cached", "--whitespace=nowarn", "-"],
                    Some(&patch),
                )
                .map(|_| ())
            },
        )
    }

    pub fn unstage_hunk(
        &self,
        path: &str,
        hunk: &DiffHunk,
    ) -> Result<Option<UndoRecord>, ActionError> {
        self.recording_index(
            UndoSummary::Unstage { files: 1 },
            format!("Unstage a change in {path}"),
            || {
                let (old, new) = self.sides(path, HunkSide::Staged)?;
                let patch = patch_of(path, hunk, &old, &new);
                self.ok_with_input(
                    &["apply", "--cached", "-R", "--whitespace=nowarn", "-"],
                    Some(&patch),
                )
                .map(|_| ())
            },
        )
    }

    /// Throws the hunk away in the working copy; the whole file is saved first.
    pub fn discard_hunk(&self, path: &str, hunk: &DiffHunk) -> Result<UndoRecord, ActionError> {
        let (old, new) = self.sides(path, HunkSide::Unstaged)?;
        let patch = patch_of(path, hunk, &old, &new);
        let mut saved = self.save_files(&[path.to_string()])?;
        self.ok_with_input(&["apply", "-R", "--whitespace=nowarn", "-"], Some(&patch))?;
        self.mark_discarded(&mut saved);
        let mut record = discard_record(saved);
        record.description = format!("Discard a change in {path}");
        Ok(record)
    }

    fn sides(&self, path: &str, side: HunkSide) -> Result<(Vec<u8>, Vec<u8>), ActionError> {
        let in_index = self.blob(&format!(":{path}"));
        let (old, new) = match side {
            HunkSide::Unstaged => (in_index, fs::read(self.root().join(path)).ok()),
            HunkSide::Staged => (self.blob(&format!("HEAD:{path}")), in_index),
        };
        match (old, new) {
            (Some(before), Some(after)) => {
                if before.contains(&0) || after.contains(&0) {
                    return Err(ActionError::Other {
                        git_output: format!("{path} is binary: use the whole-file action"),
                    });
                }
                Ok((before, after))
            }
            _ => Err(ActionError::Other {
                git_output: format!("{path} is new or deleted: use the whole-file action"),
            }),
        }
    }

    pub(crate) fn blob(&self, spec: &str) -> Option<Vec<u8>> {
        self.run(&["cat-file", "blob", spec], None)
            .ok()
            .filter(|output| output.success)
            .map(|output| output.stdout)
    }
}

/// A unified diff of the one hunk, with the lines taken verbatim from the files
/// (line endings and a missing final newline included), so `git apply` matches.
fn patch_of(path: &str, hunk: &DiffHunk, old: &[u8], new: &[u8]) -> Vec<u8> {
    let old_lines: Vec<&[u8]> = old.split_inclusive(|byte| *byte == b'\n').collect();
    let new_lines: Vec<&[u8]> = new.split_inclusive(|byte| *byte == b'\n').collect();
    let mut body: Vec<u8> = Vec::new();
    let (mut old_count, mut new_count): (u32, u32) = (0, 0);
    for row in &hunk.rows {
        let (marker, source, number) = match row.kind {
            RowKind::Context => (b' ', &old_lines, row.old_line),
            RowKind::Removed => (b'-', &old_lines, row.old_line),
            RowKind::Added => (b'+', &new_lines, row.new_line),
        };
        match row.kind {
            RowKind::Context => {
                old_count += 1;
                new_count += 1;
            }
            RowKind::Removed => old_count += 1,
            RowKind::Added => new_count += 1,
        }
        let line = number
            .and_then(|line_number| source.get(line_number as usize - 1))
            .copied()
            .unwrap_or_default();
        body.push(marker);
        body.extend_from_slice(line);
        if !line.ends_with(b"\n") {
            body.push(b'\n');
            body.extend_from_slice(NO_NEWLINE_MARKER);
        }
    }
    // An empty side is addressed as the line before the (non-existent) first one.
    let old_start = if old_count == 0 {
        hunk.old_start.saturating_sub(1)
    } else {
        hunk.old_start
    };
    let new_start = if new_count == 0 {
        hunk.new_start.saturating_sub(1)
    } else {
        hunk.new_start
    };
    let mut patch = format!(
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
    )
    .into_bytes();
    patch.extend_from_slice(&body);
    patch
}
