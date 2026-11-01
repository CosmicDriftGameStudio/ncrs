//! "Compute folder sizes": the folders of one directory are measured one walk
//! at a time, and the values are kept for the session.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ncrs_core::{FolderSize, PanelSide};
use ncrs_fs::SortColumn;

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
    pub stop: Arc<AtomicBool>,
}

/// The walk of one folder the app starts on the blocking pool. Its result
/// comes back with the same `run`.
#[derive(Debug)]
pub struct FolderWalk {
    pub run: u64,
    pub folder: PathBuf,
    pub stop: Arc<AtomicBool>,
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
        Some(FolderWalk {
            run: run.id,
            folder,
            stop: Arc::clone(&run.stop),
        })
    }

    /// A walk is done: keeps the size, shows it in the panels that list the
    /// folder, and returns the next walk. A result of another run is dropped.
    pub fn folder_size_measured(
        &mut self,
        run: u64,
        folder: PathBuf,
        measured: Option<FolderSize>,
        visible_rows: usize,
    ) -> Option<FolderWalk> {
        if self.folder_size_run.as_ref().map(|current| current.id) != Some(run) {
            return None;
        }
        if let (Some(size), Some(parent), Some(name)) =
            (measured, folder.parent(), folder.file_name())
        {
            self.folder_sizes
                .entry(parent.to_path_buf())
                .or_default()
                .insert(name.to_owned(), size);
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
}
