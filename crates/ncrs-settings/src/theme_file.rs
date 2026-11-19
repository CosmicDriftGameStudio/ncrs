//! A theme file the user picked.

use std::path::{Component, Path};

use ncrs_config::config::FilingWriteError;

/// What `[theme] vscode` says for a picked theme file. One inside the
/// config's folder is named from there, as the downloaded ones are, so that
/// the folder can move with it. `None` when the path is not text.
pub fn theme_file_entry(config_path: Option<&Path>, picked: &Path) -> Option<String> {
    let named = config_path
        .and_then(Path::parent)
        .and_then(|folder| picked.strip_prefix(folder).ok())
        .unwrap_or(picked);
    named.to_str().map(str::to_owned)
}

/// The name `file_name` gets in `folder`: its own when nothing is there or the
/// file there already has the same content, otherwise the first free
/// "name (2).ext", so a theme that was loaded or picked earlier is not
/// replaced by a different one of the same name.
pub fn name_in_folder(
    folder: &Path,
    file_name: &str,
    same_content: impl Fn(&Path) -> bool,
) -> String {
    let free = |name: &str| {
        let existing = folder.join(name);
        !existing.exists() || same_content(&existing)
    };
    if free(file_name) {
        return file_name.to_owned();
    }
    let (stem, extension) = match file_name.rsplit_once('.') {
        Some((stem, extension)) => (stem, format!(".{extension}")),
        None => (file_name, String::new()),
    };
    (2..1000)
        .map(|number| format!("{stem} ({number}){extension}"))
        .find(|name| free(name))
        .unwrap_or_else(|| file_name.to_owned())
}

/// Whether the file at `path` holds exactly `bytes`.
pub fn holds_bytes(path: &Path, bytes: &[u8]) -> bool {
    std::fs::read(path).is_ok_and(|held| held == bytes)
}

/// The folder below the config's, next to the downloaded themes, where the
/// sandbox keeps the theme file a user picked.
const PICKED_THEMES_FOLDER: &str = "themes/picked";

/// What came of copying a picked theme file.
#[derive(Debug)]
pub struct PickedThemeCopy {
    /// The entry for `[theme] vscode`.
    pub entry: String,
    pub include_not_copied: Option<IncludeNotCopied>,
}

#[derive(Debug)]
pub struct IncludeNotCopied {
    pub name: String,
    pub reason: IncludeProblem,
}

#[derive(Debug)]
pub enum IncludeProblem {
    OutsideFolder,
    Unreadable(String),
}

/// An `include` is copied only when it names a file at or below the theme's
/// own folder: no `..`, no absolute path, no drive.
fn include_stays_in_folder(name: &str) -> bool {
    !name.is_empty()
        && !name.split(['/', '\\']).any(|part| part == "..")
        && Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
        && !name.starts_with(['/', '\\'])
}

fn copy_included_theme_file(picked: &Path, folder: &Path, name: &str) -> Result<(), String> {
    let source = picked.parent().unwrap_or_else(|| Path::new("")).join(name);
    let target = folder.join(name);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::copy(source, target)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Copies a picked theme file below the config's folder and returns the entry
/// for `[theme] vscode`. The sandbox grants the file only until the app quits,
/// so a path outside the container would be unreadable at the next start. The
/// one file it `include`s is copied along when it lies below the theme's
/// folder and is readable.
pub fn copy_picked_theme_file(
    config_path: &Path,
    picked: &Path,
) -> Result<PickedThemeCopy, FilingWriteError> {
    let io_failure = |failed: &Path| {
        let owned = failed.to_path_buf();
        move |error: std::io::Error| FilingWriteError::Io {
            path: owned,
            reason: error.to_string(),
        }
    };
    let file_name = picked.file_name().ok_or_else(|| FilingWriteError::Io {
        path: picked.to_path_buf(),
        reason: "not a file".to_owned(),
    })?;
    let folder = config_path
        .parent()
        .ok_or_else(|| FilingWriteError::Io {
            path: config_path.to_path_buf(),
            reason: "no folder".to_owned(),
        })?
        .join(PICKED_THEMES_FOLDER);
    std::fs::create_dir_all(&folder).map_err(io_failure(&folder))?;
    let picked_bytes = std::fs::read(picked).map_err(io_failure(picked))?;
    let stored_name = name_in_folder(&folder, &file_name.to_string_lossy(), |existing| {
        holds_bytes(existing, &picked_bytes)
    });
    let target = folder.join(&stored_name);
    std::fs::write(&target, &picked_bytes).map_err(io_failure(&target))?;
    let include_not_copied = match ncrs_config::vscode_theme::included_name(picked) {
        Ok(Some(name)) if !include_stays_in_folder(&name) => Some(IncludeNotCopied {
            name,
            reason: IncludeProblem::OutsideFolder,
        }),
        Ok(Some(name)) => copy_included_theme_file(picked, &folder, &name)
            .err()
            .map(|reason| IncludeNotCopied {
                name,
                reason: IncludeProblem::Unreadable(reason),
            }),
        Ok(None) | Err(_) => None,
    };
    Ok(PickedThemeCopy {
        entry: format!("{PICKED_THEMES_FOLDER}/{stored_name}"),
        include_not_copied,
    })
}
