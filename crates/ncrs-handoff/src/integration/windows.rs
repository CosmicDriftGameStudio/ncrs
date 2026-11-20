//! The Explorer entries under `HKEY_CURRENT_USER\Software\Classes`: no
//! installer writes them (ncrs comes as a zip), so the settings do, and take
//! them away again.

use std::io;

use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

use super::entries::MENU_PLACES;
use super::{menu_entries, Context, Entry, State, Switch};

const FOLDER_VERBS: &str = r"Software\Classes\Directory\shell";
const FOLDER_VERB: &str = r"Software\Classes\Directory\shell\ncrs";
const VERB: &str = "ncrs";
/// What the default verb of folders was before ncrs took it, kept beside it.
const EARLIER_DEFAULT: &str = "NcrsPreviousDefault";

pub fn read() -> State {
    let root = RegKey::predef(HKEY_CURRENT_USER);
    State {
        explorer_menu: MENU_PLACES
            .iter()
            .all(|(key, _)| root.open_subkey(key).is_ok()),
        default_for_folders: root
            .open_subkey(FOLDER_VERBS)
            .and_then(|key| key.get_value::<String, _>(""))
            .is_ok_and(|verb| verb == VERB),
    }
}

pub fn apply(switch: Switch, on: bool, context: &Context<'_>) -> Result<(), String> {
    let root = RegKey::predef(HKEY_CURRENT_USER);
    match (switch, on) {
        (Switch::ExplorerMenu, true) => write_menu(&root, context.menu_label),
        (Switch::ExplorerMenu, false) => restore_default(&root).and_then(|()| remove_menu(&root)),
        (Switch::DefaultForFolders, true) => take_default(&root, context.menu_label),
        (Switch::DefaultForFolders, false) => restore_default(&root).and_then(|()| {
            if read().explorer_menu {
                Ok(())
            } else {
                remove_key(&root, FOLDER_VERB)
            }
        }),
    }
    .map_err(|error| error.to_string())
}

fn exe_path() -> io::Result<String> {
    Ok(std::env::current_exe()?.to_string_lossy().into_owned())
}

fn write_entries(root: &RegKey, entries: &[Entry]) -> io::Result<()> {
    for entry in entries {
        let (key, _) = root.create_subkey(&entry.key)?;
        key.set_value(&entry.name, &entry.data.as_str())?;
    }
    Ok(())
}

fn write_menu(root: &RegKey, label: &str) -> io::Result<()> {
    write_entries(root, &menu_entries(&exe_path()?, label))
}

fn remove_menu(root: &RegKey) -> io::Result<()> {
    for (key, _) in MENU_PLACES {
        remove_key(root, key)?;
    }
    Ok(())
}

fn remove_key(root: &RegKey, path: &str) -> io::Result<()> {
    ignore_missing(root.delete_subkey_all(path))
}

fn ignore_missing(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The verb of folders has to exist for the default to name it, so the folder
/// entry is written whether or not the menu switch is on.
fn take_default(root: &RegKey, label: &str) -> io::Result<()> {
    let folder_verb: Vec<Entry> = menu_entries(&exe_path()?, label)
        .into_iter()
        .filter(|entry| entry.key.starts_with(FOLDER_VERB))
        .collect();
    write_entries(root, &folder_verb)?;
    let (key, _) = root.create_subkey(FOLDER_VERBS)?;
    let current = key.get_value::<String, _>("").ok();
    match current {
        Some(verb) if verb == VERB => {}
        Some(earlier) => key.set_value(EARLIER_DEFAULT, &earlier.as_str())?,
        None => ignore_missing(key.delete_value(EARLIER_DEFAULT))?,
    }
    key.set_value("", &VERB)
}

/// Gives the default verb back to what it was, but only if it is ncrs'.
fn restore_default(root: &RegKey) -> io::Result<()> {
    let key = match root.open_subkey_with_flags(FOLDER_VERBS, KEY_READ | KEY_WRITE) {
        Ok(key) => key,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if key.get_value::<String, _>("").ok().as_deref() != Some(VERB) {
        return Ok(());
    }
    match key.get_value::<String, _>(EARLIER_DEFAULT) {
        Ok(earlier) => {
            key.set_value("", &earlier.as_str())?;
            ignore_missing(key.delete_value(EARLIER_DEFAULT))
        }
        Err(_) => ignore_missing(key.delete_value("")),
    }
}
