use std::fs;

use crate::changes::PorcelainEntry;
use crate::cli::{Git, Output};
use crate::conflict::{parse_blocks, resolve_blocks, ConflictBlock, ConflictChoice};
use crate::refname::plain_ref;
use crate::ActionError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Integration {
    Merge,
    Rebase,
}

/// The two sides of a merge or rebase that stopped, as branch names. While
/// rebasing, git's "ours" is `other` (what the commits are replayed onto) and
/// "theirs" is `current` (the commits being replayed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrationDetail {
    pub kind: Integration,
    /// The branch the user is on.
    pub current: String,
    /// The branch merged in, or the one the rebase goes onto.
    pub other: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntegrateOutcome {
    Completed,
    /// Stopped in the middle; resolve these files, then `continue_integration`.
    Conflicts(Vec<ConflictedFile>),
}

/// What each side did to a path git could not combine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    BothModified,
    BothAdded,
    BothDeleted,
    DeletedByUs,
    DeletedByThem,
    AddedByUs,
    AddedByThem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictedFile {
    pub path: String,
    pub kind: ConflictKind,
    /// Empty for binary files and for conflicts where a side deleted the file.
    pub blocks: Vec<ConflictBlock>,
}

fn kind_of(entry: &PorcelainEntry) -> ConflictKind {
    match (entry.index, entry.worktree) {
        ('A', 'A') => ConflictKind::BothAdded,
        ('D', 'D') => ConflictKind::BothDeleted,
        ('D', 'U') => ConflictKind::DeletedByUs,
        ('U', 'D') => ConflictKind::DeletedByThem,
        ('A', 'U') => ConflictKind::AddedByUs,
        ('U', 'A') => ConflictKind::AddedByThem,
        _ => ConflictKind::BothModified,
    }
}

/// Whether our side / their side still has the file.
fn sides_with_file(kind: ConflictKind) -> (bool, bool) {
    match kind {
        ConflictKind::BothModified | ConflictKind::BothAdded => (true, true),
        ConflictKind::BothDeleted => (false, false),
        ConflictKind::DeletedByUs | ConflictKind::AddedByThem => (false, true),
        ConflictKind::DeletedByThem | ConflictKind::AddedByUs => (true, false),
    }
}

fn io_error(cause: std::io::Error) -> ActionError {
    ActionError::Io(cause.to_string())
}

fn other(text: String) -> ActionError {
    ActionError::Other { git_output: text }
}

impl Git {
    pub fn merge(&self, reference: &str) -> Result<IntegrateOutcome, ActionError> {
        let output = self.run(&["merge", "--no-edit", "--", plain_ref(reference)?], None)?;
        self.finished_integration(&output)
    }

    pub fn rebase(&self, reference: &str) -> Result<IntegrateOutcome, ActionError> {
        let output = self.run(&["rebase", "--", plain_ref(reference)?], None)?;
        self.finished_integration(&output)
    }

