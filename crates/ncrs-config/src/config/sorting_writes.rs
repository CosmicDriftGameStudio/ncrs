//! Writing targets and sources into `[modules.filing]`.
//!
//! The first write of the new form copies the config next to itself as
//! `config.toml.before-1.8.0` (an older ncrs rejects the new keys and would
//! drop the whole file) and moves what the old `locations` said into the new
//! lists, so nothing is kept in two places.

use super::*;
use ncrs_index::TargetId;
use ncrs_intake::{SortSource, SortTarget};

/// Added to the config's file name for the copy made before the first write
/// of targets and sources.
pub const BACKUP_SUFFIX: &str = ".before-1.8.0";

const TARGETS: &str = "targets";
const SOURCES: &str = "sources";
const NEXT_TARGET: &str = "next_target";
/// The first number a `t<n>` id gets: `main` is the first target.
const FIRST_NUMBERED_TARGET: i64 = 2;

/// The filing settings of the config at `path` as the next start reads them.
/// A change of targets or sources applies by loading them again with this.
pub fn load_filing_settings(path: &Path) -> Result<Filing, ConfigError> {
    load(path).map(|config| config.filing)
}

/// Adds a target with the next free id: `main` while no target was ever
/// there, else `t<n>`. Returns the id and the config read back.
pub fn add_sort_target(
    path: &Path,
    name: &str,
    folder: &Path,
    own_volume: Option<bool>,
) -> Result<(TargetId, Config), FilingWriteError> {
    add_sort_target_with_home(path, name, folder, own_volume, home_from_env().as_deref())
}

pub fn add_sort_target_with_home(
    path: &Path,
    name: &str,
    folder: &Path,
    own_volume: Option<bool>,
    home: Option<&Path>,
) -> Result<(TargetId, Config), FilingWriteError> {
    let mut added = None;
    let config = write_sorting(path, home, |filing| {
        let id = allocate_target_id(filing, path)?;
        let target = SortTarget {
            id: id.clone(),
            name: name.to_owned(),
            path: folder.to_path_buf(),
            own_volume,
        };
        added = Some(id);
        put_target(filing, path, &target)
    })?;
    let id = added.ok_or_else(|| not_a_table(path, TARGETS))?;
    Ok((id, config))
}

/// Replaces the target with the id of `target`, or appends it.
pub fn upsert_sort_target(path: &Path, target: &SortTarget) -> Result<Config, FilingWriteError> {
    upsert_sort_target_with_home(path, target, home_from_env().as_deref())
}

pub fn upsert_sort_target_with_home(
    path: &Path,
    target: &SortTarget,
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    write_sorting(path, home, |filing| put_target(filing, path, target))
}

/// Removes the target; sources that name it stay and are paused until they
/// name another. Its id is never handed out again.
pub fn remove_sort_target(path: &Path, id: &TargetId) -> Result<Config, FilingWriteError> {
    remove_sort_target_with_home(path, id, home_from_env().as_deref())
}

pub fn remove_sort_target_with_home(
    path: &Path,
    id: &TargetId,
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    write_sorting(path, home, |filing| {
        remember_target_ids(filing, path)?;
        let mut list = take_list(filing, TARGETS, path)?;
        let found = list.iter().position(|entry| {
            entry.get("id").and_then(toml_edit::Item::as_str) == Some(id.as_str())
        });
        if let Some(index) = found {
            list.remove(index);
        }
        put_list(filing, TARGETS, list);
        Ok(())
    })
}

/// Replaces the source at the folder `replacing` (the folder of `source` when
/// `None`), or appends it.
pub fn upsert_sort_source(
    path: &Path,
    source: &SortSource,
    replacing: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    upsert_sort_source_with_home(path, source, replacing, home_from_env().as_deref())
}

pub fn upsert_sort_source_with_home(
    path: &Path,
    source: &SortSource,
    replacing: Option<&Path>,
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    write_sorting(path, home, |filing| {
        let mut list = take_list(filing, SOURCES, path)?;
        let folder = replacing.unwrap_or(&source.path);
        let index = list
            .iter()
            .position(|entry| entry_is_at(entry, folder, home));
        let filled = match index.and_then(|found| list.get_mut(found)) {
            Some(entry) => fill_source(entry, source),
            None => {
                let mut entry = toml_edit::Table::new();
                let filled = fill_source(&mut entry, source);
                list.push(entry);
                filled
            }
        };
        put_list(filing, SOURCES, list);
        filled
    })
}

/// Removes the source at `folder`. The list stays in the file, even when it
/// is empty, so the old `locations` do not bring the source back.
pub fn remove_sort_source(path: &Path, folder: &Path) -> Result<Config, FilingWriteError> {
    remove_sort_source_with_home(path, folder, home_from_env().as_deref())
}

pub fn remove_sort_source_with_home(
    path: &Path,
    folder: &Path,
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    write_sorting(path, home, |filing| {
        let mut list = take_list(filing, SOURCES, path)?;
        let found = list
            .iter()
            .position(|entry| entry_is_at(entry, folder, home));
        if let Some(index) = found {
            list.remove(index);
        }
        put_list(filing, SOURCES, list);
        Ok(())
    })
}

