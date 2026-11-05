use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::Point;
use ncrs_config::system_monitor::{StatusValue, SystemSettings};
use ncrs_ui_kit::list_window::ListWindow;

use crate::drives::{DriveMeasurer, DriveProbe};
use crate::gate::Gate;
use crate::kill::{KillFailure, KillTarget};
use crate::sampler::{Depth, Sampler, SystemDrives, Wanted};
use crate::snapshot::{DriveRow, NetworkRow, Snapshot, TaskRow};
use crate::tasks::{order_groups, order_tasks, Group, TaskColumn};

/// The kill is not offered in the App Sandbox build.
pub const KILL_OFFERED: bool = cfg!(not(feature = "sandbox"));

pub type SharedSampler = Arc<Mutex<Option<Sampler>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    Kill,
    CopyCommandLine,
    GoToFolder,
}

#[derive(Debug, Clone)]
pub struct Menu {
    /// The task of the row when the menu opened: what its entries act on,
    /// however the list moves meanwhile.
    pub task: TaskRow,
    pub origin: Point,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub struct KillDialog {
    pub target: KillTarget,
    /// The button Enter presses: `0` cancels, `1` kills.
    pub focus: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    Kill { name: String, failure: KillFailure },
    Measure(String),
}

#[derive(Debug)]
pub struct State {
    pub settings: SystemSettings,
    pub open: bool,
    pub sampler: Option<SharedSampler>,
    pub measuring: Gate,
    pub drives: Arc<DriveMeasurer>,
    pub snapshot: Option<Snapshot>,
    pub drive_rows: Option<Vec<DriveRow>>,
    pub column: TaskColumn,
    pub filter: String,
    pub filter_focused: bool,
    /// Positions in the snapshot's tasks, in the order shown.
    pub order: Vec<usize>,
    /// The cursor sits on a row, not on a task: a new measurement may put
    /// another task under it.
    pub cursor_index: usize,
    /// Tasks of one program are shown as one row. Only the page keeps this,
    /// the settings do not.
    pub grouped: bool,
    /// The rows when grouped, in the order shown; `order` then holds the
    /// smallest pid of each.
    pub groups: Vec<Group>,
    pub window: ListWindow,
    pub command_offset: usize,
    pub menu: Option<Menu>,
    pub kill_dialog: Option<KillDialog>,
    pub failure: Option<Failure>,
    pub notice: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self::with_drive_probe(Arc::new(SystemDrives::default()))
    }
}

