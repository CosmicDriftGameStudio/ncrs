//! "Compute folder sizes": the folders of one directory are measured one walk
//! at a time, and the values are kept for the session.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use iced::futures::SinkExt as _;
use ncrs_core::{FolderSize, PanelSide};
use ncrs_fs::SortColumn;
use ncrs_ui_kit::file_row::SizeProgress;

use crate::message::PanelMessage;
use crate::panel::PanelState;
use crate::state::Panels;

/// The folder sizes measured in this session, by folder and entry name.
pub type FolderSizes = HashMap<PathBuf, HashMap<OsString, FolderSize>>;

/// The folders of one directory still to be measured, one walk at a time.
#[derive(Debug)]
pub struct FolderSizeRun {
    pub id: u64,
    pub side: PanelSide,
    pub directory: PathBuf,
    pub waiting: VecDeque<PathBuf>,
    /// How many folders the run started with, and the one being walked.
    pub total: usize,
    pub current: Option<PathBuf>,
    /// Bytes counted so far in `current`, while it is walked.
    pub running_bytes: u64,
    pub stop: Arc<AtomicBool>,
}

impl FolderSizeRun {
    /// What the Size cell of `path`, an entry of `directory`, says.
    pub fn progress_of(&self, directory: &Path, path: &Path) -> SizeProgress {
        if self.directory != directory {
            SizeProgress::Idle
        } else if self.current.as_deref() == Some(path) {
            SizeProgress::Running(self.running_bytes)
        } else if self.waiting.iter().any(|waiting| waiting == path) {
            SizeProgress::Waiting
        } else {
            SizeProgress::Idle
        }
    }
}

/// The walk of one folder the app starts on the blocking pool. Its result
/// comes back with the same `run`.
#[derive(Debug)]
pub struct FolderWalk {
    pub run: u64,
    pub folder: PathBuf,
    pub stop: Arc<AtomicBool>,
}

/// The walk of one folder, off the UI thread; nothing when the run is over.
pub fn walk_task(walk: Option<FolderWalk>) -> iced::Task<PanelMessage> {
    let Some(FolderWalk { run, folder, stop }) = walk else {
        return iced::Task::none();
    };
    iced::Task::run(
        iced::stream::channel(RUNNING_REPORTS_KEPT, async move |mut output| {
            let walked = folder.clone();
            let mut running_output = output.clone();
            let running_folder = folder.clone();
            let done = tokio::task::spawn_blocking(move || {
                ncrs_fs::measure_folder_tree_reporting(&walked, &stop, |bytes| {
                    // A full channel drops a report: the next one is larger anyway.
                    drop(running_output.try_send(PanelMessage::FolderSizeRunning {
                        run,
                        folder: running_folder.clone(),
                        bytes,
                    }));
                })
            })
            .await;
            let measured = done.ok().flatten();
            drop(
                output
                    .send(PanelMessage::FolderSizeMeasured {
                        run,
                        folder,
                        size: measured.as_ref().map(|tree| tree.total),
                        below: measured.map(|tree| tree.below).unwrap_or_default(),
                    })
                    .await,
            );
        }),
        std::convert::identity,
    )
}

/// How many running totals wait for the UI before further ones are dropped.
const RUNNING_REPORTS_KEPT: usize = 4;

/// How far a run is, for the progress line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderSizeProgress {
    /// The number of the folder being walked, from 1.
    pub done: usize,
    pub total: usize,
    pub name: String,
}

/// The folders "Compute folder sizes" acts on: the tagged ones, else the one
/// under the cursor, else every folder of the listing when the cursor is on
/// `..`. Links are not entered, so they are not measured.
pub fn folder_size_targets(panel: &PanelState) -> Vec<PathBuf> {
    if panel.is_virtual() || panel.loading {
        return Vec::new();
    }
    let measurable =
        |entry: &ncrs_core::FileEntry| entry.is_dir && !entry.is_parent && !entry.is_symlink;
    if panel.selection.any_tagged() {
        return panel
            .selection
            .ordered(&panel.entries)
            .into_iter()
            .filter(|entry| measurable(entry))
            .map(|entry| entry.path.clone())
            .collect();
    }
    match panel.selected_entry() {
        Some(entry) if entry.is_parent => panel
            .entries
            .iter()
            .filter(|listed| measurable(listed))
            .map(|listed| listed.path.clone())
            .collect(),
        Some(entry) if measurable(entry) => vec![entry.path.clone()],
        _ => Vec::new(),
    }
}

impl Panels {
    /// A reload of the same folder keeps the run; another folder ends it.
    pub fn stop_folder_size_run_if_left(&mut self, side: PanelSide) {
        let left = self
            .folder_size_run
            .as_ref()
            .is_some_and(|run| run.side == side && self.panel(side).path != run.directory);
        if left {
            self.stop_folder_size_run();
        }
    }

