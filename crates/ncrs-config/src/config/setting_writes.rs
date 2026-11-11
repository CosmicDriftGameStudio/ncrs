use super::*;

/// One value the settings page changes: the tables it sits in, its key and
/// what it becomes, `None` to drop its line. Only the `*_setting` functions
/// below build one; what they cannot rule out the loader catches before the
/// file is replaced.
pub struct Setting {
    tables: &'static [&'static str],
    key: &'static str,
    value: Option<toml_edit::Value>,
}

pub const THEME_TABLE: &str = "theme";
pub const MODULES_TABLE: &str = "modules";
pub const FILING_TABLE: &str = "filing";
pub const AI_TABLE: &str = "ai";
pub const OPEN_TABLE: &str = "open";
pub const FILE_LIST_TAGS_TABLE: &str = "file_list_tags";
pub const SYSTEM_TABLE: &str = "system";
pub const COLUMNS_TABLE: &str = "columns";
pub const APPEARANCE_TABLE: &str = "appearance";
pub const AI_TABLES: &[&str] = &[MODULES_TABLE, FILING_TABLE, AI_TABLE];

pub fn theme_preset_setting(preset: ThemePreset) -> Setting {
    Setting {
        tables: &[THEME_TABLE],
        key: PRESET_KEY,
        value: Some(preset.name().into()),
    }
}

pub fn language_setting(language: Language) -> Setting {
    Setting {
        tables: &[],
        key: "lang",
        value: Some(language.code().into()),
    }
}

pub fn check_for_updates_setting(enabled: bool) -> Setting {
    Setting {
        tables: &[],
        key: "check_for_updates",
        value: Some(enabled.into()),
    }
}

/// Sets the top-level `check_for_updates`; same guarantees as
/// [`write_theme_preset`].
pub fn write_check_for_updates(path: &Path, enabled: bool) -> Result<Config, FilingWriteError> {
    write_setting(
        path,
        check_for_updates_setting(enabled),
        home_from_env().as_deref(),
    )
}

pub fn favourites_bar_setting(shown: bool) -> Setting {
    Setting {
        tables: &[],
        key: "favourites_bar",
        value: Some(shown.into()),
    }
}

/// Sets the top-level `favourites_bar`; same guarantees as
/// [`write_theme_preset`].
pub fn write_favourites_bar(path: &Path, shown: bool) -> Result<Config, FilingWriteError> {
    write_setting(
        path,
        favourites_bar_setting(shown),
        home_from_env().as_deref(),
    )
}

pub fn filing_enabled_setting(enabled: bool) -> Setting {
    Setting {
        tables: &[MODULES_TABLE, FILING_TABLE],
        key: "enabled",
        value: Some(enabled.into()),
    }
}

pub fn filing_load_setting(load: Load) -> Setting {
    let value = match load {
        Load::Gentle => "gentle".into(),
        Load::Balanced => "balanced".into(),
        Load::Full => "full".into(),
        Load::Workers(count) => i64::try_from(count.get()).unwrap_or(i64::MAX).into(),
    };
    Setting {
        tables: &[MODULES_TABLE, FILING_TABLE],
        key: "load",
        value: Some(value),
    }
}

pub fn default_pattern_setting(pattern: Option<&str>) -> Setting {
    Setting {
        tables: &[MODULES_TABLE, FILING_TABLE],
        key: "default_pattern",
        value: pattern.map(toml_edit::Value::from),
    }
}

/// The lists of what is the user's own in `[modules.filing]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OwnDetail {
    Names,
    Ibans,
    TaxIds,
}

impl OwnDetail {
    /// Whether the loader keeps `entry` (trimmed): only an IBAN is checked.
    pub fn accepts(self, entry: &str) -> bool {
        match self {
            Self::Ibans => iban_is_valid(entry),
            Self::Names | Self::TaxIds => true,
        }
    }

    /// The entries the file gives for this list, as the loader keeps them.
    pub fn entries_in(self, section: &FilingSection) -> Vec<String> {
        let listed = match self {
            Self::Names => &section.own_names,
            Self::Ibans => &section.own_ibans,
            Self::TaxIds => &section.own_tax_ids,
        };
        own_entries(listed, |entry| self.accepts(entry))
    }