/// Like `write_edited`, on the `[modules.filing]` table, and the first write
/// of the new form migrates the old one and keeps a copy of the old file.
fn write_sorting(
    path: &Path,
    home: Option<&Path>,
    edit: impl FnOnce(&mut toml_edit::Table) -> Result<(), FilingWriteError>,
) -> Result<Config, FilingWriteError> {
    let target = resolve_write_target(path)?;
    let _writing = one_config_writer(&target);
    let source = read_source(&target).map_err(FilingWriteError::Config)?;
    let before = match &source {
        Some(text) => Some(parse_config(text, path, home).map_err(FilingWriteError::Config)?),
        None => None,
    };
    let mut document = parse_document(source.as_deref().unwrap_or_default(), path)?;
    let modules = sub_table(document.as_table_mut(), "modules", true)
        .ok_or_else(|| not_a_table(path, "modules"))?;
    let filing =
        sub_table(modules, "filing", false).ok_or_else(|| not_a_table(path, "modules.filing"))?;
    let first_of_the_new_form = filing.get(TARGETS).is_none() && filing.get(SOURCES).is_none();
    if first_of_the_new_form {
        let derived = before.map(|config| config.filing).unwrap_or_default();
        migrate(filing, path, &derived)?;
    }
    edit(filing)?;
    let edited = document.to_string();
    let written = parse_config(&edited, path, home).map_err(FilingWriteError::Config)?;
    if first_of_the_new_form && source.is_some() {
        back_up(&target)?;
    }
    replace_file(&target, &edited)?;
    Ok(written)
}

/// The targets and sources the old `locations` stood for become entries;
/// inboxes leave `locations` and archives lose their flags, so only the index
/// folders remain there.
fn migrate(
    filing: &mut toml_edit::Table,
    path: &Path,
    derived: &Filing,
) -> Result<(), FilingWriteError> {
    let mut targets = toml_edit::ArrayOfTables::new();
    for setting in &derived.targets {
        let mut entry = toml_edit::Table::new();
        fill_target(&mut entry, &setting.target)?;
        targets.push(entry);
    }
    let mut sources = toml_edit::ArrayOfTables::new();
    for setting in &derived.sources {
        let mut entry = toml_edit::Table::new();
        fill_source(&mut entry, &setting.source)?;
        sources.push(entry);
    }
    put_list(filing, TARGETS, targets);
    put_list(filing, SOURCES, sources);
    remember_target_ids(filing, path)?;
    if let Some(locations) = filing
        .get_mut("locations")
        .and_then(toml_edit::Item::as_array_of_tables_mut)
    {
        locations.retain(|entry| entry.get("mode").and_then(toml_edit::Item::as_str) != Some("inbox"));
        for entry in locations.iter_mut() {
            for key in ["target", "transfer", "continuous"] {
                entry.remove(key);
            }
        }
        if locations.is_empty() {
            filing.remove("locations");
        }
    }
    Ok(())
}

fn back_up(target: &Path) -> Result<(), FilingWriteError> {
    let mut name = target.as_os_str().to_owned();
    name.push(BACKUP_SUFFIX);
    let backup = PathBuf::from(name);
    // Never over an existing copy: that one is the config before 1.8.0.
    if backup.symlink_metadata().is_ok() {
        return Ok(());
    }
    // `copy` gives the copy the permissions of the original.
    std::fs::copy(target, &backup)
        .map(|_| ())
        .map_err(io_error(&backup))
}

fn entry_is_at(entry: &toml_edit::Table, folder: &Path, home: Option<&Path>) -> bool {
    entry
        .get("path")
        .and_then(toml_edit::Item::as_str)
        .and_then(|text| expand_location_path(text, home).ok())
        .is_some_and(|found| same_folder(&ncrs_intake::without_verbatim_prefix(found), folder))
}

/// The list `key` of `filing`, taken out of it; an inline `[]` or an array of
/// inline tables counts, a missing key is empty.
fn take_list(
    filing: &mut toml_edit::Table,
    key: &str,
    path: &Path,
) -> Result<toml_edit::ArrayOfTables, FilingWriteError> {
    match filing.remove(key) {
        None => Ok(toml_edit::ArrayOfTables::new()),
        Some(toml_edit::Item::Value(toml_edit::Value::Array(listed))) if listed.is_empty() => {
            Ok(toml_edit::ArrayOfTables::new())
        }
        Some(item) => item
            .into_array_of_tables()
            .map_err(|_| not_a_table(path, key)),
    }
}

/// An empty list is written as `key = []`: the key has to stay, because its
/// presence is what keeps the old `locations` out of it.
fn put_list(filing: &mut toml_edit::Table, key: &str, list: toml_edit::ArrayOfTables) {
    if list.is_empty() {
        filing.insert(
            key,
            toml_edit::Item::Value(toml_edit::Value::Array(toml_edit::Array::new())),
        );
    } else {
        filing.insert(key, toml_edit::Item::ArrayOfTables(list));
    }
}