impl State {
    pub fn with_drive_probe(probe: Arc<dyn DriveProbe>) -> Self {
        let settings = SystemSettings::default();
        Self {
            column: TaskColumn::from_setting(settings.sort),
            settings,
            open: false,
            sampler: None,
            measuring: Gate::default(),
            drives: Arc::new(DriveMeasurer::new(probe)),
            snapshot: None,
            drive_rows: None,
            filter: String::new(),
            filter_focused: false,
            order: Vec::new(),
            cursor_index: 0,
            grouped: false,
            groups: Vec::new(),
            window: ListWindow::default(),
            command_offset: 0,
            menu: None,
            kill_dialog: None,
            failure: None,
            notice: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn settings(&self) -> &SystemSettings {
        &self.settings
    }

    /// Takes over what the config says. Without a reason to measure any more,
    /// the sampler and what it found are let go.
    pub fn set_settings(&mut self, settings: SystemSettings) {
        self.settings = settings;
        self.release_if_idle();
    }

    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    pub fn filter_has_focus(&self) -> bool {
        self.filter_focused
    }

    pub fn filter_text(&self) -> &str {
        &self.filter
    }

    pub fn sorted_by(&self) -> TaskColumn {
        self.column
    }

    pub fn is_grouped(&self) -> bool {
        self.grouped
    }

    pub fn dialog_is_open(&self) -> bool {
        self.kill_dialog.is_some()
    }

    pub fn menu_is_open(&self) -> bool {
        self.menu.is_some()
    }

    pub fn has_sampler(&self) -> bool {
        self.sampler.is_some()
    }

    pub(crate) fn release_if_idle(&mut self) {
        if self.depth().is_none() {
            self.sampler = None;
            self.snapshot = None;
            self.drive_rows = None;
            self.order.clear();
        }
    }

    /// What the next tick measures; `None` when nothing needs measuring.
    pub(crate) fn depth(&self) -> Option<Depth> {
        if self.open {
            return Some(Depth::Full);
        }
        let values = self.settings.badge_values();
        if values.is_empty() {
            return None;
        }
        let has = |value| values.contains(&value);
        Some(Depth::Light(Wanted {
            cpu: has(StatusValue::Cpu),
            memory: has(StatusValue::Memory),
            swap: has(StatusValue::Swap),
            load: has(StatusValue::Load),
            network: has(StatusValue::Network),
        }))
    }

    pub(crate) fn tasks(&self) -> &[TaskRow] {
        self.snapshot
            .as_ref()
            .map_or(&[], |snapshot| snapshot.tasks.as_slice())
    }

    pub(crate) fn task_at_row(&self, row: usize) -> Option<&TaskRow> {
        self.tasks().get(*self.order.get(row)?)
    }

    pub(crate) fn task_by_pid(&self, pid: u32) -> Option<&TaskRow> {
        self.tasks().iter().find(|task| task.pid == pid)
    }

    /// The row as shown: the sums of a program when grouped, else the task.
    pub(crate) fn row_at(&self, row: usize) -> Option<&TaskRow> {
        if self.grouped {
            self.groups.get(row).map(|group| &group.row)
        } else {
            self.task_at_row(row)
        }
    }

    pub(crate) fn group_at(&self, row: usize) -> Option<&Group> {
        self.groups.get(row)
    }

    /// Orders the rows again after a measurement. The cursor and the window
    /// stay where they are, only held inside a list that got shorter.
    pub(crate) fn reorder(&mut self) {
        if self.grouped {
            self.groups = order_groups(self.tasks(), self.column, &self.filter);
            self.order = self.groups.iter().map(Group::representative).collect();
        } else {
            self.groups.clear();
            self.order = order_tasks(self.tasks(), self.column, &self.filter);
        }
        self.cursor_index = self.cursor_index.min(self.order.len().saturating_sub(1));
    }

    /// A new filter, column or grouping is another list: back to the top.
    pub(crate) fn restart_list(&mut self) {
        self.cursor_index = 0;
        self.window = ListWindow::default();
        self.reorder();
    }

    /// The task of the row the cursor is on now (the smallest pid of a program
    /// when grouped).
    pub(crate) fn task_under_cursor(&self) -> Option<&TaskRow> {
        self.task_at_row(self.cursor_index)
    }

    pub fn pid_under_cursor(&self) -> Option<u32> {
        self.task_under_cursor().map(|task| task.pid)
    }

    pub(crate) fn toggle_grouping(&mut self) {
        self.grouped = !self.grouped;
        self.menu = None;
        self.kill_dialog = None;
        self.restart_list();
    }

    pub(crate) fn put_cursor_on_row(&mut self, row: usize) {
        self.cursor_index = row.min(self.order.len().saturating_sub(1));
    }

    /// The longest command among the rows in `from..to`, in characters.
    pub(crate) fn longest_command(&self, from: usize, to: usize) -> usize {
        (from..to)
            .filter_map(|row| self.row_at(row))
            .map(|task| task.command.chars().count())
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn menu_entries(&self) -> Vec<MenuEntry> {
        let mut entries = Vec::new();
        if KILL_OFFERED && !self.grouped {
            entries.push(MenuEntry::Kill);
        }
        entries.push(MenuEntry::CopyCommandLine);
        entries.push(MenuEntry::GoToFolder);
        entries
    }

    pub(crate) fn entry_is_enabled(&self, entry: MenuEntry, task: &TaskRow) -> bool {
        match entry {
            MenuEntry::Kill => !self.grouped && task.pid != 0 && task.pid != std::process::id(),
            MenuEntry::CopyCommandLine => true,
            MenuEntry::GoToFolder => program_folder(task).is_some(),
        }
    }

    pub(crate) fn networks_to_show(&self) -> Vec<&NetworkRow> {
        let show_idle =
            self.settings.idle_interfaces == ncrs_config::system_monitor::Visibility::Show;
        self.snapshot.as_ref().map_or_else(Vec::new, |snapshot| {
            snapshot
                .networks
                .iter()
                .filter(|network| show_idle || network.has_traffic)
                .collect()
        })
    }

    pub(crate) fn drives_to_show(&self) -> Vec<&DriveRow> {
        let show_system =
            self.settings.system_volumes == ncrs_config::system_monitor::Visibility::Show;
        self.drive_rows.as_ref().map_or_else(Vec::new, |rows| {
            rows.iter()
                .filter(|drive| show_system || !drive.is_system_volume)
                .collect()
        })
    }

    pub fn refresh_every(&self) -> Duration {
        self.settings.refresh.duration()
    }
}

/// The folder of the program of a task and the file in it.
pub fn program_folder(task: &TaskRow) -> Option<(std::path::PathBuf, String)> {
    let program = task.program.as_ref()?;
    // The path a process reports is not always canonical, and the panel reads
    // `sftp://…` as a server: only an absolute path is a folder on this disk.
    let folder = program
        .parent()
        .filter(|folder| folder.is_absolute())?
        .to_path_buf();
    let file = program.file_name()?.to_string_lossy().into_owned();
    Some((folder, file))
}

#[cfg(test)]
impl State {
    pub(crate) fn test_put_snapshot(&mut self, snapshot: Snapshot) {
        self.snapshot = Some(snapshot);
        self.reorder();
    }

    pub(crate) fn test_put_drives(&mut self, rows: Vec<DriveRow>) {
        self.drive_rows = Some(rows);
    }

    pub(crate) fn test_cursor_row(&self) -> usize {
        self.cursor_index
    }

    pub(crate) fn test_row_count(&self) -> usize {
        self.order.len()
    }

    pub(crate) fn test_network_names(&self) -> Vec<String> {
        self.networks_to_show()
            .into_iter()
            .map(|network| network.name.clone())
            .collect()
    }

    pub(crate) fn test_drive_names(&self) -> Vec<String> {
        self.drives_to_show()
            .into_iter()
            .map(|drive| drive.name.clone())
            .collect()
    }

    pub(crate) fn test_menu_index_of_go_to_folder(&self) -> usize {
        self.menu_entries()
            .iter()
            .position(|entry| *entry == MenuEntry::GoToFolder)
            .unwrap_or(0)
    }
}