    fn key(self) -> &'static str {
        match self {
            Self::Names => "own_names",
            Self::Ibans => "own_ibans",
            Self::TaxIds => "own_tax_ids",
        }
    }
}

pub fn own_detail_setting(detail: OwnDetail, entries: &[String]) -> Setting {
    Setting {
        tables: &[MODULES_TABLE, FILING_TABLE],
        key: detail.key(),
        value: (!entries.is_empty())
            .then(|| toml_edit::Value::Array(entries.iter().map(String::as_str).collect())),
    }
}

pub fn file_list_tags_settings(tags: &FileListTags) -> [Setting; 3] {
    let list = |entries: Vec<&str>| toml_edit::Value::Array(entries.into_iter().collect());
    [
        Setting {
            tables: &[FILE_LIST_TAGS_TABLE],
            key: "show",
            value: Some(tags.show.into()),
        },
        Setting {
            tables: &[FILE_LIST_TAGS_TABLE],
            key: "groups",
            value: Some(list(tags.groups.iter().map(|group| group.name()).collect())),
        },
        // Older files keep tag names here; the next write takes them out.
        Setting {
            tables: &[FILE_LIST_TAGS_TABLE],
            key: "only",
            value: None,
        },
    ]
}

pub fn system_settings(settings: &SystemSettings) -> [Setting; 8] {
    let list = |entries: Vec<&str>| toml_edit::Value::Array(entries.into_iter().collect());
    let setting = |key: &'static str, value: toml_edit::Value| Setting {
        tables: &[SYSTEM_TABLE],
        key,
        value: Some(value),
    };
    [
        setting("refresh", settings.refresh.name().into()),
        setting("sort", settings.sort.name().into()),
        setting("network_unit", settings.network_unit.name().into()),
        setting("drive_column", settings.drive_column.name().into()),
        setting(
            "status_values",
            list(
                settings
                    .status_values
                    .iter()
                    .map(|value| value.name())
                    .collect(),
            ),
        ),
        setting("status_style", settings.status_style.name().into()),
        setting("idle_interfaces", settings.idle_interfaces.name().into()),
        setting("system_volumes", settings.system_volumes.name().into()),
    ]
}

/// The whole `[columns]` section. No widths drops the `widths` line.
pub fn columns_settings(columns: &PanelColumns) -> [Setting; 6] {
    let list = |entries: &[Column]| {
        toml_edit::Value::Array(entries.iter().map(|column| column.key()).collect())
    };
    let setting = |key: &'static str, value: Option<toml_edit::Value>| Setting {
        tables: &[COLUMNS_TABLE],
        key,
        value,
    };
    let widths = (!columns.widths.is_empty()).then(|| {
        let mut table = toml_edit::InlineTable::new();
        for (column, pixels) in &columns.widths {
            table.insert(column.key(), i64::from(*pixels).into());
        }
        toml_edit::Value::InlineTable(table)
    });
    [
        setting("same_in_both", Some(columns.same_in_both.into())),
        setting("left", Some(list(&columns.left))),
        setting("right", Some(list(&columns.right))),
        setting("widths", widths),
        setting("date_format", Some(columns.date_format.name().into())),
        setting("size_format", Some(columns.size_format.name().into())),
    ]
}

/// The whole `[appearance]` section; the system's choice drops its line.
pub fn appearance_settings(appearance: &Appearance) -> [Setting; 2] {
    let font = match &appearance.font {
        FontChoice::System => None,
        FontChoice::Named(name) => Some(name.as_str().into()),
    };
    let font_size = match appearance.font_size {
        FontSizeChoice::System => None,
        FontSizeChoice::Points(points) => Some(i64::from(points).into()),
    };
    [
        Setting {
            tables: &[APPEARANCE_TABLE],
            key: "font",
            value: font,
        },
        Setting {
            tables: &[APPEARANCE_TABLE],
            key: "font_size",
            value: font_size,
        },
    ]
}