    pub fn stop_folder_size_run(&mut self) {
        if let Some(run) = self.folder_size_run.take() {
            run.stop.store(true, Ordering::Relaxed);
        }
    }

    /// Starts a run over the targets of the active panel and returns its first
    /// walk; `None` when there is nothing to measure.
    pub fn start_folder_size_run(&mut self) -> Option<FolderWalk> {
        let targets = folder_size_targets(self.active_panel());
        if targets.is_empty() {
            return None;
        }
        self.stop_folder_size_run();
        self.folder_size_runs_started += 1;
        self.folder_size_run = Some(FolderSizeRun {
            id: self.folder_size_runs_started,
            side: self.active,
            directory: self.active_panel().path.clone(),
            total: targets.len(),
            current: None,
            running_bytes: 0,
            waiting: targets.into(),
            stop: Arc::new(AtomicBool::new(false)),
        });
        self.next_folder_size_walk()
    }

    /// The walk of the next waiting folder; the run is over when none is left.
    pub fn next_folder_size_walk(&mut self) -> Option<FolderWalk> {
        let run = self.folder_size_run.as_mut()?;
        let Some(folder) = run.waiting.pop_front() else {
            self.folder_size_run = None;
            return None;
        };
        run.current = Some(folder.clone());
        run.running_bytes = 0;
        Some(FolderWalk {
            run: run.id,
            folder,
            stop: Arc::clone(&run.stop),
        })
    }

