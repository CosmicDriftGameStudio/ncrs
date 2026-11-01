//! Reading a downloaded extension package (a `.vsix`, which is a ZIP archive)
//! in memory, and putting the color theme it carries on disk.
//!
//! The package comes from the internet, so nothing in it is trusted: entries
//! are looked up by name and never extracted, every read has a size limit that
//! does not depend on what the archive claims, and every path the extension
//! names is checked here before it is joined to a folder.

use std::io::{Cursor, Read as _};
use std::path::Path;

use jsonc_parser::{parse_to_value, JsonObject, JsonValue, ParseOptions};
use zip::result::ZipError;
use zip::ZipArchive;

use crate::open_vsx::{shown_text, ExtensionId, FetchError, MAX_SHOWN_CHARS, MAX_VSIX_BYTES};
use ncrs_config::config::{self, Config, SIZE_LIMIT};
use ncrs_config::vscode_theme::VscodeTheme;

const PACKAGE_JSON: &str = "extension/package.json";
const LICENSE_LIMIT: u64 = 256 * 1024;
const LICENSE_NAMES: [&str; 6] = [
    "LICENSE",
    "LICENSE.txt",
    "LICENSE.md",
    "license",
    "license.txt",
    "license.md",
];

/// The folder next to the config file that holds the installed themes.
const THEMES_FOLDER: &str = "themes";
/// Below `themes/`, the theme files that came from a link.
const WEB_THEMES_FOLDER: &str = "web";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Brightness {
    Dark,
    Light,
}

/// One color theme an extension contributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributedTheme {
    pub label: String,
    /// `None` when the extension does not say, or says something unknown.
    pub brightness: Option<Brightness>,
    /// The theme file in the package, normalized, below `extension/`.
    pub path: String,
}

/// `raw` as a relative path with `/` and no empty or `.` parts, or `None` when
/// it could leave its folder: a leading separator, a `..`, a drive or stream
/// (`:`) or a control character. Both separators count, since the package
/// comes from any system.
pub fn normalize_path(raw: &str) -> Option<String> {
    if raw.starts_with(['/', '\\']) || raw.chars().any(char::is_control) {
        return None;
    }
    let mut kept = Vec::new();
    for part in raw.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return None,
            named if named.contains(':') => return None,
            named => kept.push(named),
        }
    }
    (!kept.is_empty()).then(|| kept.join("/"))
}

fn checked_path(raw: &str) -> Result<String, FetchError> {
    normalize_path(raw).ok_or_else(|| FetchError::UnsafePath(raw.to_owned()))
}

/// Where `include` of the theme at `theme_path` (normalized) points. It is
/// checked on its own first, so `../x.json` is refused even when the theme
/// sits in a subfolder.
fn resolve_include(theme_path: &str, include: &str) -> Result<String, FetchError> {
    let relative = checked_path(include)?;
    Ok(match theme_path.rsplit_once('/') {
        Some((folder, _)) => format!("{folder}/{relative}"),
        None => relative,
    })
}

type Archive<'a> = ZipArchive<Cursor<&'a [u8]>>;

fn open(vsix: &[u8]) -> Result<Archive<'_>, FetchError> {
    if vsix.len() as u64 > MAX_VSIX_BYTES {
        return Err(FetchError::TooLarge);
    }
    ZipArchive::new(Cursor::new(vsix)).map_err(|err| FetchError::BadPackage(err.to_string()))
}

/// The entry `name`, or `None` when the package has none. More than `limit`
/// bytes is an error; what the archive says about the size is not used.
fn read_entry(
    archive: &mut Archive<'_>,
    name: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, FetchError> {
    let entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(FetchError::BadPackage(err.to_string())),
    };
    let mut bytes = Vec::new();
    entry
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| FetchError::BadPackage(format!("{name}: {err}")))?;
    if bytes.len() as u64 > limit {
        return Err(FetchError::FileTooLarge(name.to_owned()));
    }
    Ok(Some(bytes))
}