pub fn ai_enabled_setting(enabled: bool) -> Setting {
    Setting {
        tables: AI_TABLES,
        key: "enabled",
        value: Some(enabled.into()),
    }
}

/// Sets `preset` in `[theme]` of the config at `path`, keeping every other
/// line, comment and section as written. Returns the config read back.
pub fn write_theme_preset(path: &Path, preset: ThemePreset) -> Result<Config, FilingWriteError> {
    write_setting(
        path,
        theme_preset_setting(preset),
        home_from_env().as_deref(),
    )
}

/// Sets the top-level `lang`; same guarantees as [`write_theme_preset`].
pub fn write_language(path: &Path, language: Language) -> Result<Config, FilingWriteError> {
    write_setting(path, language_setting(language), home_from_env().as_deref())
}

/// Sets `enabled` in `[modules.filing]`; same guarantees as [`write_theme_preset`].
pub fn write_filing_enabled(path: &Path, enabled: bool) -> Result<Config, FilingWriteError> {
    write_setting(
        path,
        filing_enabled_setting(enabled),
        home_from_env().as_deref(),
    )
}

/// Sets `load` in `[modules.filing]` and drops the old `threads` line of
/// `[modules.filing.ai]`, so the load alone decides the workers and the model
/// threads; same guarantees as [`write_theme_preset`].
pub fn write_filing_load(path: &Path, load: Load) -> Result<Config, FilingWriteError> {
    write_settings(
        path,
        &[filing_load_setting(load), ai_threads_dropped_setting()],
        home_from_env().as_deref(),
    )
}

pub fn ai_threads_dropped_setting() -> Setting {
    Setting {
        tables: AI_TABLES,
        key: "threads",
        value: None,
    }
}

/// Sets `default_pattern` in `[modules.filing]`, or drops the line for
/// `None`; same guarantees as [`write_theme_preset`]. The loader ignores a
/// pattern Hermes cannot render, so a caller checks it with `pattern_is_valid`.
pub fn write_default_pattern(
    path: &Path,
    pattern: Option<&str>,
) -> Result<Config, FilingWriteError> {
    write_setting(
        path,
        default_pattern_setting(pattern.map(str::trim)),
        home_from_env().as_deref(),
    )
}

/// Sets one list of the user's own details in `[modules.filing]`, or drops the
/// line for an empty list; same guarantees as [`write_theme_preset`]. The
/// loader drops what is not valid, so a caller checks an IBAN first.
pub fn write_own_details(
    path: &Path,
    detail: OwnDetail,
    entries: &[String],
) -> Result<Config, FilingWriteError> {
    write_setting(
        path,
        own_detail_setting(detail, entries),
        home_from_env().as_deref(),
    )
}

/// Writes the whole `[file_list_tags]` section; same guarantees as
/// [`write_theme_preset`].
pub fn write_file_list_tags(path: &Path, tags: &FileListTags) -> Result<Config, FilingWriteError> {
    write_settings(
        path,
        &file_list_tags_settings(tags),
        home_from_env().as_deref(),
    )
}

/// Writes the whole `[system]` section; same guarantees as
/// [`write_theme_preset`].
pub fn write_system_settings(
    path: &Path,
    settings: &SystemSettings,
) -> Result<Config, FilingWriteError> {
    write_settings(path, &system_settings(settings), home_from_env().as_deref())
}

/// Writes the whole `[columns]` section; same guarantees as
/// [`write_theme_preset`].
pub fn write_columns(path: &Path, columns: &PanelColumns) -> Result<Config, FilingWriteError> {
    write_settings(path, &columns_settings(columns), home_from_env().as_deref())
}

/// Writes the whole `[appearance]` section; same guarantees as
/// [`write_theme_preset`].
pub fn write_appearance(path: &Path, appearance: &Appearance) -> Result<Config, FilingWriteError> {
    write_settings(
        path,
        &appearance_settings(appearance),
        home_from_env().as_deref(),
    )
}

