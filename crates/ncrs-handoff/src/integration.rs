//! The two switches that make ncrs the program other apps and the system hand
//! folders to: the Explorer context menu and the default for folders. Which of
//! them a computer has depends on the system; `available` says.

use std::path::Path;

mod entries;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

pub use entries::{menu_entries, Entry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    /// "Open in ncrs" in the context menu of folders, of the folder background
    /// and of files in the Explorer (Windows).
    ExplorerMenu,
    /// ncrs opens folders when another program asks the system to: the default
    /// verb of folders in the Explorer (Windows), `inode/directory` in
    /// `xdg-mime` (Linux).
    DefaultForFolders,
}

/// Which switches are on right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct State {
    pub explorer_menu: bool,
    pub default_for_folders: bool,
}

impl State {
    pub const fn is_on(self, switch: Switch) -> bool {
        match switch {
            Switch::ExplorerMenu => self.explorer_menu,
            Switch::DefaultForFolders => self.default_for_folders,
        }
    }
}

/// What a change needs from the app.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// A folder of ncrs to keep the earlier choice in while ncrs is the
    /// default (Linux).
    pub memory_dir: Option<&'a Path>,
    /// The words of the Explorer menu entry, in the language of the app.
    pub menu_label: &'a str,
}

/// The switches this system has, in the order the settings show them.
pub const fn available() -> &'static [Switch] {
    #[cfg(windows)]
    {
        &[Switch::ExplorerMenu, Switch::DefaultForFolders]
    }
    #[cfg(target_os = "linux")]
    {
        &[Switch::DefaultForFolders]
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        &[]
    }
}

/// Reads the switches from the system. Starts programs on Linux, so not for
/// the UI thread.
pub fn read() -> State {
    #[cfg(windows)]
    {
        windows::read()
    }
    #[cfg(target_os = "linux")]
    {
        linux::read()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        State::default()
    }
}

/// Turns `switch` on or off and reads the state back. The error is the
/// system's reason in words.
pub fn apply(switch: Switch, on: bool, context: &Context<'_>) -> Result<State, String> {
    if !available().contains(&switch) {
        return Err("this switch does not exist on this system".to_owned());
    }
    #[cfg(windows)]
    {
        windows::apply(switch, on, context)?;
    }
    #[cfg(target_os = "linux")]
    {
        linux::apply(switch, on, context)?;
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (on, context);
    }
    Ok(read())
}