fn required_entry(
    archive: &mut Archive<'_>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, FetchError> {
    read_entry(archive, name, limit)?.ok_or_else(|| {
        FetchError::MissingFile(name.strip_prefix("extension/").unwrap_or(name).to_owned())
    })
}

/// Parses `bytes` as a JSON object (comments and trailing commas allowed) and
/// hands it to `read`. `file` names it in errors.
fn with_json_object<T>(
    bytes: &[u8],
    file: &str,
    read: impl FnOnce(JsonObject<'_>) -> T,
) -> Result<T, FetchError> {
    let bad = |reason: String| FetchError::BadJson {
        file: file.to_owned(),
        reason,
    };
    let text = std::str::from_utf8(bytes).map_err(|_not_utf8| bad("not valid UTF-8".to_owned()))?;
    match parse_to_value(text, &ParseOptions::default()) {
        Ok(Some(JsonValue::Object(object))) => Ok(read(object)),
        Ok(_) => Err(bad("not a JSON object".to_owned())),
        Err(err) => Err(bad(err.to_string())),
    }
}

fn brightness_of(ui_theme: &str) -> Option<Brightness> {
    match ui_theme {
        "vs-dark" | "hc-black" => Some(Brightness::Dark),
        "vs" | "hc-light" => Some(Brightness::Light),
        _ => None,
    }
}

/// The themes `contributes.themes` of the manifest lists. One that names a
/// path outside the package folder fails the whole package.
pub fn list_themes(vsix: &[u8]) -> Result<Vec<ContributedTheme>, FetchError> {
    let mut archive = open(vsix)?;
    let manifest = required_entry(&mut archive, PACKAGE_JSON, SIZE_LIMIT)?;
    let listed = with_json_object(&manifest, PACKAGE_JSON, |mut root| {
        let mut themes = root
            .take_object("contributes")
            .and_then(|mut contributes| contributes.take_array("themes"))
            .map(|array| array.take_inner())
            .unwrap_or_default();
        themes
            .drain(..)
            .filter_map(|theme| match theme {
                JsonValue::Object(mut object) => {
                    let path = object.take_string("path")?.into_owned();
                    let label = object
                        .take_string("label")
                        .map(|label| shown_text(&label, MAX_SHOWN_CHARS));
                    let ui_theme = object.take_string("uiTheme").map(|ui| ui.into_owned());
                    Some((path, label, ui_theme))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    })?;
    let mut themes = Vec::new();
    for (raw_path, label, ui_theme) in listed {
        let path = checked_path(&raw_path)?;
        themes.push(ContributedTheme {
            label: label
                .filter(|named| !named.is_empty())
                .unwrap_or_else(|| path.clone()),
            brightness: ui_theme.as_deref().and_then(brightness_of),
            path,
        });
    }
    if themes.is_empty() {
        return Err(FetchError::NoThemes);
    }
    Ok(themes)
}

/// A file of the package as it goes to disk: its path relative to the folder
/// of the extension, and its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// What an installed theme consists of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeFiles {
    pub theme: PackageFile,
    pub include: Option<PackageFile>,
    pub license: Option<PackageFile>,
}

impl ThemeFiles {
    fn all(&self) -> impl Iterator<Item = &PackageFile> {
        std::iter::once(&self.theme)
            .chain(self.include.as_ref())
            .chain(self.license.as_ref())
    }
}

fn package_file(
    archive: &mut Archive<'_>,
    path: String,
    limit: u64,
) -> Result<PackageFile, FetchError> {
    let bytes = required_entry(archive, &format!("extension/{path}"), limit)?;
    Ok(PackageFile { path, bytes })
}

fn read_theme_files_limited(
    vsix: &[u8],
    theme: &ContributedTheme,
    file_limit: u64,
) -> Result<ThemeFiles, FetchError> {
    let mut archive = open(vsix)?;
    let theme_path = checked_path(&theme.path)?;
    let theme_file = package_file(&mut archive, theme_path.clone(), file_limit)?;
    let include = with_json_object(&theme_file.bytes, &theme_path, |mut root| {
        root.take_string("include").map(|name| name.into_owned())
    })?;
    let include_file = match include {
        Some(name) => {
            let include_path = resolve_include(&theme_path, &name)?;
            Some(package_file(&mut archive, include_path, file_limit)?)
        }
        None => None,
    };
    let mut license = None;
    for name in LICENSE_NAMES {
        if let Some(bytes) = read_entry(&mut archive, &format!("extension/{name}"), LICENSE_LIMIT)?
        {
            license = Some(PackageFile {
                path: name.to_owned(),
                bytes,
            });
            break;
        }
    }
    Ok(ThemeFiles {
        theme: theme_file,
        include: include_file,
        license,
    })
}

/// The files of `theme`: the theme itself, the one file it includes and the
/// license. A missing license is fine, a missing theme or include is not.
pub fn read_theme_files(vsix: &[u8], theme: &ContributedTheme) -> Result<ThemeFiles, FetchError> {
    read_theme_files_limited(vsix, theme, SIZE_LIMIT)
}

fn write_failed(path: &Path, err: &std::io::Error) -> FetchError {
    FetchError::WriteFailed {
        path: path.display().to_string(),
        reason: err.to_string(),
    }
}

/// Writes `files` below `folder`, creating the folders they need.
fn write_files(folder: &Path, files: &ThemeFiles) -> Result<(), FetchError> {
    for file in files.all() {
        let target = folder.join(&file.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|err| write_failed(parent, &err))?;
        }
        std::fs::write(&target, &file.bytes).map_err(|err| write_failed(&target, &err))?;
    }
    Ok(())
}

/// Puts `theme` of the package below `themes/<id>/` next to the config file
/// and points `[theme] vscode` at it. Returns the config read back.
pub fn install_theme(
    vsix: &[u8],
    theme: &ContributedTheme,
    id: &ExtensionId,
    config_path: &Path,
) -> Result<Config, FetchError> {
    let files = read_theme_files(vsix, theme)?;
    let config_folder = config_path.parent().ok_or(FetchError::NoConfigPath)?;
    let folder = config_folder.join(THEMES_FOLDER).join(id.to_string());
    write_files(&folder, &files)?;
    let relative = format!("{THEMES_FOLDER}/{id}/{}", files.theme.path);
    Ok(config::write_vscode_theme(config_path, &relative)?)
}

/// Puts a theme file that came from a link below `themes/web/` next to the
/// config file and points `[theme] vscode` at it. The file must be a usable
/// theme: it is read back with the theme reader before it takes its place.
/// Returns the config read back and the name to show.
pub fn install_theme_file(
    bytes: &[u8],
    file_name: &str,
    config_path: &Path,
) -> Result<(Config, String), FetchError> {
    if bytes.len() as u64 > SIZE_LIMIT {
        return Err(FetchError::FileTooLarge(file_name.to_owned()));
    }
    let config_folder = config_path.parent().ok_or(FetchError::NoConfigPath)?;
    let folder = config_folder.join(THEMES_FOLDER).join(WEB_THEMES_FOLDER);
    std::fs::create_dir_all(&folder).map_err(|err| write_failed(&folder, &err))?;
    let target = folder.join(file_name);
    let candidate = folder.join(format!("{file_name}.part"));
    std::fs::write(&candidate, bytes).map_err(|err| write_failed(&candidate, &err))?;
    let theme = VscodeTheme::load(&candidate);
    let unusable = theme.problem.or_else(|| {
        theme
            .colors
            .is_empty()
            .then(|| "no usable colors".to_owned())
    });
    if let Some(reason) = unusable {
        drop(std::fs::remove_file(&candidate));
        return Err(FetchError::BadJson {
            file: file_name.to_owned(),
            reason,
        });
    }
    std::fs::rename(&candidate, &target).map_err(|err| write_failed(&target, &err))?;
    let relative = format!("{THEMES_FOLDER}/{WEB_THEMES_FOLDER}/{file_name}");
    let config = config::write_vscode_theme(config_path, &relative)?;
    let label = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem)
        .to_owned();
    Ok((config, label))
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(any(test, feature = "test-support"))]
pub mod fixtures;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests;
