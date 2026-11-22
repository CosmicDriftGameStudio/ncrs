use std::fs;

use crate::cli::Git;
use crate::detail::{ChangeKind, ChangedFile};
use crate::diff::line_counts;
use crate::undo::{SavedFile, UndoAction, UndoRecord, UndoSummary};
use crate::ActionError;

/// One line of `git status --porcelain`, reduced to what the actions need.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PorcelainEntry {
    pub index: char,
    pub worktree: char,
    pub path: String,
}

impl PorcelainEntry {
    pub fn is_untracked(&self) -> bool {
        self.index == '?' && self.worktree == '?'
    }

    pub fn is_unmerged(&self) -> bool {
        matches!(
            (self.index, self.worktree),
            ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D')
        )
    }

    pub fn has_unstaged_change(&self) -> bool {
        !self.is_unmerged() && !self.is_untracked() && self.worktree != ' '
    }
}

/// The changed files of a working copy, split the way the index splits them.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ChangeLists {
    /// In the working copy only, untracked files included.
    pub unstaged: Vec<ChangedFile>,
    /// In the index, against HEAD.
    pub staged: Vec<ChangedFile>,
}

fn kind_of_letter(letter: char) -> ChangeKind {
    match letter {
        'A' | 'C' | '?' => ChangeKind::Added,
        'D' => ChangeKind::Deleted,
        'R' => ChangeKind::Renamed,
        _ => ChangeKind::Modified,
    }
}

fn changed(path: &str, kind: ChangeKind, old: &[u8], new: &[u8]) -> ChangedFile {
    let counts = line_counts(old, new);
    ChangedFile {
        path: path.to_string(),
        old_path: None,
        kind,
        added: counts.map_or(0, |(added, _)| added),
        removed: counts.map_or(0, |(_, removed)| removed),
        binary: counts.is_none(),
    }
}

impl Git {
    /// The files not staged and the files staged, each with its line counts.
    /// Files in conflict are in neither: see `conflicts`.
    pub fn changes(&self) -> Result<ChangeLists, ActionError> {
        let mut lists = ChangeLists::default();
        for entry in self.porcelain()? {
            if entry.is_unmerged() {
                continue;
            }
            let path = entry.path.as_str();
            if entry.index != ' ' && !entry.is_untracked() {
                let old = self.blob(&format!("HEAD:{path}")).unwrap_or_default();
                let new = self.blob(&format!(":{path}")).unwrap_or_default();
                lists
                    .staged
                    .push(changed(path, kind_of_letter(entry.index), &old, &new));
            }
            if entry.is_untracked() || entry.has_unstaged_change() {
                let old = self.blob(&format!(":{path}")).unwrap_or_default();
                let new = fs::read(self.root().join(path)).unwrap_or_default();
                lists
                    .unstaged
                    .push(changed(path, kind_of_letter(entry.worktree), &old, &new));
            }
        }
        lists.staged.sort_by(|a, b| a.path.cmp(&b.path));
        lists.unstaged.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(lists)
    }

    pub(crate) fn porcelain(&self) -> Result<Vec<PorcelainEntry>, ActionError> {
        let output = self.ok(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let mut fields = text.split('\0');
        let mut entries = Vec::new();
        while let Some(field) = fields.next() {
            let mut chars = field.chars();
            let (Some(index), Some(worktree), Some(' ')) =
                (chars.next(), chars.next(), chars.next())
            else {
                continue;
            };
            if matches!(index, 'R' | 'C') || matches!(worktree, 'R' | 'C') {
                fields.next();
            }
            entries.push(PorcelainEntry {
                index,
                worktree,
                path: chars.as_str().to_string(),
            });
        }
        Ok(entries)
    }

    /// Stages the files (new, changed or deleted).
    pub fn stage(&self, paths: &[&str]) -> Result<Option<UndoRecord>, ActionError> {
        self.recording_index(
            UndoSummary::Stage { files: paths.len() },
            format!("Stage {} file(s)", paths.len()),
            || {
                let mut args = vec!["add", "--all", "--"];
                args.extend_from_slice(paths);
                self.ok(&args).map(|_| ())
            },
        )
    }

    pub fn stage_all(&self) -> Result<Option<UndoRecord>, ActionError> {
        self.recording_index(
            UndoSummary::Stage { files: 0 },
            "Stage all changes".to_string(),
            || self.ok(&["add", "--all"]).map(|_| ()),
        )
    }

    pub fn unstage(&self, paths: &[&str]) -> Result<Option<UndoRecord>, ActionError> {
        self.recording_index(
            UndoSummary::Unstage { files: paths.len() },
            format!("Unstage {} file(s)", paths.len()),
            || {
                let mut args = vec!["reset", "-q", "--"];
                args.extend_from_slice(paths);
                self.ok(&args).map(|_| ())
            },
        )
    }

    pub fn unstage_all(&self) -> Result<Option<UndoRecord>, ActionError> {
        self.recording_index(
            UndoSummary::Unstage { files: 0 },
            "Unstage all changes".to_string(),
            || self.ok(&["reset", "-q"]).map(|_| ()),
        )
    }

    /// Throws away the working-copy changes of one file (staged changes stay);
    /// an untracked file is deleted. What is lost is saved first.
    pub fn discard_file(&self, path: &str) -> Result<UndoRecord, ActionError> {
        let entries = self.porcelain()?;
        let entry = entries.iter().find(|entry| entry.path == path);
        let untracked = entry.is_some_and(PorcelainEntry::is_untracked);
        self.discard_paths(&[(path.to_string(), untracked)])
    }

    /// Discards every working-copy change and deletes the untracked files.
    pub fn discard_all(&self) -> Result<UndoRecord, ActionError> {
        let targets: Vec<(String, bool)> = self
            .porcelain()?
            .into_iter()
            .filter(|entry| entry.is_untracked() || entry.has_unstaged_change())
            .map(|entry| {
                let untracked = entry.is_untracked();
                (entry.path, untracked)
            })
            .collect();
        self.discard_paths(&targets)
    }

    fn discard_paths(&self, targets: &[(String, bool)]) -> Result<UndoRecord, ActionError> {
        let paths: Vec<String> = targets.iter().map(|(path, _)| path.clone()).collect();
        let mut saved = self.save_files(&paths)?;
        for (path, untracked) in targets {
            if *untracked {
                self.remove_untracked(path)?;
            } else {
                self.ok(&["checkout", "--", path])?;
            }
        }
        self.mark_discarded(&mut saved);
        Ok(discard_record(saved))
    }

    fn remove_untracked(&self, path: &str) -> Result<(), ActionError> {
        let target = self.root().join(path);
        fs::remove_file(&target).map_err(|cause| ActionError::Io(cause.to_string()))?;
        let mut folder = target.parent();
        while let Some(parent) = folder {
            if parent == self.root() || fs::remove_dir(parent).is_err() {
                break;
            }
            folder = parent.parent();
        }
        Ok(())
    }
}

pub fn discard_record(saved: Vec<SavedFile>) -> UndoRecord {
    let files = saved.len();
    UndoRecord {
        summary: UndoSummary::Discard { files },
        description: format!("Discard {files} file(s)"),
        action: UndoAction::RestoreFiles { saved },
    }
}
