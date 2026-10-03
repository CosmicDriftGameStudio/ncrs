//! The config file: the shared shape of everything the user can set.
//!
//! Later subsystems (keymap, theme, language) add their sections as optional
//! fields on `Config`, so a file written for today's version keeps loading.
//! `version` is there for migration. Unknown fields are rejected rather than
//! ignored: a misspelt key would otherwise silently do nothing.
//!
//! A missing file is not an error, the defaults apply. A broken file is: the
//! whole config falls back to the defaults (no partial merge, which would
//! leave a half-applied setup nobody can reason about) and the caller reports
//! the error.

use std::ffi::OsString;
use std::fmt;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::de::{Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_spanned::Spanned;

use crate::keymap::Keymap;
use crate::palette::{ColorRole, Palette};

/// The only version this build reads.
pub const CURRENT_VERSION: u32 = 1;

/// A config is a few lines; anything bigger is not one, and reading it would
/// only delay the start.
const SIZE_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub version: Version,
    #[serde(default)]
    pub open: OpenPrograms,
    /// The `[keys]` section as written; `load` turns it into `keymap`.
    #[serde(default)]
    pub keys: KeySection,
    /// The keys in effect: the built-in ones with `keys` applied.
    #[serde(skip)]
    pub keymap: Arc<Keymap>,
    /// The `[theme]` section as written; `load` turns it into `palette`.
    #[serde(default)]
    pub theme: ThemeSection,
    /// The colors in effect: the built-in ones with `theme` applied.
    #[serde(skip)]
    pub palette: Palette,
    /// Set when the palette is legal but hard to read. It does not stop the
    /// palette from being used.
    #[serde(skip)]
    pub contrast_warning: Option<ContrastWarning>,
}

/// A text and its ground in the `[theme]` palette whose contrast is low.
#[derive(Debug, Clone, PartialEq)]
pub struct ContrastWarning {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub foreground: ColorRole,
    pub background: ColorRole,
    pub ratio: f32,
}

/// One line of `[theme]`: a role (or `preset`) and the text written for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeEntry {
    pub key: Spanned<String>,
    pub value: Spanned<String>,
}

/// The `[theme]` section, unchecked. Like `KeySection`, it keeps the text with
/// its position so a wrong color is reported at its own line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeSection {
    pub entries: Vec<ThemeEntry>,
}

impl<'de> Deserialize<'de> for ThemeSection {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ThemeSectionVisitor;

        impl<'de> Visitor<'de> for ThemeSectionVisitor {
            type Value = ThemeSection;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a table of color roles")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ThemeSection, A::Error> {
                let mut entries = Vec::new();
                while let Some(key) = map.next_key::<Spanned<String>>()? {
                    let value = map.next_value::<Spanned<String>>()?;
                    entries.push(ThemeEntry { key, value });
                }
                Ok(ThemeSection { entries })
            }
        }

        deserializer.deserialize_map(ThemeSectionVisitor)
    }
}

/// One line of `[keys]`: an action and the keys written for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEntry {
    pub action: Spanned<String>,
    pub chords: Vec<Spanned<String>>,
}

/// The `[keys]` section, unchecked. Names and keys stay text with their
/// position, so a wrong one is reported at its own line once the keymap has
/// looked at it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeySection {
    pub entries: Vec<KeyEntry>,
}

/// A single key or a list of them.
enum Chords {
    One(String),
    Many(Vec<Spanned<String>>),
}

impl<'de> Deserialize<'de> for Chords {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ChordsVisitor;

        impl<'de> Visitor<'de> for ChordsVisitor {
            type Value = Chords;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a key such as \"F5\" or a list of keys")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Chords, E> {
                Ok(Chords::One(v.to_owned()))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Chords, A::Error> {
                let mut chords = Vec::new();
                while let Some(chord) = seq.next_element::<Spanned<String>>()? {
                    chords.push(chord);
                }
                Ok(Chords::Many(chords))
            }
        }

        deserializer.deserialize_any(ChordsVisitor)
    }
}

impl<'de> Deserialize<'de> for KeySection {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct KeySectionVisitor;

