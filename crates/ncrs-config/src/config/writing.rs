use super::*;

/// A rule a list of filing locations breaks. Indices point into that list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilingRuleViolation {
    InboxAsTarget {
        index: usize,
    },
    SecondTarget {
        index: usize,
    },
    /// There are locations, but none is the target.
    NoTarget,
    /// Same folder, or one inside the other.
    Overlap {
        earlier: usize,
        later: usize,
    },
}

/// The rules every list of locations obeys, in the config and in the setup
/// alike. Paths are compared as they are: the config compares the spelling,
/// the setup canonical paths.
pub fn check_filing_rules(locations: &[FilingLocation]) -> Result<(), FilingRuleViolation> {
    let mut target_seen = false;
    for (index, location) in locations.iter().enumerate() {
        if !location.is_target {
            continue;
        }
        if location.mode == LocationMode::Inbox {
            return Err(FilingRuleViolation::InboxAsTarget { index });
        }
        if target_seen {
            return Err(FilingRuleViolation::SecondTarget { index });
        }
        target_seen = true;
    }
    if !locations.is_empty() && !target_seen {
        return Err(FilingRuleViolation::NoTarget);
    }
    for (later, later_location) in locations.iter().enumerate() {
        let overlapping = locations.iter().take(later).position(|earlier_location| {
            later_location.path.starts_with(&earlier_location.path)
                || earlier_location.path.starts_with(&later_location.path)
        });
        if let Some(earlier) = overlapping {
            return Err(FilingRuleViolation::Overlap { earlier, later });
        }
    }
    Ok(())
}

/// Why the setup's locations were not written. The file is unchanged in
/// every case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilingWriteError {
    /// The choice breaks a rule; an empty choice counts as `NoTarget`.
    Rule(FilingRuleViolation),
    /// TOML has no way to spell a path that is not UTF-8.
    PathNotText(PathBuf),
    /// The file as it is now does not load, or the result would not.
    Config(ConfigError),
    Io {
        path: PathBuf,
        reason: String,
    },
}

/// Turns the filing module on and replaces its locations in the config at
/// `path`, keeping every other line, comment and section as written. Returns
/// the config read back from disk.
pub fn write_filing_locations(
    path: &Path,
    locations: &[FilingLocation],
) -> Result<Config, FilingWriteError> {
    write_filing_locations_with_home(path, locations, home_from_env().as_deref())
}

pub fn write_filing_locations_with_home(
    path: &Path,
    locations: &[FilingLocation],
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    if locations.is_empty() {
        return Err(FilingWriteError::Rule(FilingRuleViolation::NoTarget));
    }
    check_filing_rules(locations).map_err(FilingWriteError::Rule)?;
    write_edited(path, home, |source| {
        with_filing_locations(source, path, locations)
    })
}

/// Replaces the `[[servers]]` entries of the config at `path` with `servers`,
/// keeping every other line, comment and section as written, and returns the
/// config read back from disk. An empty list removes the entries.
pub fn write_servers(path: &Path, servers: &[ServerEntry]) -> Result<Config, FilingWriteError> {
    write_servers_with_home(path, servers, home_from_env().as_deref())
}

pub fn write_servers_with_home(
    path: &Path,
    servers: &[ServerEntry],
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    write_edited(path, home, |source| with_servers(source, path, servers))
}

/// `source` with `servers` as its `[[servers]]` entries.
pub fn with_servers(
    source: &str,
    path: &Path,
    servers: &[ServerEntry],
) -> Result<String, FilingWriteError> {
    let mut document = parse_document(source, path)?;
    // A comment above the first entry belongs to the list, not to the entry.
    let first_prefix = document
        .get("servers")
        .and_then(toml_edit::Item::as_array_of_tables)
        .and_then(|old| old.get(0))
        .and_then(|first| first.decor().prefix().cloned());
    if servers.is_empty() {
        document.remove("servers");
        return Ok(document.to_string());
    }
    let mut entries = toml_edit::ArrayOfTables::new();
    for server in servers {
        let mut entry = toml_edit::Table::new();
        if let Some(name) = &server.name {
            entry.insert("name", toml_edit::value(name.as_str()));
        }
        entry.insert("host", toml_edit::value(server.host.as_str()));
        if server.port != 22 {
            entry.insert("port", toml_edit::value(i64::from(server.port)));
        }
        entry.insert("user", toml_edit::value(server.user.as_str()));
        if let Some(folder) = &server.path {
            entry.insert("path", toml_edit::value(folder.as_str()));
        }
        if entries.is_empty() {
            if let Some(prefix) = first_prefix.clone() {
                entry.decor_mut().set_prefix(prefix);
            }
        }
        entries.push(entry);
    }
    document.insert("servers", toml_edit::Item::ArrayOfTables(entries));
    Ok(document.to_string())
}

/// Replaces the config at `path` with `edit` applied to its text, then reads
/// it back. Both the text before and the text after must load, so nothing
/// the loader would reject reaches the file, and the replacement is atomic.
pub fn write_edited(
    path: &Path,
    home: Option<&Path>,
    edit: impl FnOnce(&str) -> Result<String, FilingWriteError>,
) -> Result<Config, FilingWriteError> {
    let target = resolve_write_target(path)?;
    let source = read_source(&target).map_err(FilingWriteError::Config)?;
    // The edit is based on exactly this text, so a mistake in it would be
    // carried into the new file or, worse, papered over.
    if let Some(text) = &source {
        parse_config(text, path, home).map_err(FilingWriteError::Config)?;
    }
    let edited = edit(source.as_deref().unwrap_or_default())?;
    parse_config(&edited, path, home).map_err(FilingWriteError::Config)?;
    replace_file(&target, &edited)?;
    load_with_home(path, home).map_err(FilingWriteError::Config)
}

/// Sets `vscode` in `[theme]` of the config at `path` to `relative`, a path
/// from the config's folder, keeping every other line, comment and section as
/// written. An existing `vscode` is replaced, `preset` and the role colors stay.
/// Returns the config read back from disk. Only `Config` and `Io` occur.
pub fn write_vscode_theme(path: &Path, relative: &str) -> Result<Config, FilingWriteError> {
    write_vscode_theme_with_home(path, relative, home_from_env().as_deref())
}

pub fn write_vscode_theme_with_home(
    path: &Path,
    relative: &str,
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    write_edited(path, home, |source| {
        with_vscode_theme(source, path, relative)
    })
}

/// `source` with `vscode = "<relative>"` in `[theme]`, which is created when
/// missing.
pub fn with_vscode_theme(
    source: &str,
    path: &Path,
    relative: &str,
) -> Result<String, FilingWriteError> {
    let mut document = parse_document(source, path)?;
    let theme = sub_table(document.as_table_mut(), "theme", false)
        .ok_or_else(|| not_a_table(path, "theme"))?;
    match theme
        .get_mut(VSCODE_KEY)
        .and_then(toml_edit::Item::as_value_mut)
    {
        Some(vscode) => {
            let decor = vscode.decor().clone();
            *vscode = toml_edit::Value::from(relative);
            *vscode.decor_mut() = decor;
        }
        None => {
            theme.insert(VSCODE_KEY, toml_edit::value(relative));
        }
    }
    Ok(document.to_string())
}
