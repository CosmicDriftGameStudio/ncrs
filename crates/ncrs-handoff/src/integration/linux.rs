//! ncrs as the opener of folders in the desktop's MIME database, through the
//! `xdg-mime` of the desktop (xdg-utils), which writes `mimeapps.list`.

use std::fs;
use std::process::Command;

use super::{Context, State, Switch};

const FOLDER_TYPE: &str = "inode/directory";
const DESKTOP_FILE: &str = "ncrs.desktop";
const MEMORY_FILE: &str = "earlier-folder-opener";

pub fn read() -> State {
    State {
        explorer_menu: false,
        default_for_folders: query_default().is_some_and(|desktop| is_ours(&desktop)),
    }
}

pub fn apply(switch: Switch, on: bool, context: &Context<'_>) -> Result<(), String> {
    match (switch, on) {
        (Switch::DefaultForFolders, true) => take_default(context),
        (Switch::DefaultForFolders, false) => restore_default(context),
        (Switch::ExplorerMenu, _) => Ok(()),
    }
}

fn is_ours(desktop_file: &str) -> bool {
    desktop_file.trim() == DESKTOP_FILE
}

/// What to give back to `xdg-mime`: the remembered desktop file, if there is
/// one that is not ncrs'.
fn earlier_choice(remembered: &str) -> Option<&str> {
    let choice = remembered.trim();
    (!choice.is_empty() && !is_ours(choice)).then_some(choice)
}

fn query_default() -> Option<String> {
    let output = Command::new("xdg-mime")
        .args(["query", "default", FOLDER_TYPE])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn set_default(desktop_file: &str) -> Result<(), String> {
    let status = Command::new("xdg-mime")
        .args(["default", desktop_file, FOLDER_TYPE])
        .status()
        .map_err(|error| format!("xdg-mime could not be started: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("xdg-mime failed: {status}"))
    }
}

fn take_default(context: &Context<'_>) -> Result<(), String> {
    let earlier = query_default().unwrap_or_default();
    if !is_ours(&earlier) {
        if let Some(dir) = context.memory_dir {
            fs::create_dir_all(dir)
                .and_then(|()| fs::write(dir.join(MEMORY_FILE), &earlier))
                .map_err(|error| error.to_string())?;
        }
    }
    set_default(DESKTOP_FILE)
}

fn restore_default(context: &Context<'_>) -> Result<(), String> {
    let remembered = context
        .memory_dir
        .and_then(|dir| fs::read_to_string(dir.join(MEMORY_FILE)).ok())
        .unwrap_or_default();
    match earlier_choice(&remembered) {
        Some(choice) => set_default(choice),
        None => Err("there is no earlier choice to go back to".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_desktop_file_of_ncrs_counts_as_ours() {
        assert!(is_ours("ncrs.desktop\n"));
        assert!(!is_ours("org.gnome.Nautilus.desktop"));
        assert!(!is_ours(""));
    }

    #[test]
    fn nothing_is_given_back_when_nothing_or_ncrs_was_remembered() {
        assert_eq!(
            earlier_choice("org.gnome.Nautilus.desktop\n"),
            Some("org.gnome.Nautilus.desktop")
        );
        assert_eq!(earlier_choice(""), None);
        assert_eq!(earlier_choice("ncrs.desktop"), None);
    }
}