fn put_target(
    filing: &mut toml_edit::Table,
    path: &Path,
    target: &SortTarget,
) -> Result<(), FilingWriteError> {
    let mut list = take_list(filing, TARGETS, path)?;
    let index = list.iter().position(|entry| {
        entry.get("id").and_then(toml_edit::Item::as_str) == Some(target.id.as_str())
    });
    let filled = match index.and_then(|found| list.get_mut(found)) {
        Some(entry) => fill_target(entry, target),
        None => {
            let mut entry = toml_edit::Table::new();
            let filled = fill_target(&mut entry, target);
            list.push(entry);
            filled
        }
    };
    put_list(filing, TARGETS, list);
    remember_target_ids(filing, path)?;
    filled
}

fn fill_target(entry: &mut toml_edit::Table, target: &SortTarget) -> Result<(), FilingWriteError> {
    entry.insert("id", toml_edit::value(target.id.as_str()));
    entry.insert("name", toml_edit::value(target.name.as_str()));
    entry.insert("path", toml_edit::value(path_text(&target.path)?));
    put_own_volume(entry, target.own_volume);
    Ok(())
}

fn put_own_volume(entry: &mut toml_edit::Table, own_volume: Option<bool>) {
    match own_volume {
        Some(own) => {
            entry.insert("own_volume", toml_edit::value(own));
        }
        None => {
            entry.remove("own_volume");
        }
    }
}

fn fill_source(entry: &mut toml_edit::Table, source: &SortSource) -> Result<(), FilingWriteError> {
    entry.insert("path", toml_edit::value(path_text(&source.path)?));
    for (key, text) in [("include", &source.include), ("exclude", &source.exclude)] {
        if text.is_empty() {
            entry.remove(key);
        } else {
            entry.insert(key, toml_edit::value(text.as_str()));
        }
    }
    let mut targets = toml_edit::Array::new();
    for id in &source.targets {
        targets.push(id.as_str());
    }
    entry.insert("targets", toml_edit::value(targets));
    entry.insert("when", toml_edit::value(watch_word(source.watch)));
    entry.insert("files", toml_edit::value(files_word(source.transfer)));
    entry.insert("no_text", toml_edit::value(no_text_word(source.no_text)));
    entry.insert("several", toml_edit::value(several_word(source.several)));
    put_own_volume(entry, source.own_volume);
    Ok(())
}

fn path_text(path: &Path) -> Result<&str, FilingWriteError> {
    path.to_str()
        .ok_or_else(|| FilingWriteError::PathNotText(path.to_path_buf()))
}

/// The number of a `t<n>` id; `main` and anything else has none.
fn numbered(id: &str) -> Option<i64> {
    id.strip_prefix('t')?.parse().ok().filter(|number| *number >= FIRST_NUMBERED_TARGET)
}

fn target_ids(filing: &toml_edit::Table) -> Vec<String> {
    filing
        .get(TARGETS)
        .and_then(toml_edit::Item::as_array_of_tables)
        .map(|list| {
            list.iter()
                .filter_map(|entry| entry.get("id").and_then(toml_edit::Item::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Writes `next_target` so that no id of a target in the file is handed out
/// again once the target is gone. Nothing is written for a file that never had
/// a target, `main` is still free there.
fn remember_target_ids(filing: &mut toml_edit::Table, path: &Path) -> Result<(), FilingWriteError> {
    let ids = target_ids(filing);
    let written = match filing.get(NEXT_TARGET) {
        None => None,
        Some(item) => Some(
            item.as_integer()
                .ok_or_else(|| not_a_table(path, NEXT_TARGET))?,
        ),
    };
    if ids.is_empty() && written.is_none() {
        return Ok(());
    }
    let highest = ids.iter().filter_map(|id| numbered(id)).max().unwrap_or(0);
    let next = written
        .unwrap_or(0)
        .max(highest + 1)
        .max(FIRST_NUMBERED_TARGET);
    filing.insert(NEXT_TARGET, toml_edit::value(next));
    Ok(())
}

fn allocate_target_id(
    filing: &toml_edit::Table,
    path: &Path,
) -> Result<TargetId, FilingWriteError> {
    let ids = target_ids(filing);
    let written = filing.get(NEXT_TARGET).and_then(toml_edit::Item::as_integer);
    let taken = |wanted: &str| ids.iter().any(|id| id == wanted);
    let id = if written.is_none() && !taken("main") {
        TargetId::main()
    } else {
        let highest = ids.iter().filter_map(|id| numbered(id)).max().unwrap_or(0);
        let number = written
            .unwrap_or(0)
            .max(highest + 1)
            .max(FIRST_NUMBERED_TARGET);
        TargetId::new(&format!("t{number}"))
            .ok_or_else(|| not_a_table(path, NEXT_TARGET))?
    };
    Ok(id)
}