/// Sets `enabled` in `[modules.filing.ai]`; same guarantees as [`write_theme_preset`].
pub fn write_ai_enabled(path: &Path, enabled: bool) -> Result<Config, FilingWriteError> {
    write_setting(
        path,
        ai_enabled_setting(enabled),
        home_from_env().as_deref(),
    )
}

pub fn program_setting(kind: OpenKind, line: Option<&ProgramLine>) -> Setting {
    Setting {
        tables: &[OPEN_TABLE],
        key: match kind {
            OpenKind::View => "view",
            OpenKind::Edit => "edit",
            OpenKind::Terminal => "terminal",
        },
        value: line.map(|words| toml_edit::Value::Array(words.words().collect())),
    }
}

pub fn vscode_theme_setting(file: Option<&str>) -> Setting {
    Setting {
        tables: &[THEME_TABLE],
        key: VSCODE_KEY,
        value: file.map(toml_edit::Value::from),
    }
}

pub fn color_setting(role: ColorRole, color: Option<&str>) -> Setting {
    Setting {
        tables: &[THEME_TABLE],
        key: role.name(),
        value: color.map(toml_edit::Value::from),
    }
}

/// The provider and the keys that belong to it; the keys of the other
/// provider are dropped, since the loader refuses them next to this one.
/// The consented servers stay when the provider goes back to the local one,
/// so that a later switch to the same server does not ask again.
pub fn ai_provider_settings(choice: &AiProviderChoice) -> Vec<Setting> {
    let (provider, threads, endpoint, model, consented) = match choice {
        AiProviderChoice::Local { threads } => (
            AiProviderName::Local,
            threads.map(|count| NonZeroI64::from(count).get().into()),
            None,
            None,
            None,
        ),
        AiProviderChoice::Http {
            endpoint,
            model,
            consented_origins,
        } => (
            AiProviderName::Http,
            None,
            Some(endpoint.as_str().into()),
            Some(model.as_str().into()),
            Some(toml_edit::Value::Array(
                consented_origins.iter().map(String::as_str).collect(),
            )),
        ),
    };
    let setting = |key, value| Setting {
        tables: AI_TABLES,
        key,
        value,
    };
    let mut settings = vec![
        setting("provider", Some(provider_text(provider).into())),
        setting("threads", threads),
        setting("endpoint", endpoint),
        setting("model", model),
    ];
    if consented.is_some() {
        settings.push(setting("consented_origins", consented));
    }
    settings
}

/// What the settings tab sets in `[modules.filing.ai]` besides `enabled`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiProviderChoice {
    /// `None` leaves the thread count to the module.
    Local { threads: Option<NonZeroU32> },
    Http {
        endpoint: String,
        model: String,
        /// Every server documents may go to, the new one included.
        consented_origins: Vec<String>,
    },
}

/// Sets `view`, `edit` or `terminal` in `[open]`, `None` drops it so that the system
/// default opens the file; same guarantees as [`write_theme_preset`].
pub fn write_program(
    path: &Path,
    kind: OpenKind,
    line: Option<&ProgramLine>,
) -> Result<Config, FilingWriteError> {
    write_setting(
        path,
        program_setting(kind, line),
        home_from_env().as_deref(),
    )
}

/// Drops `vscode` from `[theme]`; same guarantees as [`write_theme_preset`].
pub fn remove_vscode_theme(path: &Path) -> Result<Config, FilingWriteError> {
    write_setting(path, vscode_theme_setting(None), home_from_env().as_deref())
}

/// Sets the color of `role` in `[theme]`, `None` drops it so that the color
/// scheme decides; same guarantees as [`write_theme_preset`].
pub fn write_color(
    path: &Path,
    role: ColorRole,
    color: Option<&str>,
) -> Result<Config, FilingWriteError> {
    write_setting(path, color_setting(role, color), home_from_env().as_deref())
}

/// Sets the provider in `[modules.filing.ai]` with its keys; same guarantees
/// as [`write_theme_preset`].
pub fn write_ai_provider(
    path: &Path,
    choice: &AiProviderChoice,
) -> Result<Config, FilingWriteError> {
    write_settings(
        path,
        &ai_provider_settings(choice),
        home_from_env().as_deref(),
    )
}