        impl<'de> Visitor<'de> for KeySectionVisitor {
            type Value = KeySection;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a table of action names")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<KeySection, A::Error> {
                let mut entries = Vec::new();
                while let Some(action) = map.next_key::<Spanned<String>>()? {
                    let value = map.next_value::<Spanned<Chords>>()?;
                    let span = value.span();
                    let chords = match value.into_inner() {
                        Chords::One(text) => vec![Spanned::new(span, text)],
                        Chords::Many(texts) => texts,
                    };
                    entries.push(KeyEntry { action, chords });
                }
                Ok(KeySection { entries })
            }
        }

        deserializer.deserialize_map(KeySectionVisitor)
    }
}

/// The `version` key. Anything but `CURRENT_VERSION` is refused, so a file
/// from a newer ncrs is not half understood.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version(u32);

impl Default for Version {
    fn default() -> Self {
        Self(CURRENT_VERSION)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let number = u32::deserialize(deserializer)?;
        if number == CURRENT_VERSION {
            Ok(Self(number))
        } else {
            Err(D::Error::custom(format!(
                "unsupported version {number}, this ncrs reads version {CURRENT_VERSION}"
            )))
        }
    }
}

/// The `[open]` section: the programs behind F3 and F4. A missing one means
/// the system default of the platform.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenPrograms {
    #[serde(default)]
    pub view: Option<ProgramLine>,
    #[serde(default)]
    pub edit: Option<ProgramLine>,
}

/// Program plus leading arguments; the file is always appended as the last
/// argument by `open_command`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramLine {
    program: String,
    args: Vec<String>,
}

impl ProgramLine {
    /// Not validated: for the built-in defaults and tests. The file goes
    /// through `Deserialize`, which checks.
    pub fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        }
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    fn parse(mut words: Vec<String>) -> Result<Self, &'static str> {
        if words.is_empty() {
            return Err("needs at least the program");
        }
        let program = words.remove(0);
        if program.trim().is_empty() {
            return Err("the program is empty");
        }
        if program.contains('\0') || words.iter().any(|arg| arg.contains('\0')) {
            return Err("contains a NUL character");
        }
        if depends_on_working_directory(&program) {
            return Err("use a program name from PATH or an absolute path");
        }
        Ok(Self {
            program,
            args: words,
        })
    }
}

/// A relative program with a separator, or on Windows a drive-relative one
/// such as `C:edit.exe` or `\tools\edit.exe`, would depend on the directory
/// ncrs happened to start in.
fn depends_on_working_directory(program: &str) -> bool {
    let path = Path::new(program);
    let anchored_elsewhere = path
        .components()
        .any(|part| matches!(part, Component::Prefix(_) | Component::RootDir));
    !path.is_absolute() && (program.contains(is_separator) || anchored_elsewhere)
}

fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

impl<'de> Deserialize<'de> for ProgramLine {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let words = Vec::<String>::deserialize(deserializer)?;
        Self::parse(words).map_err(D::Error::custom)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    /// Not read at all: an I/O error other than "not found", not a regular
    /// file, or too big.
    Unreadable { path: PathBuf, reason: String },
    /// Read, but not a valid config: TOML or schema error, or invalid UTF-8.
    Invalid {
        path: PathBuf,
        line: Option<usize>,
        reason: String,
    },
}

impl ConfigError {
    pub fn path(&self) -> &Path {
        match self {
            Self::Unreadable { path, .. } | Self::Invalid { path, .. } => path,
        }
    }

    pub fn line(&self) -> Option<usize> {
        match self {
            Self::Unreadable { .. } => None,
            Self::Invalid { line, .. } => *line,
        }
    }

    pub fn reason(&self) -> &str {
        match self {
            Self::Unreadable { reason, .. } | Self::Invalid { reason, .. } => reason,
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "config file {}", self.path().display())?;
        if let Some(line) = self.line() {
            write!(f, ", line {line}")?;
        }
        write!(f, ": {}. Using the defaults.", self.reason())
    }
}

impl std::error::Error for ConfigError {}

/// Where the config file lives on this machine, or `None` when the variables
/// that say so are missing.
pub fn default_path() -> Option<PathBuf> {
    path_from_env(|name| std::env::var_os(name))
}

fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}

/// Windows: `%APPDATA%\ncrs\config.toml`.
#[cfg(windows)]
fn path_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("APPDATA")).map(|dir| dir.join("ncrs").join("config.toml"))
}

/// macOS: always `~/.config`. A Dock launch sees no shell exports, so honouring
/// `XDG_CONFIG_HOME` would make the file depend on how ncrs was started.
#[cfg(target_os = "macos")]
fn path_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    home_config(var)
}

