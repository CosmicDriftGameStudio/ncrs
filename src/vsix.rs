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

use crate::config::{self, Config, SIZE_LIMIT};
use crate::open_vsx::{shown_text, ExtensionId, FetchError, MAX_SHOWN_CHARS, MAX_VSIX_BYTES};
use crate::vscode_theme::VscodeTheme;

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
#[cfg(test)]
pub mod tests {
    use std::io::Write as _;

    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    use super::*;

    /// A package with the given entries, each named below `extension/`.
    pub fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, bytes) in entries {
            writer
                .start_file(format!("extension/{name}"), options)
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    /// The manifest of an extension with these themes: label, uiTheme, path.
    pub fn manifest(themes: &[(&str, &str, &str)]) -> String {
        let listed: Vec<String> = themes
            .iter()
            .map(|(label, ui, path)| {
                format!(r#"{{"label": {label:?}, "uiTheme": {ui:?}, "path": {path:?}}}"#)
            })
            .collect();
        format!(r#"{{"contributes": {{"themes": [{}]}}}}"#, listed.join(","))
    }

    pub const THEME_JSON: &str = r##"{"colors": {"editor.background": "#102030"}}"##;

    fn one_theme_package() -> Vec<u8> {
        package(&[
            (
                "package.json",
                manifest(&[("Example Dark", "vs-dark", "./themes/dark.json")]).as_bytes(),
            ),
            ("themes/dark.json", THEME_JSON.as_bytes()),
        ])
    }

    fn theme_at(path: &str) -> ContributedTheme {
        ContributedTheme {
            label: "Example".to_owned(),
            brightness: None,
            path: path.to_owned(),
        }
    }

    #[test]
    fn paths_are_normalized_and_anything_that_could_leave_the_folder_is_refused() {
        assert_eq!(
            normalize_path("./themes/dark.json").as_deref(),
            Some("themes/dark.json")
        );
        assert_eq!(
            normalize_path("themes\\sub//dark.json").as_deref(),
            Some("themes/sub/dark.json")
        );
        assert_eq!(normalize_path("a/./b.json").as_deref(), Some("a/b.json"));
        for raw in [
            "",
            ".",
            "./",
            "../x.json",
            "a/../x.json",
            "a\\..\\x.json",
            "/abs.json",
            "\\abs.json",
            "C:x.json",
            "C:\\x.json",
            "a/b:c.json",
            "a/b\0.json",
        ] {
            assert_eq!(normalize_path(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn an_include_is_resolved_next_to_its_theme_and_never_above_it() {
        assert_eq!(
            resolve_include("themes/dark.json", "./base.json").unwrap(),
            "themes/base.json"
        );
        assert_eq!(
            resolve_include("dark.json", "base/x.json").unwrap(),
            "base/x.json"
        );
        assert_eq!(
            resolve_include("themes/dark.json", "../x.json"),
            Err(FetchError::UnsafePath("../x.json".to_owned()))
        );
    }

    #[test]
    fn a_single_theme_is_listed_with_its_brightness() {
        let themes = list_themes(&one_theme_package()).unwrap();
        assert_eq!(
            themes,
            [ContributedTheme {
                label: "Example Dark".to_owned(),
                brightness: Some(Brightness::Dark),
                path: "themes/dark.json".to_owned(),
            }]
        );
    }

    #[test]
    fn a_label_with_a_newline_or_bidi_control_is_cleaned() {
        let manifest = r#"{"contributes": {"themes": [{"label": "Ex\n\u202Eample\u2067 Dark", "uiTheme": "vs-dark", "path": "./a.json"}]}}"#;
        let vsix = package(&[("package.json", manifest.as_bytes())]);
        let themes = list_themes(&vsix).unwrap();
        assert_eq!(themes[0].label, "Example Dark");
    }

    #[test]
    fn several_themes_are_listed_in_order_with_dark_and_light() {
        let vsix = package(&[(
            "package.json",
            manifest(&[
                ("Example Dark", "vs-dark", "./a.json"),
                ("Example Light", "vs", "./b.json"),
                ("Example Contrast", "hc-black", "./c.json"),
                ("Example Contrast Light", "hc-light", "./d.json"),
                ("Example Unknown", "other", "./e.json"),
            ])
            .as_bytes(),
        )]);
        let listed: Vec<(String, Option<Brightness>)> = list_themes(&vsix)
            .unwrap()
            .into_iter()
            .map(|theme| (theme.label, theme.brightness))
            .collect();
        assert_eq!(
            listed,
            [
                ("Example Dark".to_owned(), Some(Brightness::Dark)),
                ("Example Light".to_owned(), Some(Brightness::Light)),
                ("Example Contrast".to_owned(), Some(Brightness::Dark)),
                ("Example Contrast Light".to_owned(), Some(Brightness::Light)),
                ("Example Unknown".to_owned(), None),
            ]
        );
    }

    #[test]
    fn a_package_without_themes_says_so() {
        for manifest in [
            r#"{"contributes": {"themes": []}}"#,
            r#"{"contributes": {}}"#,
            r#"{"name": "example"}"#,
        ] {
            let vsix = package(&[("package.json", manifest.as_bytes())]);
            assert_eq!(list_themes(&vsix), Err(FetchError::NoThemes), "{manifest}");
        }
    }

    #[test]
    fn a_package_without_a_manifest_or_that_is_no_archive_is_refused() {
        let vsix = package(&[("readme.md", b"hello")]);
        assert_eq!(
            list_themes(&vsix),
            Err(FetchError::MissingFile("package.json".to_owned()))
        );
        assert!(matches!(
            list_themes(b"this is not a zip archive"),
            Err(FetchError::BadPackage(_))
        ));
        let broken = package(&[("package.json", b"{not json")]);
        assert!(matches!(
            list_themes(&broken),
            Err(FetchError::BadJson { .. })
        ));
    }

    #[test]
    fn a_theme_path_that_leaves_the_package_fails_the_listing() {
        for path in ["../x.json", "/abs.json", "C:x.json", "a\\..\\x.json"] {
            let vsix = package(&[(
                "package.json",
                manifest(&[("Example", "vs-dark", path)]).as_bytes(),
            )]);
            assert_eq!(
                list_themes(&vsix),
                Err(FetchError::UnsafePath(path.to_owned())),
                "{path}"
            );
        }
    }

    #[test]
    fn a_theme_and_its_license_are_read() {
        let listed = manifest(&[("Example", "vs-dark", "./dark.json")]);
        let vsix = package(&[
            ("package.json", listed.as_bytes()),
            ("dark.json", THEME_JSON.as_bytes()),
            ("LICENSE.md", b"MIT text"),
            ("license", b"lower"),
        ]);
        let files = read_theme_files(&vsix, &theme_at("dark.json")).unwrap();
        assert_eq!(files.theme.path, "dark.json");
        assert_eq!(files.theme.bytes, THEME_JSON.as_bytes());
        assert_eq!(files.include, None);
        let license = files.license.unwrap();
        assert_eq!(
            license.path, "LICENSE.md",
            "the first name in the list that exists"
        );
        assert_eq!(license.bytes, b"MIT text");
    }

    #[test]
    fn a_missing_license_is_fine() {
        let files = read_theme_files(&one_theme_package(), &theme_at("themes/dark.json")).unwrap();
        assert_eq!(files.license, None);
    }

    #[test]
    fn the_included_file_is_read_relative_to_the_theme() {
        let vsix = package(&[
            (
                "themes/dark.json",
                br#"{"include": "./base/common.json", "colors": {}}"#,
            ),
            ("themes/base/common.json", THEME_JSON.as_bytes()),
        ]);
        let files = read_theme_files(&vsix, &theme_at("themes/dark.json")).unwrap();
        let include = files.include.unwrap();
        assert_eq!(include.path, "themes/base/common.json");
        assert_eq!(include.bytes, THEME_JSON.as_bytes());
    }

    #[test]
    fn an_include_that_climbs_out_is_refused() {
        for include in ["../x.json", "/x.json", "C:x.json", "a\\..\\x.json"] {
            let theme = format!(r#"{{"include": {include:?}, "colors": {{}}}}"#);
            let vsix = package(&[
                ("themes/dark.json", theme.as_bytes()),
                ("x.json", THEME_JSON.as_bytes()),
            ]);
            assert_eq!(
                read_theme_files(&vsix, &theme_at("themes/dark.json")),
                Err(FetchError::UnsafePath(include.to_owned())),
                "{include}"
            );
        }
    }

    #[test]
    fn a_missing_theme_or_include_names_the_file() {
        let vsix = package(&[("readme.md", b"x")]);
        assert_eq!(
            read_theme_files(&vsix, &theme_at("themes/dark.json")),
            Err(FetchError::MissingFile("themes/dark.json".to_owned()))
        );
        let with_include = package(&[("dark.json", br#"{"include": "base.json"}"#)]);
        assert_eq!(
            read_theme_files(&with_include, &theme_at("dark.json")),
            Err(FetchError::MissingFile("base.json".to_owned()))
        );
    }

    #[test]
    fn an_unsafe_theme_path_is_refused_when_reading_too() {
        let vsix = package(&[("package.json", b"{}")]);
        assert_eq!(
            read_theme_files(&vsix, &theme_at("../x.json")),
            Err(FetchError::UnsafePath("../x.json".to_owned()))
        );
    }

    #[test]
    fn a_theme_that_is_not_json_is_refused() {
        let vsix = package(&[("dark.json", b"colors: nope")]);
        assert!(matches!(
            read_theme_files(&vsix, &theme_at("dark.json")),
            Err(FetchError::BadJson { .. })
        ));
    }

    #[test]
    fn an_entry_is_cut_off_at_the_limit_whatever_the_archive_says() {
        let vsix = package(&[("big.json", &[b' '; 100])]);
        let mut archive = open(&vsix).unwrap();
        assert_eq!(
            read_entry(&mut archive, "extension/big.json", 100)
                .unwrap()
                .map(|bytes| bytes.len()),
            Some(100)
        );
        assert_eq!(
            read_entry(&mut archive, "extension/big.json", 99),
            Err(FetchError::FileTooLarge("extension/big.json".to_owned()))
        );
        assert_eq!(
            read_entry(&mut archive, "extension/none.json", 99),
            Ok(None)
        );
    }

    #[test]
    fn a_theme_of_more_than_a_mebibyte_is_refused() {
        let limit = usize::try_from(SIZE_LIMIT).unwrap();
        let padding = " ".repeat(limit);
        let too_big = format!(r#"{{"colors": {{}}}}{padding}"#);
        let vsix = package(&[("dark.json", too_big.as_bytes())]);
        assert_eq!(
            read_theme_files(&vsix, &theme_at("dark.json")),
            Err(FetchError::FileTooLarge("extension/dark.json".to_owned()))
        );

        let exactly = format!(
            r#"{{"colors": {{}}}}{}"#,
            " ".repeat(limit - r#"{"colors": {}}"#.len())
        );
        assert_eq!(exactly.len(), limit);
        let at_the_limit = package(&[("dark.json", exactly.as_bytes())]);
        read_theme_files(&at_the_limit, &theme_at("dark.json")).unwrap();
    }

    #[test]
    fn a_license_of_more_than_256_kib_is_refused() {
        let vsix = package(&[
            ("dark.json", THEME_JSON.as_bytes()),
            ("LICENSE", &vec![b'x'; 256 * 1024 + 1]),
        ]);
        assert_eq!(
            read_theme_files(&vsix, &theme_at("dark.json")),
            Err(FetchError::FileTooLarge("extension/LICENSE".to_owned()))
        );
    }

    #[test]
    fn a_package_over_the_size_limit_is_refused_without_reading_it() {
        let size = usize::try_from(MAX_VSIX_BYTES).unwrap() + 1;
        assert_eq!(list_themes(&vec![0; size]), Err(FetchError::TooLarge));
    }

    fn example_id() -> ExtensionId {
        ExtensionId::parse("example.dark-themes").unwrap()
    }

    #[test]
    fn an_installed_theme_lands_below_the_config_and_is_switched_on() {
        let base = tempfile::tempdir().unwrap();
        let config_path = base.path().join("config.toml");
        std::fs::write(&config_path, "# mine\nlang = \"en\"\n").unwrap();
        let vsix = package(&[
            (
                "themes/dark.json",
                br#"{"include": "./base/common.json", "colors": {}}"#,
            ),
            ("themes/base/common.json", THEME_JSON.as_bytes()),
            ("LICENSE", b"Example license"),
        ]);

        let config = install_theme(
            &vsix,
            &theme_at("themes/dark.json"),
            &example_id(),
            &config_path,
        )
        .unwrap();

        let folder = base.path().join("themes/example.dark-themes");
        assert_eq!(
            std::fs::read(folder.join("themes/base/common.json")).unwrap(),
            THEME_JSON.as_bytes()
        );
        assert!(folder.join("themes/dark.json").is_file());
        assert_eq!(
            std::fs::read(folder.join("LICENSE")).unwrap(),
            b"Example license"
        );
        let text = std::fs::read_to_string(&config_path).unwrap();
        assert!(text.starts_with("# mine\nlang = \"en\"\n"), "{text}");
        assert!(
            text.contains("vscode = \"themes/example.dark-themes/themes/dark.json\""),
            "{text}"
        );
        assert_eq!(config.vscode_warning, None);
        assert_eq!(
            config.palette.background,
            iced::Color::from_rgb8(0x10, 0x20, 0x30)
        );
    }

    #[test]
    fn installing_again_replaces_the_files_and_the_setting() {
        let base = tempfile::tempdir().unwrap();
        let config_path = base.path().join("config.toml");
        let first = package(&[("themes/dark.json", THEME_JSON.as_bytes())]);
        install_theme(
            &first,
            &theme_at("themes/dark.json"),
            &example_id(),
            &config_path,
        )
        .unwrap();

        let second = package(&[(
            "themes/light.json",
            br##"{"colors": {"editor.background": "#f0f0f0"}}"##,
        )]);
        let config = install_theme(
            &second,
            &theme_at("themes/light.json"),
            &example_id(),
            &config_path,
        )
        .unwrap();

        let text = std::fs::read_to_string(&config_path).unwrap();
        assert_eq!(text.matches("vscode").count(), 1, "{text}");
        assert_eq!(
            config.palette.background,
            iced::Color::from_rgb8(0xf0, 0xf0, 0xf0)
        );
    }

    #[test]
    fn a_package_that_fails_to_read_writes_nothing() {
        let base = tempfile::tempdir().unwrap();
        let config_path = base.path().join("config.toml");
        let vsix = package(&[("readme.md", b"x")]);

        let result = install_theme(
            &vsix,
            &theme_at("themes/dark.json"),
            &example_id(),
            &config_path,
        );

        assert_eq!(
            result,
            Err(FetchError::MissingFile("themes/dark.json".to_owned()))
        );
        assert!(!base.path().join("themes").exists());
        assert!(!config_path.exists());
    }

    #[test]
    fn a_folder_that_cannot_be_made_is_a_write_error() {
        let base = tempfile::tempdir().unwrap();
        let config_path = base.path().join("config.toml");
        std::fs::write(base.path().join("themes"), "in the way").unwrap();
        let vsix = package(&[("themes/dark.json", THEME_JSON.as_bytes())]);

        let result = install_theme(
            &vsix,
            &theme_at("themes/dark.json"),
            &example_id(),
            &config_path,
        );

        assert!(
            matches!(result, Err(FetchError::WriteFailed { .. })),
            "{result:?}"
        );
        assert!(!config_path.exists());
    }

    const WEB_THEME: &str = r##"{"colors": {"editor.background": "#102030"}}"##;

    #[test]
    fn a_theme_file_from_a_link_is_installed_below_themes_web() {
        let base = tempfile::tempdir().unwrap();
        let config_path = base.path().join("config.toml");

        let (config, label) =
            install_theme_file(WEB_THEME.as_bytes(), "example.com-dark.json", &config_path)
                .unwrap();

        assert_eq!(label, "example.com-dark");
        assert!(base
            .path()
            .join("themes/web/example.com-dark.json")
            .is_file());
        assert!(!base
            .path()
            .join("themes/web/example.com-dark.json.part")
            .exists());
        let text = std::fs::read_to_string(&config_path).unwrap();
        assert!(text.contains("themes/web/example.com-dark.json"), "{text}");
        drop(config);
    }

    #[test]
    fn a_link_that_is_no_theme_leaves_the_old_theme_and_the_config_alone() {
        let base = tempfile::tempdir().unwrap();
        let config_path = base.path().join("config.toml");
        install_theme_file(WEB_THEME.as_bytes(), "example.com-dark.json", &config_path).unwrap();
        let before = std::fs::read_to_string(&config_path).unwrap();

        for bad in [
            &b"<html>not json</html>"[..],
            b"{}",
            b"{\"colors\": {}}",
            b"[1]",
        ] {
            let result = install_theme_file(bad, "example.com-dark.json", &config_path);
            assert!(
                matches!(result, Err(FetchError::BadJson { .. })),
                "{result:?}"
            );
        }
        let too_big = vec![b' '; usize::try_from(SIZE_LIMIT).unwrap() + 1];
        assert!(matches!(
            install_theme_file(&too_big, "example.com-dark.json", &config_path),
            Err(FetchError::FileTooLarge(_))
        ));

        assert_eq!(
            std::fs::read_to_string(base.path().join("themes/web/example.com-dark.json")).unwrap(),
            WEB_THEME
        );
        assert_eq!(std::fs::read_to_string(&config_path).unwrap(), before);
        assert!(!base
            .path()
            .join("themes/web/example.com-dark.json.part")
            .exists());
    }
}
