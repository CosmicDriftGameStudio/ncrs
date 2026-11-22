use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cli::Git;
use crate::ActionError;

const KEPT_BACKUP_FOLDERS: usize = 10;
/// The width of the zero-padded nanosecond stamp that names a backup folder.
const BACKUP_NAME_DIGITS: usize = 30;

/// What the toolbar names for the undo; the view turns it into its own text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UndoSummary {
    Stage { files: usize },
    Unstage { files: usize },
    Discard { files: usize },
    Commit { title: String },
    DeleteBranch { name: String },
}

/// The last local action and how to take it back. Held by the caller (in memory
/// only); `Git::undo` checks that nothing newer would be lost before it acts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoRecord {
    pub summary: UndoSummary,
    /// English text for a tooltip when the view has no translation for the summary.
    pub description: String,
    pub action: UndoAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UndoAction {
    /// The index as a tree before and after the action.
    Index {
        before: String,
        after: String,
    },
    ResetCommit {
        new_head: String,
        previous_head: String,
    },
    RestoreFiles {
        saved: Vec<SavedFile>,
    },
    RecreateBranch {
        name: String,
        sha: String,
    },
}

/// A file as the discard found it, kept under `.git/ncrs-undo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedFile {
    pub path: String,
    /// The copy of the content; `None` when the file did not exist (a deleted file).
    pub backup: Option<PathBuf>,
    /// Hash of the working copy right after the discard, `None` when it is gone.
    pub after_hash: Option<u64>,
}

fn hash_of_file(path: &Path) -> Option<u64> {
    let bytes = fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
}

fn io_error(cause: std::io::Error) -> ActionError {
    ActionError::Io(cause.to_string())
}

impl Git {
    /// The index as a tree, `None` while there are conflicts (no tree can be written).
    pub(crate) fn index_tree(&self) -> Option<String> {
        self.try_text(&["write-tree"])
            .filter(|tree| !tree.is_empty())
    }

    /// Runs `change` and records how to take back what it did to the index.
    pub(crate) fn recording_index(
        &self,
        summary: UndoSummary,
        description: String,
        change: impl FnOnce() -> Result<(), ActionError>,
    ) -> Result<Option<UndoRecord>, ActionError> {
        let before = self.index_tree();
        change()?;
        let after = self.index_tree();
        Ok(before
            .zip(after)
            .filter(|(old_tree, new_tree)| old_tree != new_tree)
            .map(|(old_tree, new_tree)| UndoRecord {
                summary,
                description,
                action: UndoAction::Index {
                    before: old_tree,
                    after: new_tree,
                },
            }))
    }

    /// Copies the working files that a discard is about to lose.
    pub(crate) fn save_files(&self, paths: &[String]) -> Result<Vec<SavedFile>, ActionError> {
        let folder = self.new_backup_folder()?;
        let mut saved = Vec::new();
        for (number, path) in paths.iter().enumerate() {
            let source = self.root().join(path);
            let backup = if source.is_file() {
                let target = folder.join(number.to_string());
                fs::copy(&source, &target).map_err(io_error)?;
                Some(target)
            } else {
                None
            };
            saved.push(SavedFile {
                path: path.clone(),
                backup,
                after_hash: None,
            });
        }
        Ok(saved)
    }

    pub(crate) fn mark_discarded(&self, saved: &mut [SavedFile]) {
        for file in saved {
            file.after_hash = hash_of_file(&self.root().join(&file.path));
        }
    }

    fn new_backup_folder(&self) -> Result<PathBuf, ActionError> {
        let parent = self.git_dir()?.join("ncrs-undo");
        match fs::symlink_metadata(&parent) {
            Ok(found) if found.is_dir() => {}
            Ok(_) => {
                return Err(ActionError::Io(
                    "the undo folder in .git is not a plain folder".to_string(),
                ))
            }
            Err(_) => fs::create_dir_all(&parent).map_err(io_error)?,
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let folder = parent.join(format!("{nanos:030}"));
        fs::create_dir_all(&folder).map_err(io_error)?;
        prune_backups(&parent);
        Ok(folder)
    }

    /// Takes the last local action back, or says why that is not safe.
    pub fn undo(&self, record: &UndoRecord) -> Result<(), ActionError> {
        match &record.action {
            UndoAction::Index { before, after } => {
                if self.index_tree().as_deref() != Some(after.as_str()) {
                    return Err(ActionError::UndoNotSafe(
                        "the index changed since".to_string(),
                    ));
                }
                self.ok(&["read-tree", before])?;
                Ok(())
            }
            UndoAction::ResetCommit {
                new_head,
                previous_head,
            } => {
                if self.head_sha().as_deref() != Some(new_head.as_str()) {
                    return Err(ActionError::UndoNotSafe(
                        "HEAD moved since the commit".to_string(),
                    ));
                }
                if !self
                    .text(&["branch", "-r", "--contains", new_head])?
                    .is_empty()
                {
                    return Err(ActionError::UndoNotSafe(
                        "the commit is already pushed".to_string(),
                    ));
                }
                self.ok(&["reset", "--soft", previous_head])?;
                Ok(())
            }
            UndoAction::RestoreFiles { saved } => self.restore_files(saved),
            UndoAction::RecreateBranch { name, sha } => {
                if self
                    .try_text(&["rev-parse", "--verify", "-q", &format!("refs/heads/{name}")])
                    .is_some()
                {
                    return Err(ActionError::UndoNotSafe(format!(
                        "branch {name} exists again"
                    )));
                }
                self.ok(&["branch", name, sha])?;
                Ok(())
            }
        }
    }

    fn restore_files(&self, saved: &[SavedFile]) -> Result<(), ActionError> {
        for file in saved {
            refuse_symlinks(self.root(), &file.path)?;
            if hash_of_file(&self.root().join(&file.path)) != file.after_hash {
                return Err(ActionError::UndoNotSafe(format!(
                    "{} changed since the discard",
                    file.path
                )));
            }
        }
        for file in saved {
            let target = self.root().join(&file.path);
            match &file.backup {
                Some(backup) => {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent).map_err(io_error)?;
                    }
                    fs::copy(backup, &target).map_err(io_error)?;
                }
                None => {
                    if target.is_file() {
                        fs::remove_file(&target).map_err(io_error)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Refuses a path that is, or runs through, a symlink inside `root`: a write
/// or a read there would leave the working copy.
pub fn refuse_symlinks(root: &Path, relative: &str) -> Result<(), ActionError> {
    let mut current = root.to_path_buf();
    for part in Path::new(relative).components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(found) if found.file_type().is_symlink() => {
                return Err(ActionError::Io(format!(
                    "{relative} is or runs through a symlink"
                )))
            }
            Ok(_) => {}
            Err(_) => return Ok(()),
        }
    }
    Ok(())
}

fn is_backup_folder_name(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|text| {
        text.len() == BACKUP_NAME_DIGITS && text.bytes().all(|b| b.is_ascii_digit())
    })
}

fn prune_backups(parent: &Path) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_dir())
                && is_backup_folder_name(&entry.file_name())
        })
        .map(|entry| entry.path())
        .collect();
    folders.sort();
    let surplus = folders.len().saturating_sub(KEPT_BACKUP_FOLDERS);
    for old in folders.into_iter().take(surplus) {
        let _ = fs::remove_dir_all(old);
    }
}
