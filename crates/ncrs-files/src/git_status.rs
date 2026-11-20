//! What git says about the rows of a panel: read off the UI thread when the
//! folder changes, shown in the Git column and in the status bar.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use iced::Task;
use ncrs_core::PanelSide;
use ncrs_git::{FileState, RepoStatus};
use ncrs_ui_kit::git_mark::GitMark;

use crate::message::PanelMessage;
use crate::panel::PanelState;
use crate::state::Panels;

/// A reload of the folder that stays in it waits this long, so a burst of
/// changes on disk is read once.
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(300);

/// What the status bar says about the repository of the active panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSummary {
    /// `None` when HEAD is detached.
    pub branch: Option<String>,
    pub head_short: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Paths git reports as changed, untracked or conflicted; ignored ones do
    /// not count.
    pub changed: usize,
}

/// One row of the listing a scan looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRow {
    pub name: OsString,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// The answer of one scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitScan {
    pub summary: Option<GitSummary>,
    /// By entry name.
    pub marks: HashMap<OsString, GitMark>,
}

/// The git state of one panel.
#[derive(Debug, Clone, Default)]
pub struct PanelGit {
    /// The folder `root`, `summary` and `marks` are about.
    pub folder: PathBuf,
    /// The repository the folder is in; the Git column shows while it is set.
    pub root: Option<PathBuf>,
    pub summary: Option<GitSummary>,
    pub marks: HashMap<OsString, GitMark>,
    /// Id of the latest scan; older answers are dropped.
    pub request_id: u64,
    pub stop: Arc<AtomicBool>,
}

impl PanelGit {
    pub fn in_repository(&self) -> bool {
        self.root.is_some()
    }

    pub fn mark_of(&self, name: &OsString) -> Option<GitMark> {
        self.marks.get(name).copied()
    }
}

fn mark_of_state(state: FileState) -> GitMark {
    match state {
        FileState::Modified | FileState::Renamed => GitMark::Modified,
        FileState::Added => GitMark::Added,
        FileState::Deleted => GitMark::Deleted,
        FileState::Untracked => GitMark::Untracked,
        FileState::Ignored => GitMark::Ignored,
        FileState::Conflicted => GitMark::Conflicted,
    }
}

fn summary_of(status: &RepoStatus) -> GitSummary {
    GitSummary {
        branch: status.branch.clone(),
        head_short: status.head_short.clone(),
        ahead: status.ahead,
        behind: status.behind,
        changed: status
            .entries
            .values()
            .filter(|state| **state != FileState::Ignored)
            .count(),
    }
}

/// Reads the repository of `root` and the marks of `rows`. Blocks. `None` when
/// `stop` ended it.
pub fn scan(root: Option<&Path>, rows: &[ScanRow], stop: &Arc<AtomicBool>) -> Option<GitScan> {
    let status = match root {
        Some(root) => match ncrs_git::status(root, stop) {
            Ok(status) => Some(status),
            Err(ncrs_git::GitError::Interrupted) => return None,
            // A repository git cannot read keeps its column, without marks.
            Err(_) => None,
        },
        None => None,
    };
    let mut marks = HashMap::new();
    for row in rows {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        let mark = if row.is_dir && ncrs_git::is_repository_root(&row.path) {
            Some(GitMark::Repository)
        } else {
            status
                .as_ref()
                .zip(root)
                .and_then(|(status, root)| {
                    ncrs_git::state_of_entry(status, root, &row.path, row.is_dir)
                })
                .map(mark_of_state)
        };
        if let Some(mark) = mark {
            marks.insert(row.name.clone(), mark);
        }
    }
    Some(GitScan {
        summary: status.as_ref().map(summary_of),
        marks,
    })
}

impl Panels {
    /// Starts reading what git says about the folder of `side`, and stops the
    /// read still running for it. The Git column follows the folder at once;
    /// the marks arrive with [`PanelMessage::GitScanned`].
    pub fn start_git_scan(&mut self, side: PanelSide) -> Task<PanelMessage> {
        let panel = self.panel_mut(side);
        panel.git.stop.store(true, Ordering::Relaxed);
        if panel.is_virtual() || panel.is_hit_list() {
            panel.git = PanelGit {
                request_id: panel.git.request_id + 1,
                ..PanelGit::default()
            };
            return Task::none();
        }
        let stays_in_folder = panel.git.folder == panel.path;
        let request_id = panel.git.request_id + 1;
        let stop = Arc::new(AtomicBool::new(false));
        let root = ncrs_git::repository_root_of(&panel.path);
        let (summary, marks) = if stays_in_folder && panel.git.root == root {
            (
                panel.git.summary.take(),
                std::mem::take(&mut panel.git.marks),
            )
        } else {
            (None, HashMap::new())
        };
        panel.git = PanelGit {
            folder: panel.path.clone(),
            root: root.clone(),
            summary,
            marks,
            request_id,
            stop: stop.clone(),
        };
        let rows = scan_rows(panel);
        let delay = if stays_in_folder {
            RELOAD_DEBOUNCE
        } else {
            Duration::ZERO
        };
        Task::perform(
            async move {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                if stop.load(Ordering::Relaxed) {
                    return None;
                }
                tokio::task::spawn_blocking(move || scan(root.as_deref(), &rows, &stop))
                    .await
                    .ok()
                    .flatten()
            },
            move |scan| PanelMessage::GitScanned {
                side,
                request_id,
                scan: scan.map(Box::new),
            },
        )
    }

    /// Takes the answer of a scan, unless the panel moved on since.
    pub fn apply_git_scan(&mut self, side: PanelSide, request_id: u64, scan: Option<GitScan>) {
        let git = &mut self.panel_mut(side).git;
        if let (true, Some(scan)) = (git.request_id == request_id, scan) {
            git.summary = scan.summary;
            git.marks = scan.marks;
        }
    }
}

fn scan_rows(panel: &PanelState) -> Vec<ScanRow> {
    panel
        .entries
        .iter()
        .filter(|entry| !entry.is_parent)
        .map(|entry| ScanRow {
            name: entry.name.clone(),
            path: entry.path.clone(),
            is_dir: entry.is_dir,
        })
        .collect()
}
