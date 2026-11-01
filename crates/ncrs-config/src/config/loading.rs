use super::*;

/// Reads the config at `path`. A missing file gives the defaults.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    load_with_home(path, home_from_env().as_deref())
}

pub fn load_with_home(path: &Path, home: Option<&Path>) -> Result<Config, ConfigError> {
    match read_source(path)? {
        Some(source) => parse_config(&source, path, home),
        None => Ok(Config::default()),
    }
}

/// The text of the config at `path`, or `None` when there is no file.
pub fn read_source(path: &Path) -> Result<Option<String>, ConfigError> {
    let unreadable = |reason: String| ConfigError::Unreadable {
        path: path.to_path_buf(),
        reason,
    };
    let not_regular = || unreadable("not a regular file".to_owned());
    // Checked on the path before opening: opening a FIFO for reading already
    // blocks until a writer appears, which would hang the start.
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Err(not_regular()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(unreadable(err.to_string())),
    }
    let file = std::fs::File::open(path).map_err(|err| unreadable(err.to_string()))?;
    // Again on the handle, in case the path was swapped in between.
    let meta = file.metadata().map_err(|err| unreadable(err.to_string()))?;
    if !meta.is_file() {
        return Err(not_regular());
    }
    let too_big = || unreadable(format!("larger than {SIZE_LIMIT} bytes"));
    if meta.len() > SIZE_LIMIT {
        return Err(too_big());
    }
    let mut bytes = Vec::new();
    file.take(SIZE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| unreadable(err.to_string()))?;
    if bytes.len() as u64 > SIZE_LIMIT {
        return Err(too_big());
    }
    match String::from_utf8(bytes) {
        Ok(source) => Ok(Some(source)),
        Err(err) => {
            let valid = err.utf8_error().valid_up_to();
            Err(ConfigError::Invalid {
                path: path.to_path_buf(),
                line: line_at(err.as_bytes(), valid),
                reason: "not valid UTF-8".to_owned(),
            })
        }
    }
}

/// The config `source` describes, as if read from `config.toml`, for tests
/// that need a loaded one without a file.
#[cfg(any(test, feature = "test-support"))]
pub fn from_text(source: &str) -> Result<Config, ConfigError> {
    parse_config(source, Path::new("config.toml"), None)
}

/// Every check `load` makes on the text; `path` only names the file in errors.
pub fn parse_config(source: &str, path: &Path, home: Option<&Path>) -> Result<Config, ConfigError> {
    let mut config =
        toml_edit::de::from_str::<Config>(source).map_err(|err| ConfigError::Invalid {
            path: path.to_path_buf(),
            line: err
                .span()
                .and_then(|span| line_at(source.as_bytes(), span.start)),
            reason: err.message().trim().to_owned(),
        })?;
    if let Some(entry) = &config.lang_entry {
        match Language::from_code(entry.get_ref()) {
            Some(language) => config.lang = Some(language),
            None => {
                config.lang = Some(Language::default());
                config.language_warning = Some(LanguageWarning {
                    path: path.to_path_buf(),
                    line: line_at(source.as_bytes(), entry.span().start),
                    code: entry.get_ref().clone(),
                });
            }
        }
    }
    // Not part of the parse: a key that clashes with another is only known
    // once all of `[keys]` is there. It fails the whole file like any other
    // error, so `[open]` is not half applied next to a rejected keymap.
    if !config.keys.entries.is_empty() {
        let keymap = Keymap::with_overrides(&config.keys).map_err(|err| ConfigError::Invalid {
            path: path.to_path_buf(),
            line: line_at(source.as_bytes(), err.span.start),
            reason: err.reason,
        })?;
        config.keymap = Arc::new(keymap);
    }
    if !config.theme.entries.is_empty() {
        let vscode = load_vscode(&config.theme, path, home);
        let configured =
            Palette::from_section(&config.theme, vscode.as_ref().map(|layer| &layer.theme))
                .map_err(|err| ConfigError::Invalid {
                    path: path.to_path_buf(),
                    line: line_at(source.as_bytes(), err.span.start),
                    reason: err.reason,
                })?;
        config.vscode_warning = vscode.and_then(|layer| {
            Some(VscodeWarning {
                path: path.to_path_buf(),
                line: line_at(source.as_bytes(), layer.span.start),
                reason: layer.theme.problem?,
                theme: layer.file,
            })
        });
        config.contrast_warning = configured.palette.weakest_contrast().map(|weak| {
            let found = configured
                .span_of(weak.foreground)
                .or_else(|| configured.span_of(weak.background))
                .or_else(|| configured.vscode_span.clone())
                .or_else(|| configured.preset_span.clone());
            ContrastWarning {
                path: path.to_path_buf(),
                line: found.and_then(|span| line_at(source.as_bytes(), span.start)),
                foreground: weak.foreground,
                background: weak.background,
                ratio: weak.ratio,
            }
        });
        config.palette = configured.palette;
    }
    config.filing =
        resolve_filing(&config.modules.filing, home).map_err(|err| ConfigError::Invalid {
            path: path.to_path_buf(),
            line: line_at(source.as_bytes(), err.span.start),
            reason: err.reason,
        })?;
    config.file_list_tags = resolve_file_list_tags(&config.file_list_tags_section);
    config.servers =
        resolve_servers(&config.server_sections).map_err(|err| ConfigError::Invalid {
            path: path.to_path_buf(),
            line: line_at(source.as_bytes(), err.span.start),
            reason: err.reason,
        })?;
    let (kinds, skips) = resolve_kinds(&config.kind_sections);
    config.kinds = kinds;
    config.kind_warning = (!skips.is_empty()).then(|| KindWarning {
        path: path.to_path_buf(),
        skips,
    });
    Ok(config)
}

