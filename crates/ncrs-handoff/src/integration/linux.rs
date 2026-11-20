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
        None => remove_own_default(),
    }
}

/// With no earlier choice, the system falls back to its own default once the
/// entry `xdg-mime default` wrote for ncrs is gone.
fn remove_own_default() -> Result<(), String> {
    let Some(path) = mimeapps_path() else {
        return Err("the folder of mimeapps.list is not known".to_owned());
    };
    let list = match fs::read_to_string(&path) {
        Ok(list) => list,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let cleaned = without_own_folder_default(&list);
    if cleaned == list {
        return Ok(());
    }
    fs::write(&path, cleaned).map_err(|error| error.to_string())
}

fn mimeapps_path() -> Option<std::path::PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".config"))
        })?;
    Some(config.join("mimeapps.list"))
}

/// The `[Default Applications]` section of a mimeapps.list without ncrs as
/// opener of folders; other openers listed for folders stay.
fn without_own_folder_default(list: &str) -> String {
    let prefix = format!("{FOLDER_TYPE}=");
    let mut in_defaults = false;
    let mut kept = Vec::new();
    for line in list.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_defaults = trimmed == "[Default Applications]";
        } else if in_defaults {
            if let Some(value) = trimmed
                .strip_prefix(&prefix)
                .filter(|value| value.split(';').any(is_ours))
            {
                let others: Vec<&str> = value
                    .split(';')
                    .map(str::trim)
                    .filter(|desktop| !desktop.is_empty() && !is_ours(desktop))
                    .collect();
                if others.is_empty() {
                    continue;
                }
                kept.push(format!("{prefix}{};\n", others.join(";")));
                continue;
            }
        }
        kept.push(line.to_owned());
    }
    kept.concat()
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
    fn the_entry_of_ncrs_leaves_the_list_and_the_rest_stays() {
        let list = "[Default Applications]\ntext/plain=a.desktop\ninode/directory=ncrs.desktop\n\n[Added Associations]\ninode/directory=ncrs.desktop;\n";
        assert_eq!(
            without_own_folder_default(list),
            "[Default Applications]\ntext/plain=a.desktop\n\n[Added Associations]\ninode/directory=ncrs.desktop;\n"
        );
        let shared = "[Default Applications]\ninode/directory=ncrs.desktop;b.desktop;\n";
        assert_eq!(
            without_own_folder_default(shared),
            "[Default Applications]\ninode/directory=b.desktop;\n"
        );
        let other = "[Default Applications]\ninode/directory=b.desktop\n";
        assert_eq!(without_own_folder_default(other), other);
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