pub fn write_setting(
    path: &Path,
    setting: Setting,
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    write_settings(path, &[setting], home)
}

pub fn write_settings(
    path: &Path,
    settings: &[Setting],
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    write_edited(path, home, |source| with_settings(source, path, settings))
}

/// What one action gets in `[keys]`: the listed keys as `[keys]` spells
/// them, or `None` to drop its line, so that it has its built-in keys again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChange {
    pub action: KeyAction,
    pub chords: Option<Vec<String>>,
}

pub const KEYS_TABLE: &str = "keys";

/// Writes the lines of `changes` to `[keys]` and leaves every other action,
/// line and comment as written. The result must load, so a clash or a key
/// the keymap refuses leaves the file unchanged and comes back as `Config`.
pub fn write_keys(path: &Path, changes: &[KeyChange]) -> Result<Config, FilingWriteError> {
    write_keys_with_home(path, changes, home_from_env().as_deref())
}

pub fn write_keys_with_home(
    path: &Path,
    changes: &[KeyChange],
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    write_edited(path, home, |source| with_keys(source, path, changes))
}

/// `source` with the lines of `changes` in `[keys]`. The table is created
/// only for a line to write: dropping lines from a file without one leaves
/// it as it is.
pub fn with_keys(
    source: &str,
    path: &Path,
    changes: &[KeyChange],
) -> Result<String, FilingWriteError> {
    let mut document = parse_document(source, path)?;
    let writes_a_line = changes.iter().any(|change| change.chords.is_some());
    if !writes_a_line && !document.contains_key(KEYS_TABLE) {
        return Ok(document.to_string());
    }
    let keys = sub_table(document.as_table_mut(), KEYS_TABLE, false)
        .ok_or_else(|| not_a_table(path, KEYS_TABLE))?;
    for change in changes {
        let name = change.action.name();
        let Some(chords) = &change.chords else {
            keys.remove(name);
            continue;
        };
        let value = toml_edit::Value::Array(chords.iter().map(String::as_str).collect());
        match keys.get_mut(name).and_then(toml_edit::Item::as_value_mut) {
            Some(existing) => {
                let decor = existing.decor().clone();
                *existing = value;
                *existing.decor_mut() = decor;
            }
            None => {
                keys.insert(name, toml_edit::Item::Value(value));
            }
        }
    }
    Ok(document.to_string())
}

/// `source` with the settings in place, in order.
pub fn with_settings(
    source: &str,
    path: &Path,
    settings: &[Setting],
) -> Result<String, FilingWriteError> {
    let mut document = parse_document(source, path)?;
    for setting in settings {
        put_setting(document.as_table_mut(), path, setting)?;
    }
    Ok(document.to_string())
}

/// A value goes in place, its tables created when missing; an existing one
/// keeps the comment on its line. A dropped value creates no table.
pub fn put_setting(
    root: &mut toml_edit::Table,
    path: &Path,
    setting: &Setting,
) -> Result<(), FilingWriteError> {
    let Some(value) = &setting.value else {
        let found = setting
            .tables
            .iter()
            .try_fold(root as &mut dyn toml_edit::TableLike, |table, name| {
                table.get_mut(name)?.as_table_like_mut()
            });
        if let Some(table) = found {
            table.remove(setting.key);
        }
        return Ok(());
    };
    let mut table = root;
    for (depth, name) in setting.tables.iter().enumerate() {
        let implicit = depth + 1 < setting.tables.len();
        table = sub_table(table, name, implicit).ok_or_else(|| not_a_table(path, name))?;
    }
    match table
        .get_mut(setting.key)
        .and_then(toml_edit::Item::as_value_mut)
    {
        Some(existing) => {
            let decor = existing.decor().clone();
            *existing = value.clone();
            *existing.decor_mut() = decor;
        }
        None => {
            table.insert(setting.key, toml_edit::Item::Value(value.clone()));
        }
    }
    Ok(())
}
