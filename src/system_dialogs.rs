//! The system dialogs. Called from `update`: on macOS rfd sets up the panel as
//! soon as the dialog is created, which AppKit allows only on the main thread.

use std::path::{Path, PathBuf};

use iced::Task;

/// Opens the system folder dialog in `start_in` and yields the folder picked,
/// or `None` when it was cancelled.
pub fn pick_folder_with_system_dialog(start_in: &Path) -> Task<Option<PathBuf>> {
    // `pick_folder()` runs here, not inside the task.
    let picked = rfd::AsyncFileDialog::new()
        .set_directory(start_in)
        .pick_folder();
    Task::perform(picked, |chosen| {
        chosen.map(|handle| handle.path().to_path_buf())
    })
}

/// What the file dialog of the settings tab picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilePick {
    /// The program behind F3 or F4; only app bundles in the App Store build.
    Program { apps_only: bool },
    /// A VS Code color theme.
    ThemeFile,
}

/// Opens the system file dialog for `pick` and yields the file picked, or
/// `None` when it was cancelled. Called from `update`, like
/// [`pick_folder_with_system_dialog`].
pub fn pick_file_with_system_dialog(pick: FilePick) -> Task<Option<PathBuf>> {
    let plain_dialog = rfd::AsyncFileDialog::new();
    let dialog = match pick {
        FilePick::Program { apps_only } => {
            let in_apps = if cfg!(target_os = "macos") {
                plain_dialog.set_directory("/Applications")
            } else {
                plain_dialog
            };
            if apps_only {
                in_apps.add_filter("App", &["app"])
            } else {
                in_apps
            }
        }
        FilePick::ThemeFile => plain_dialog.add_filter("JSON", &["json"]),
    };
    Task::perform(dialog.pick_file(), |chosen| {
        chosen.map(|handle| handle.path().to_path_buf())
    })
}