/// The built-in kinds with the `[[kinds]]` entries applied; an entry without
/// an ID, and a word that names two kinds, are left out and returned.
pub fn resolve_kinds(sections: &[KindSection]) -> (Arc<KindCatalog>, Vec<KindSkip>) {
    let custom: Vec<CustomKind> = sections
        .iter()
        .map(|section| CustomKind {
            id: section.id.clone(),
            german: section.de.clone(),
            english: section.en.clone(),
            words: section.words.clone(),
        })
        .collect();
    let (catalog, skips) = KindCatalog::built_in().extended(&custom);
    (Arc::new(catalog), skips)
}

pub fn resolve_file_list_tags(section: &FileListTagsSection) -> FileListTags {
    let defaults = FileListTags::default();
    let groups = section.groups.as_ref().map_or(defaults.groups, |names| {
        TagGroup::ALL
            .into_iter()
            .filter(|group| names.iter().any(|name| name.trim() == group.name()))
            .collect()
    });
    FileListTags {
        show: section.show.unwrap_or(defaults.show),
        groups,
    }
}

/// Checks the `[[servers]]` entries as text; nothing is looked up or opened.
pub fn resolve_servers(sections: &[ServerSection]) -> Result<Vec<ServerEntry>, FilingError> {
    sections
        .iter()
        .map(|section| {
            let word = |value: &Spanned<String>, what: &str, forbidden: &[char]| {
                let text = value.get_ref();
                let bad = text.is_empty()
                    || text
                        .chars()
                        .any(|c| c.is_whitespace() || c.is_control() || forbidden.contains(&c));
                if bad {
                    return Err(FilingError {
                        span: value.span(),
                        reason: format!(
                            "{what} must not be empty and must not contain spaces or {}",
                            forbidden
                                .iter()
                                .map(|c| format!("`{c}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    });
                }
                Ok(text.clone())
            };
            let host = word(&section.host, "host", &['@', '/'])?;
            let user = word(&section.user, "user", &['@', '/', ':'])?;
            let port = match &section.port {
                None => 22,
                Some(port) => u16::try_from(*port.get_ref())
                    .ok()
                    .filter(|number| *number != 0)
                    .ok_or_else(|| FilingError {
                        span: port.span(),
                        reason: "port must be a number from 1 to 65535".to_owned(),
                    })?,
            };
            let path = match &section.path {
                None => None,
                Some(path) if path.get_ref().starts_with('/') => Some(path.get_ref().clone()),
                Some(path) => {
                    return Err(FilingError {
                        span: path.span(),
                        reason: "path must be absolute: it starts with `/`".to_owned(),
                    })
                }
            };
            Ok(ServerEntry {
                name: section
                    .name
                    .as_ref()
                    .map(|name| name.get_ref().trim().to_owned())
                    .filter(|name| !name.is_empty()),
                host,
                port,
                user,
                path,
            })
        })
        .collect()
}

/// The VS Code theme `[theme]` names, read, with where it was named.
pub struct VscodeLayer {
    theme: VscodeTheme,
    span: Range<usize>,
    file: PathBuf,
}

/// Reads the file named by `vscode`. It never fails the config: whatever is
/// wrong with the file ends up in `theme.problem`.
pub fn load_vscode(
    section: &ThemeSection,
    config_path: &Path,
    home: Option<&Path>,
) -> Option<VscodeLayer> {
    let entry = section
        .entries
        .iter()
        .rev()
        .find(|entry| entry.key.as_ref() == VSCODE_KEY)?;
    let text = entry.value.as_ref();
    let resolved = if text.starts_with('~') {
        expand_location_path(text, home)
    } else {
        Ok(config_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(text))
    };
    let (theme, file) = match resolved {
        Ok(file) => (VscodeTheme::load(&file), file),
        Err(reason) => (VscodeTheme::unusable(reason), PathBuf::from(text)),
    };
    Some(VscodeLayer {
        theme,
        span: entry.value.span(),
        file,
    })
}
