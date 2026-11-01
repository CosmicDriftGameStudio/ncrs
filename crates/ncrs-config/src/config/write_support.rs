use super::*;

pub fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> FilingWriteError + '_ {
    move |err| FilingWriteError::Io {
        path: path.to_path_buf(),
        reason: err.to_string(),
    }
}

/// The file a write replaces. A symlinked config stays a symlink: the file it
/// points to gets the new text. A missing config is created with its folders.
pub fn resolve_write_target(path: &Path) -> Result<PathBuf, FilingWriteError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => std::fs::canonicalize(path).map_err(io_error(path)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder).map_err(io_error(folder))?;
            }
            Ok(path.to_path_buf())
        }
        Err(err) => Err(io_error(path)(err)),
    }
}

/// The config text as an editable document that keeps its comments and layout.
pub fn parse_document(
    source: &str,
    path: &Path,
) -> Result<toml_edit::DocumentMut, FilingWriteError> {
    source.parse::<toml_edit::DocumentMut>().map_err(|err| {
        FilingWriteError::Config(ConfigError::Invalid {
            path: path.to_path_buf(),
            line: err
                .span()
                .and_then(|span| line_at(source.as_bytes(), span.start)),
            reason: err.message().trim().to_owned(),
        })
    })
}

pub fn not_a_table(path: &Path, key: &str) -> FilingWriteError {
    FilingWriteError::Config(ConfigError::Invalid {
        path: path.to_path_buf(),
        line: None,
        reason: format!("`{key}` is not a table"),
    })
}

/// `source` with `enabled = true` and `locations` in `[modules.filing]`.
pub fn with_filing_locations(
    source: &str,
    path: &Path,
    locations: &[FilingLocation],
) -> Result<String, FilingWriteError> {
    let mut document = parse_document(source, path)?;
    let modules = sub_table(document.as_table_mut(), "modules", true)
        .ok_or_else(|| not_a_table(path, "modules"))?;
    let filing =
        sub_table(modules, "filing", false).ok_or_else(|| not_a_table(path, "modules.filing"))?;
    match filing
        .get_mut("enabled")
        .and_then(toml_edit::Item::as_value_mut)
    {
        Some(enabled) => {
            let decor = enabled.decor().clone();
            *enabled = toml_edit::Value::from(true);
            *enabled.decor_mut() = decor;
        }
        None => {
            filing.insert("enabled", toml_edit::value(true));
        }
    }
    // A comment above the first location belongs to the list, not to the
    // entry that is replaced.
    let first_prefix = filing
        .get("locations")
        .and_then(toml_edit::Item::as_array_of_tables)
        .and_then(|old| old.get(0))
        .and_then(|first| first.decor().prefix().cloned());
    let mut entries = toml_edit::ArrayOfTables::new();
    for location in locations {
        let text = location
            .path
            .to_str()
            .ok_or_else(|| FilingWriteError::PathNotText(location.path.clone()))?;
        let mut entry = toml_edit::Table::new();
        entry.insert("path", toml_edit::value(text));
        let mode = match location.mode {
            LocationMode::Inbox => "inbox",
            LocationMode::Archive => "archive",
        };
        entry.insert("mode", toml_edit::value(mode));
        entry.insert("images", toml_edit::value(location.images));
        if location.is_target {
            entry.insert("target", toml_edit::value(true));
        }
        if entries.is_empty() {
            if let Some(prefix) = first_prefix.clone() {
                entry.decor_mut().set_prefix(prefix);
            }
        }
        entries.push(entry);
    }
    filing.insert("locations", toml_edit::Item::ArrayOfTables(entries));
    Ok(document.to_string())
}

/// The table `key` in `parent`, created when missing. An inline table becomes
/// a regular one, since an array of tables cannot sit inside it. `None` when
/// `key` holds something else, which `load` would have rejected.
pub fn sub_table<'a>(
    parent: &'a mut toml_edit::Table,
    key: &str,
    implicit: bool,
) -> Option<&'a mut toml_edit::Table> {
    let item = parent.entry(key).or_insert_with(|| {
        let mut created = toml_edit::Table::new();
        created.set_implicit(implicit);
        toml_edit::Item::Table(created)
    });
    if item.is_inline_table() {
        if let Ok(table) = std::mem::take(item).into_table() {
            *item = toml_edit::Item::Table(table);
        }
    }
    item.as_table_mut()
}

/// A temporary file next to `target`, renamed over it: a crash leaves the old
/// file or the new one, never half of either.
pub fn replace_file(target: &Path, content: &str) -> Result<(), FilingWriteError> {
    replace_file_atomically(target, ".config.toml.", content)
        .map_err(|(path, err)| io_error(&path)(err))
}