/// Other Unix: `$XDG_CONFIG_HOME` if set and absolute, else `~/.config`.
#[cfg(not(any(target_os = "macos", windows)))]
fn path_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("XDG_CONFIG_HOME"))
        .map(|dir| dir.join("ncrs").join("config.toml"))
        .or_else(|| home_config(var))
}

/// No fallback to the working directory or `/` the way `fs::home_dir` has: a
/// config read from wherever ncrs started would be a surprise.
#[cfg(not(windows))]
fn home_config(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("HOME")).map(|home| home.join(".config").join("ncrs").join("config.toml"))
}

/// Reads the config at `path`. A missing file gives the defaults.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
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
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
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
    let source = match String::from_utf8(bytes) {
        Ok(source) => source,
        Err(err) => {
            let valid = err.utf8_error().valid_up_to();
            return Err(ConfigError::Invalid {
                path: path.to_path_buf(),
                line: line_at(err.as_bytes(), valid),
                reason: "not valid UTF-8".to_owned(),
            });
        }
    };
    let mut config =
        toml_edit::de::from_str::<Config>(&source).map_err(|err| ConfigError::Invalid {
            path: path.to_path_buf(),
            line: err
                .span()
                .and_then(|span| line_at(source.as_bytes(), span.start)),
            reason: err.message().trim().to_owned(),
        })?;
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
        let configured =
            Palette::from_section(&config.theme).map_err(|err| ConfigError::Invalid {
                path: path.to_path_buf(),
                line: line_at(source.as_bytes(), err.span.start),
                reason: err.reason,
            })?;
        config.contrast_warning = configured.palette.weakest_contrast().map(|weak| {
            let found = configured
                .span_of(weak.foreground)
                .or_else(|| configured.span_of(weak.background))
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
    Ok(config)
}

/// 1-based line of the byte at `offset`.
fn line_at(source: &[u8], offset: usize) -> Option<usize> {
    let before = source.get(..offset)?;
    Some(before.iter().filter(|&&byte| byte == b'\n').count() + 1)
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &tempfile::TempDir, content: &[u8]) -> PathBuf {
        let path = dir.path().join("config.toml");
        std::fs::write(&path, content).unwrap();
        path
    }

    fn load_text(content: &str) -> Config {
        let dir = tempfile::tempdir().unwrap();
        load(&write(&dir, content.as_bytes())).unwrap()
    }

    /// The (line, reason) of the error the content produces.
    fn error_line(content: &str) -> (Option<usize>, String) {
        let dir = tempfile::tempdir().unwrap();
        let err = load(&write(&dir, content.as_bytes())).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err:?}");
        (err.line(), err.reason().to_owned())
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load(&dir.path().join("nope.toml")).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn an_empty_file_gives_the_defaults() {
        assert_eq!(load_text(""), Config::default());
    }

    #[test]
    fn a_full_file_is_read() {
        let config =
            load_text("version = 1\n\n[open]\nview = [\"viewer\", \"--x\"]\nedit = [\"editor\"]\n");
        let view = config.open.view.unwrap();
        assert_eq!(view.program(), "viewer");
        assert_eq!(view.args(), ["--x"]);
        let edit = config.open.edit.unwrap();
        assert_eq!(edit.program(), "editor");
        assert!(edit.args().is_empty());
    }

    #[test]
    fn a_missing_version_means_the_current_one() {
        let config = load_text("[open]\nview = [\"viewer\"]\n");
        assert_eq!(config.version, Version::default());
    }

    #[test]
    fn a_program_name_and_an_absolute_path_are_valid() {
        #[cfg(windows)]
        let absolute = r#"["C:\\Tools\\edit.exe"]"#;
        #[cfg(not(windows))]
        let absolute = r#"["/usr/bin/vim", "-R"]"#;
        let text = format!("[open]\nview = [\"less\"]\nedit = {absolute}\n");
        let config = load_text(&text);
        assert_eq!(config.open.view.unwrap().program(), "less");
        assert!(config.open.edit.is_some());
    }

    #[test]
    fn errors_name_the_exact_line() {
        let cases: &[(&str, &str, usize)] = &[
            ("unknown top-level key", "version = 1\n\nnope = 1\n", 3),
            (
                "unknown key in [open]",
                "version = 1\n[open]\ncolour = \"red\"\n",
                3,
            ),
            ("wrong type", "version = 1\n[open]\nview = \"open\"\n", 3),
            ("empty array", "version = 1\n[open]\nview = []\n", 3),
            ("empty program", "version = 1\n[open]\nview = [\"\"]\n", 3),
            (
                "relative path",
                "version = 1\n[open]\nedit = [\"bin/edit\"]\n",
                3,
            ),
            (
                "home-relative path",
                "version = 1\n[open]\nedit = [\"~/bin/edit\"]\n",
                3,
            ),
            ("NUL", "version = 1\n[open]\nedit = [\"a\\u0000b\"]\n", 3),
            #[cfg(windows)]
            (
                "drive-relative path",
                "version = 1\n[open]\nedit = [\"C:edit.exe\"]\n",
                3,
            ),
            ("version", "# comment\nversion = 2\n", 2),
            ("syntax", "version = 1\n\n[open\n", 3),
        ];
        for (name, text, line) in cases {
            let (found, reason) = error_line(text);
            assert_eq!(found, Some(*line), "{name}: {reason}");
        }
    }

    #[test]
    fn the_reasons_say_what_is_wrong() {
        let cases = [
            ("version = 1\n[open]\nview = []\n", "at least the program"),
            ("version = 1\n[open]\nview = [\" \"]\n", "program is empty"),
            (
                "version = 1\n[open]\nview = [\"a/b\"]\n",
                "PATH or an absolute path",
            ),
            (
                "version = 2\n",
                "unsupported version 2, this ncrs reads version 1",
            ),
        ];
        for (text, expected) in cases {
            let (_, found) = error_line(text);
            assert!(found.contains(expected), "{found}");
        }
    }

    #[test]
    fn invalid_utf8_reports_its_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, b"version = 1\n# \xff\xfe\n");
        let err = load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err:?}");
        assert_eq!(err.line(), Some(2));
    }

    #[test]
    fn a_directory_is_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let err = load(dir.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Unreadable { .. }), "{err:?}");
        assert_eq!(err.line(), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("config.toml");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        let err = load(&fifo).unwrap_err();
        assert!(matches!(err, ConfigError::Unreadable { .. }), "{err:?}");
    }

    #[test]
    fn a_file_over_one_mebibyte_is_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "#".repeat(1024 * 1024 + 1).as_bytes());
        let err = load(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Unreadable { .. }), "{err:?}");
    }

    #[test]
    fn the_message_names_path_and_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, b"version = 1\n\nnope = 1\n");
        let text = load(&path).unwrap_err().to_string();
        assert!(text.contains(&path.display().to_string()), "{text}");
        assert!(text.contains("line 3"), "{text}");
    }

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(*value))
        }
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn linux_prefers_an_absolute_xdg_config_home() {
        let found = path_from_env(env(&[("XDG_CONFIG_HOME", "/x/cfg"), ("HOME", "/h")]));
        assert_eq!(found, Some(PathBuf::from("/x/cfg/ncrs/config.toml")));
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn linux_falls_back_to_home_when_xdg_is_relative_or_empty() {
        for xdg in ["rel/cfg", ""] {
            let found = path_from_env(env(&[("XDG_CONFIG_HOME", xdg), ("HOME", "/h")]));
            assert_eq!(found, Some(PathBuf::from("/h/.config/ncrs/config.toml")));
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn no_usable_home_means_no_path() {
        assert_eq!(path_from_env(env(&[])), None);
        assert_eq!(path_from_env(env(&[("HOME", "")])), None);
        assert_eq!(path_from_env(env(&[("HOME", "rel")])), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_ignores_xdg_config_home() {
        let found = path_from_env(env(&[("XDG_CONFIG_HOME", "/x/cfg"), ("HOME", "/h")]));
        assert_eq!(found, Some(PathBuf::from("/h/.config/ncrs/config.toml")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_uses_appdata() {
        let found = path_from_env(env(&[("APPDATA", r"C:\Users\u\AppData\Roaming")]));
        assert_eq!(
            found,
            Some(PathBuf::from(
                r"C:\Users\u\AppData\Roaming\ncrs\config.toml"
            ))
        );
        assert_eq!(path_from_env(env(&[("APPDATA", "rel")])), None);
        assert_eq!(path_from_env(env(&[])), None);
    }
}