    /// The folder being walked and how many of the run there are, while a run
    /// is on.
    pub fn folder_size_progress(&self) -> Option<FolderSizeProgress> {
        let run = self.folder_size_run.as_ref()?;
        let name = run
            .current
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())?;
        Some(FolderSizeProgress {
            done: run.total.saturating_sub(run.waiting.len()),
            total: run.total,
            name,
        })
    }

    /// What the Size cell of `path`, an entry of `directory`, says while a run
    /// is on.
    pub fn folder_size_progress_of(&self, directory: &Path, path: &Path) -> SizeProgress {
        self.folder_size_run
            .as_ref()
            .map_or(SizeProgress::Idle, |run| run.progress_of(directory, path))
    }

    /// The total counted so far in the folder being walked; reports of an
    /// older walk or run are dropped.
    pub fn folder_size_running(&mut self, run: u64, folder: &Path, bytes: u64) {
        if let Some(current) = self
            .folder_size_run
            .as_mut()
            .filter(|current| current.id == run && current.current.as_deref() == Some(folder))
        {
            current.running_bytes = current.running_bytes.max(bytes);
        }
    }

    /// A walk is done: keeps its size and the sizes of the folders below it,
    /// shows the size in the panels that list the folder, and returns the next
    /// walk. A result of another run is dropped.
    pub fn folder_size_measured(
        &mut self,
        run: u64,
        folder: PathBuf,
        measured: Option<FolderSize>,
        below: Vec<(PathBuf, FolderSize)>,
        visible_rows: usize,
    ) -> Option<FolderWalk> {
        if self.folder_size_run.as_ref().map(|current| current.id) != Some(run) {
            return None;
        }
        if measured.is_some() {
            for (path, size) in below {
                self.keep_folder_size(&path, size);
            }
        }
        if let (Some(size), Some(parent), Some(name)) =
            (measured, folder.parent(), folder.file_name())
        {
            self.keep_folder_size(&folder, size);
            for side in [PanelSide::Left, PanelSide::Right] {
                let panel = self.panel_mut(side);
                if panel.path != parent || panel.is_virtual() {
                    continue;
                }
                let Some(entry) = panel
                    .entries
                    .iter_mut()
                    .find(|entry| entry.is_dir && entry.name == name)
                else {
                    continue;
                };
                entry.folder_size = Some(size);
                if panel.sort.column == SortColumn::Size {
                    panel.resort(visible_rows);
                }
            }
        }
        self.next_folder_size_walk()
    }

    /// Keeps the size of `folder` under its parent, for the listing of it.
    fn keep_folder_size(&mut self, folder: &Path, size: FolderSize) {
        if let (Some(parent), Some(name)) = (folder.parent(), folder.file_name()) {
            self.folder_sizes
                .entry(parent.to_path_buf())
                .or_default()
                .insert(name.to_owned(), size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::PanelState;

    #[test]
    fn the_sizes_of_the_folders_below_are_kept_under_their_own_parent() {
        let mut panels = Panels::new(
            PanelState::new(PathBuf::from("/a")),
            PanelState::new(PathBuf::from("/b")),
            PanelSide::Left,
        );
        panels.folder_size_run = Some(FolderSizeRun {
            id: 1,
            side: PanelSide::Left,
            directory: PathBuf::from("/a"),
            total: 1,
            current: Some(PathBuf::from("/a/tree")),
            running_bytes: 0,
            waiting: VecDeque::new(),
            stop: Arc::new(AtomicBool::new(false)),
        });
        let size = |bytes| FolderSize {
            bytes,
            partial: false,
        };
        panels.folder_size_measured(
            1,
            PathBuf::from("/a/tree"),
            Some(size(30)),
            vec![
                (PathBuf::from("/a/tree/one"), size(20)),
                (PathBuf::from("/a/tree/one/two"), size(5)),
            ],
            10,
        );
        assert_eq!(
            panels.folder_sizes[Path::new("/a")][std::ffi::OsStr::new("tree")],
            size(30)
        );
        assert_eq!(
            panels.folder_sizes[Path::new("/a/tree")][std::ffi::OsStr::new("one")],
            size(20)
        );
        assert_eq!(
            panels.folder_sizes[Path::new("/a/tree/one")][std::ffi::OsStr::new("two")],
            size(5)
        );
    }

    #[test]
    fn a_stopped_walk_keeps_nothing_below() {
        let mut panels = Panels::new(
            PanelState::new(PathBuf::from("/a")),
            PanelState::new(PathBuf::from("/b")),
            PanelSide::Left,
        );
        panels.folder_size_run = Some(FolderSizeRun {
            id: 1,
            side: PanelSide::Left,
            directory: PathBuf::from("/a"),
            total: 2,
            current: Some(PathBuf::from("/a/tree")),
            running_bytes: 0,
            waiting: VecDeque::from([PathBuf::from("/a/other")]),
            stop: Arc::new(AtomicBool::new(false)),
        });
        let size = FolderSize {
            bytes: 1,
            partial: false,
        };
        panels.folder_size_measured(
            1,
            PathBuf::from("/a/tree"),
            None,
            vec![(PathBuf::from("/a/tree/one"), size)],
            10,
        );
        assert!(panels.folder_sizes.is_empty());
        let progress = panels.folder_size_progress().unwrap();
        assert_eq!((progress.done, progress.total), (2, 2));
        assert_eq!(progress.name, "other");
    }
}

#[cfg(test)]
mod running_tests {
    use super::*;

    fn panels_with_run() -> Panels {
        let mut panels = Panels::new(
            PanelState::new(PathBuf::from("/a")),
            PanelState::new(PathBuf::from("/b")),
            PanelSide::Left,
        );
        panels.folder_size_run = Some(FolderSizeRun {
            id: 1,
            side: PanelSide::Left,
            directory: PathBuf::from("/a"),
            total: 2,
            current: Some(PathBuf::from("/a/tree")),
            running_bytes: 0,
            waiting: VecDeque::from([PathBuf::from("/a/other")]),
            stop: Arc::new(AtomicBool::new(false)),
        });
        panels
    }

    #[test]
    fn the_running_total_of_the_walked_folder_grows_and_the_waiting_one_says_so() {
        let mut panels = panels_with_run();
        let directory = Path::new("/a");
        assert_eq!(
            panels.folder_size_progress_of(directory, Path::new("/a/tree")),
            SizeProgress::Running(0)
        );
        panels.folder_size_running(1, Path::new("/a/tree"), 500);
        panels.folder_size_running(1, Path::new("/a/tree"), 300);
        panels.folder_size_running(1, Path::new("/a/tree"), 900);
        assert_eq!(
            panels.folder_size_progress_of(directory, Path::new("/a/tree")),
            SizeProgress::Running(900)
        );
        assert_eq!(
            panels.folder_size_progress_of(directory, Path::new("/a/other")),
            SizeProgress::Waiting
        );
        assert_eq!(
            panels.folder_size_progress_of(directory, Path::new("/a/done")),
            SizeProgress::Idle
        );
        assert_eq!(
            panels.folder_size_progress_of(Path::new("/b"), Path::new("/a/tree")),
            SizeProgress::Idle
        );
    }

    #[test]
    fn a_report_of_another_run_or_folder_is_dropped_and_the_next_walk_starts_at_zero() {
        let mut panels = panels_with_run();
        panels.folder_size_running(2, Path::new("/a/tree"), 700);
        panels.folder_size_running(1, Path::new("/a/other"), 700);
        assert_eq!(
            panels.folder_size_progress_of(Path::new("/a"), Path::new("/a/tree")),
            SizeProgress::Running(0)
        );
        panels.folder_size_running(1, Path::new("/a/tree"), 700);
        let next = panels.folder_size_measured(1, PathBuf::from("/a/tree"), None, Vec::new(), 10);
        assert!(next.is_some());
        assert_eq!(
            panels.folder_size_progress_of(Path::new("/a"), Path::new("/a/other")),
            SizeProgress::Running(0)
        );
    }
}