    /// A merge or rebase that stopped and waits for the conflicts to be resolved.
    pub fn integration_in_progress(&self) -> Option<Integration> {
        let git_dir = self.git_dir().ok()?;
        if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
            Some(Integration::Rebase)
        } else if git_dir.join("MERGE_HEAD").exists() {
            Some(Integration::Merge)
        } else {
            None
        }
    }

    /// Which branches a merge or rebase in progress joins, as names to show.
    pub fn integration_detail(&self) -> Option<IntegrationDetail> {
        let kind = self.integration_in_progress()?;
        let git_dir = self.git_dir().ok()?;
        let read = |relative: &str| {
            fs::read_to_string(git_dir.join(relative))
                .ok()
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
        };
        let (current, other) = match kind {
            Integration::Merge => {
                let message = read("MERGE_MSG").unwrap_or_default();
                let other = message
                    .lines()
                    .next()
                    .and_then(|line| line.split('\'').nth(1))
                    .map(str::to_string)
                    .or_else(|| self.name_of(&read("MERGE_HEAD")?))
                    .unwrap_or_default();
                (self.current_branch().unwrap_or_default(), other)
            }
            Integration::Rebase => {
                let folder = if git_dir.join("rebase-merge").exists() {
                    "rebase-merge"
                } else {
                    "rebase-apply"
                };
                let current = read(&format!("{folder}/head-name"))
                    .map(|name| name.trim_start_matches("refs/heads/").to_string())
                    .unwrap_or_default();
                let other = read(&format!("{folder}/onto"))
                    .and_then(|sha| self.name_of(&sha))
                    .unwrap_or_default();
                (current, other)
            }
        };
        Some(IntegrationDetail {
            kind,
            current,
            other,
        })
    }

    fn name_of(&self, sha: &str) -> Option<String> {
        let name = self.try_text(&["name-rev", "--name-only", "--no-undefined", "--", sha])?;
        let short = name.split(['~', '^']).next().unwrap_or(&name);
        Some(short.trim_start_matches("remotes/").to_string())
    }

    pub fn conflicts(&self) -> Result<Vec<ConflictedFile>, ActionError> {
        Ok(self
            .porcelain()?
            .iter()
            .filter(|entry| entry.is_unmerged())
            .map(|entry| {
                let blocks = fs::read(self.root().join(&entry.path))
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .map(|text| parse_blocks(&text))
                    .unwrap_or_default();
                ConflictedFile {
                    path: entry.path.clone(),
                    kind: kind_of(entry),
                    blocks,
                }
            })
            .collect())
    }

    /// Resolves block `block` (its position in `ConflictedFile::blocks`; the
    /// numbers close up after each resolution) by rewriting the file.
    pub fn resolve_block(
        &self,
        path: &str,
        block: usize,
        choice: ConflictChoice,
    ) -> Result<(), ActionError> {
        if !self
            .conflicts()?
            .iter()
            .any(|conflict| conflict.path == path)
        {
            return Err(other(format!("{path} is not in conflict")));
        }
        self.rewrite_conflicts(path, Some(block), choice)
    }

    /// Resolves the whole file: every block its way, or the whole-file version
    /// of one side for binary files and delete conflicts.
    pub fn resolve_file(&self, path: &str, choice: ConflictChoice) -> Result<(), ActionError> {
        let conflict = self
            .conflicts()?
            .into_iter()
            .find(|conflict| conflict.path == path)
            .ok_or_else(|| other(format!("{path} is not in conflict")))?;
        if !conflict.blocks.is_empty() {
            return self.rewrite_conflicts(path, None, choice);
        }
        let (ours, theirs) = sides_with_file(conflict.kind);
        if matches!(
            choice,
            ConflictChoice::Both | ConflictChoice::TheirsThenOurs
        ) && ours
            && theirs
        {
            return Err(other(format!("{path} has no text blocks to combine")));
        }
        let (keep_ours, keep_theirs) = match choice {
            ConflictChoice::Ours => (ours, false),
            ConflictChoice::Theirs => (false, theirs),
            ConflictChoice::Both | ConflictChoice::TheirsThenOurs => (ours, !ours && theirs),
        };
        if keep_ours {
            self.ok(&["checkout", "--ours", "--", path])?;
            self.ok(&["add", "--", path])?;
        } else if keep_theirs {
            self.ok(&["checkout", "--theirs", "--", path])?;
            self.ok(&["add", "--", path])?;
        } else {
            self.ok(&["rm", "--quiet", "--force", "--", path])?;
        }
        Ok(())
    }

    /// Marks the file resolved. Refused while it still has conflict blocks.
    pub fn mark_resolved(&self, path: &str) -> Result<(), ActionError> {
        let remaining = self
            .conflicts()?
            .into_iter()
            .find(|conflict| conflict.path == path)
            .map_or(0, |conflict| conflict.blocks.len());
        if remaining > 0 {
            return Err(ActionError::UnresolvedConflicts { count: remaining });
        }
        self.ok(&["add", "--", path])?;
        Ok(())
    }

    /// Brings the conflict markers back and throws the resolution away.
    pub fn reset_file(&self, path: &str) -> Result<(), ActionError> {
        self.ok(&["checkout", "-m", "--", path])?;
        Ok(())
    }

    pub fn abort_integration(&self) -> Result<(), ActionError> {
        match self.integration_in_progress() {
            Some(Integration::Rebase) => self.ok(&["rebase", "--abort"])?,
            Some(Integration::Merge) => self.ok(&["merge", "--abort"])?,
            None => return Err(nothing_in_progress()),
        };
        Ok(())
    }

    /// Finishes a merge with its prepared message, or goes on with the rebase
    /// (which may stop again at the next commit).
    pub fn continue_integration(&self) -> Result<IntegrateOutcome, ActionError> {
        let open = self.conflicts()?.len();
        if open > 0 {
            return Err(ActionError::UnresolvedConflicts { count: open });
        }
        match self.integration_in_progress() {
            Some(Integration::Rebase) => {
                let output = self.run(&["rebase", "--continue"], None)?;
                self.finished_integration(&output)
            }
            Some(Integration::Merge) => {
                self.ok(&["commit", "--no-edit"])?;
                Ok(IntegrateOutcome::Completed)
            }
            None => Err(nothing_in_progress()),
        }
    }

    pub(crate) fn finished_integration(
        &self,
        output: &Output,
    ) -> Result<IntegrateOutcome, ActionError> {
        if output.success {
            return Ok(IntegrateOutcome::Completed);
        }
        self.failed_integration(output, "")
    }

    /// A failed merge/rebase/pull/stash-apply: stopped on conflicts, or a real error.
    pub(crate) fn failed_integration(
        &self,
        output: &Output,
        remote: &str,
    ) -> Result<IntegrateOutcome, ActionError> {
        let conflicts = self.conflicts()?;
        if !conflicts.is_empty() {
            return Ok(IntegrateOutcome::Conflicts(conflicts));
        }
        Err(if remote.is_empty() {
            other(output.combined())
        } else {
            crate::action_error::classify_sync_failure(&output.combined(), remote)
        })
    }

    fn rewrite_conflicts(
        &self,
        path: &str,
        block: Option<usize>,
        choice: ConflictChoice,
    ) -> Result<(), ActionError> {
        crate::undo::refuse_symlinks(self.root(), path)?;
        let file = self.root().join(path);
        let bytes = fs::read(&file).map_err(io_error)?;
        let text = String::from_utf8(bytes)
            .map_err(|cause| other(format!("{path} is not text: {cause}")))?;
        let resolved = resolve_blocks(&text, block, choice)
            .ok_or_else(|| other(format!("{path} has no such conflict block")))?;
        fs::write(&file, resolved).map_err(io_error)
    }
}

fn nothing_in_progress() -> ActionError {
    other("no merge or rebase is in progress".to_string())
}
