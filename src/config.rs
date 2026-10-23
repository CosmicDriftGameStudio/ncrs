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
use std::num::NonZeroUsize;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use ncrs_intake::LocationMode;
use ncrs_pipeline::Load;
use serde::de::{Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_spanned::Spanned;

use crate::i18n::Language;
use crate::keymap::Keymap;
use crate::palette::{ColorRole, Palette, VSCODE_KEY};
use crate::vscode_theme::VscodeTheme;

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
    /// The language the session starts in; F9 switches it for the session
    /// only, nothing is written back.
    #[serde(default)]
    pub lang: Option<Language>,
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
    /// Set when the VS Code theme named in `[theme]` could not be used in
    /// full. The preset and the roles still apply.
    #[serde(skip)]
    pub vscode_warning: Option<VscodeWarning>,
    /// The `[modules]` sections as written; `load` turns `filing` into the
    /// field of the same name below.
    #[serde(default)]
    pub modules: ModulesSection,
    /// The filing module as `load` checked it: text rules only, the folders
    /// may not exist.
    #[serde(skip)]
    pub filing: Filing,
}

/// The `[modules]` table: one sub-table per module.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulesSection {
    #[serde(default)]
    pub filing: FilingSection,
}

/// The `[modules.filing]` section, unchecked.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilingSection {
    #[serde(default)]
    pub enabled: bool,
    /// Keeps the position of the whole array for the "no target" error.
    #[serde(default)]
    pub locations: Option<Spanned<Vec<LocationEntry>>>,
    #[serde(default)]
    pub load: Option<Spanned<LoadValue>>,
}

/// The spelling of `load` in the file; `resolve_filing` maps it to `Load`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadValue {
    Name(String),
    Count(i64),
}

impl<'de> Deserialize<'de> for LoadValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LoadVisitor;

        impl Visitor<'_> for LoadVisitor {
            type Value = LoadValue;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(r#""gentle", "balanced", "full" or a number of workers"#)
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<LoadValue, E> {
                Ok(LoadValue::Name(v.to_owned()))
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<LoadValue, E> {
                Ok(LoadValue::Count(v))
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<LoadValue, E> {
                // Beyond i64 is far above any core count; it is capped later.
                Ok(LoadValue::Count(i64::try_from(v).unwrap_or(i64::MAX)))
            }
        }

        deserializer.deserialize_any(LoadVisitor)
    }
}

/// One `[[modules.filing.locations]]` table, unchecked.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocationEntry {
    pub path: Spanned<String>,
    pub mode: Spanned<LocationModeName>,
    #[serde(default)]
    pub images: bool,
    #[serde(default)]
    pub target: Option<Spanned<bool>>,
}

/// The spelling of a mode in the file; `load` maps it to `LocationMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocationModeName {
    Inbox,
    Archive,
}

impl From<LocationModeName> for LocationMode {
    fn from(name: LocationModeName) -> Self {
        match name {
            LocationModeName::Inbox => Self::Inbox,
            LocationModeName::Archive => Self::Archive,
        }
    }
}

/// The filing module's settings in effect.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filing {
    pub enabled: bool,
    pub locations: Vec<FilingLocation>,
    /// How many workers the pipeline may use; counts above the core count are
    /// capped when the pipeline starts.
    pub load: Load,
}

/// A location with an expanded, absolute, `..`-free path. Not looked up on
/// disk: whether the folder exists is for the module to find out when it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingLocation {
    pub path: PathBuf,
    pub mode: LocationMode,
    pub images: bool,
    pub is_target: bool,
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

/// The VS Code theme file named by `vscode` in `[theme]` could not be used,
/// or not in full.
#[derive(Debug, Clone, PartialEq)]
pub struct VscodeWarning {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub theme: PathBuf,
    pub reason: String,
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

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let code = String::deserialize(deserializer)?;
        Language::from_code(&code).ok_or_else(|| {
            let allowed = Language::ALL.map(|lang| format!("\"{}\"", lang.code()));
            D::Error::custom(format!(
                "unknown language \"{code}\", use one of {}",
                allowed.join(", ")
            ))
        })
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

/// Program plus leading arguments. `{path}` and `{line}` in an argument are
/// replaced by the file and the line to jump to; only when no argument has
/// `{path}` is the file appended as the last argument (see `open_command`).
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Everything that finds the user's own config, index or keychain calls this
/// first: a test that got there would read or change the real data of whoever
/// runs it.
#[allow(clippy::panic, reason = "the guard is the failure, and only in tests")]
pub fn forbid_user_machine_in_tests(what: &str) {
    if cfg!(test) {
        panic!(
            "{what} reaches the user's config, index or keychain. Tests build the app with \
             App::with_environment(Environment::isolated()) and never touch those."
        );
    }
}

/// Where the config file lives on this machine, or `None` when the variables
/// that say so are missing.
pub fn default_path() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::default_path");
    config_path_in(portable_data_dir(), |name| std::env::var_os(name))
}

const PORTABLE_DIR: &str = "data";

/// The `data` folder next to the executable, when there is one: it makes
/// this copy of ncrs portable, and config and index live there instead of in
/// the user's profile. `None` means the normal layout. The executable path
/// should be canonical, so the result can serve as a stable key.
pub fn portable_data_dir_of(exe: &Path, is_dir: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let dir = exe.parent()?;
    if is_inside_app_bundle(dir) {
        return None;
    }
    let data = dir.join(PORTABLE_DIR);
    is_dir(&data).then_some(data)
}

/// `Foo.app/Contents/MacOS`: the bundle is replaced on update and may sit in
/// a read-only place, so a `data` folder there is not a portable marker.
fn is_inside_app_bundle(exe_dir: &Path) -> bool {
    let contents = exe_dir.parent();
    exe_dir.file_name() == Some("MacOS".as_ref())
        && contents.and_then(Path::file_name) == Some("Contents".as_ref())
        && contents
            .and_then(Path::parent)
            .and_then(Path::extension)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
}

/// The portable data directory of the running executable. A failing
/// `current_exe` or `canonicalize` means the normal layout.
pub fn portable_data_dir() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::portable_data_dir");
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .ok()?;
    portable_data_dir_of(&exe, Path::is_dir)
}

/// Config and index both ask this, so they never end up in different modes.
fn config_path_in(
    portable: Option<PathBuf>,
    var: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    match portable {
        Some(dir) => Some(dir.join("config.toml")),
        None => path_from_env(var),
    }
}

fn index_path_in(
    portable: Option<PathBuf>,
    var: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    match portable {
        Some(dir) => Some(dir.join("index.db")),
        None => index_path_from_env(var),
    }
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

/// Where the index database lives on this machine, or `None` when the
/// variables that say so are missing.
pub fn default_index_path() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::default_index_path");
    index_path_in(portable_data_dir(), |name| std::env::var_os(name))
}

/// Windows: `%APPDATA%\ncrs\index.db`.
#[cfg(windows)]
fn index_path_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("APPDATA")).map(|dir| dir.join("ncrs").join("index.db"))
}

/// macOS: always `~/Library/Application Support`, for the same reason the
/// config ignores `XDG_CONFIG_HOME`.
#[cfg(target_os = "macos")]
fn index_path_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("HOME")).map(|home| {
        home.join("Library")
            .join("Application Support")
            .join("ncrs")
            .join("index.db")
    })
}

/// Other Unix: `$XDG_DATA_HOME` if set and absolute, else `~/.local/share`.
#[cfg(not(any(target_os = "macos", windows)))]
fn index_path_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("XDG_DATA_HOME"))
        .or_else(|| absolute(var("HOME")).map(|home| home.join(".local").join("share")))
        .map(|dir| dir.join("ncrs").join("index.db"))
}

/// The home directory `~` stands for. Like the config path, only an absolute
/// one counts.
fn home_from_env() -> Option<PathBuf> {
    #[cfg(windows)]
    let variable = "USERPROFILE";
    #[cfg(not(windows))]
    let variable = "HOME";
    absolute(std::env::var_os(variable))
}

/// Reads the config at `path`. A missing file gives the defaults.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    load_with_home(path, home_from_env().as_deref())
}

fn load_with_home(path: &Path, home: Option<&Path>) -> Result<Config, ConfigError> {
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

/// Every check `load` makes on the text; `path` only names the file in errors.
fn parse_config(source: &str, path: &Path, home: Option<&Path>) -> Result<Config, ConfigError> {
    let mut config =
        toml_edit::de::from_str::<Config>(source).map_err(|err| ConfigError::Invalid {
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
    Ok(config)
}

/// The VS Code theme `[theme]` names, read, with where it was named.
struct VscodeLayer {
    theme: VscodeTheme,
    span: Range<usize>,
    file: PathBuf,
}

/// Reads the file named by `vscode`. It never fails the config: whatever is
/// wrong with the file ends up in `theme.problem`.
fn load_vscode(
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

struct FilingError {
    span: Range<usize>,
    reason: String,
}

/// Text rules only, no file system access: a dead mount must not block the
/// start or cost the user the rest of the config.
fn resolve_filing(section: &FilingSection, home: Option<&Path>) -> Result<Filing, FilingError> {
    let mut filing = Filing {
        enabled: section.enabled,
        locations: Vec::new(),
        load: resolve_load(section.load.as_ref())?,
    };
    let Some(listed) = &section.locations else {
        return Ok(filing);
    };
    let entries = listed.get_ref();
    for entry in entries {
        let path_error = |reason: String| FilingError {
            span: entry.path.span(),
            reason,
        };
        let path = expand_location_path(entry.path.get_ref(), home).map_err(path_error)?;
        filing.locations.push(FilingLocation {
            path,
            mode: LocationMode::from(*entry.mode.get_ref()),
            images: entry.images,
            is_target: entry.target.as_ref().is_some_and(|flag| *flag.get_ref()),
        });
    }
    check_filing_rules(&filing.locations).map_err(|violation| {
        let target_flag_span = |index: usize| {
            entries
                .get(index)
                .and_then(|entry| entry.target.as_ref())
                .map_or_else(|| listed.span(), Spanned::span)
        };
        let path_of = |index: usize| {
            filing
                .locations
                .get(index)
                .map(|location| location.path.display().to_string())
                .unwrap_or_default()
        };
        match violation {
            FilingRuleViolation::InboxAsTarget { index } => FilingError {
                span: target_flag_span(index),
                reason: "an inbox cannot be the target, set `target = true` on an archive"
                    .to_owned(),
            },
            FilingRuleViolation::SecondTarget { index } => FilingError {
                span: target_flag_span(index),
                reason: "only one location can have `target = true`".to_owned(),
            },
            FilingRuleViolation::NoTarget => FilingError {
                span: listed.span(),
                reason: "no location has `target = true`, one archive must be the target"
                    .to_owned(),
            },
            FilingRuleViolation::Overlap { earlier, later } => FilingError {
                span: entries
                    .get(later)
                    .map_or_else(|| listed.span(), |entry| entry.path.span()),
                reason: format!(
                    "{} and {} overlap, a location cannot be the same folder as another or lie inside it",
                    path_of(earlier),
                    path_of(later)
                ),
            },
        }
    })?;
    Ok(filing)
}

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

fn write_filing_locations_with_home(
    path: &Path,
    locations: &[FilingLocation],
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    if locations.is_empty() {
        return Err(FilingWriteError::Rule(FilingRuleViolation::NoTarget));
    }
    check_filing_rules(locations).map_err(FilingWriteError::Rule)?;
    let target = resolve_write_target(path)?;
    let source = read_source(&target).map_err(FilingWriteError::Config)?;
    // The edit is based on exactly this text, so a mistake in it would be
    // carried into the new file or, worse, papered over.
    if let Some(text) = &source {
        parse_config(text, path, home).map_err(FilingWriteError::Config)?;
    }
    let edited = with_filing_locations(source.as_deref().unwrap_or_default(), path, locations)?;
    parse_config(&edited, path, home).map_err(FilingWriteError::Config)?;
    replace_file(&target, &edited)?;
    load_with_home(path, home).map_err(FilingWriteError::Config)
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> FilingWriteError + '_ {
    move |err| FilingWriteError::Io {
        path: path.to_path_buf(),
        reason: err.to_string(),
    }
}

/// The file a write replaces. A symlinked config stays a symlink: the file it
/// points to gets the new text. A missing config is created with its folders.
fn resolve_write_target(path: &Path) -> Result<PathBuf, FilingWriteError> {
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

/// `source` with `enabled = true` and `locations` in `[modules.filing]`.
fn with_filing_locations(
    source: &str,
    path: &Path,
    locations: &[FilingLocation],
) -> Result<String, FilingWriteError> {
    let not_a_table = |key: &str| {
        FilingWriteError::Config(ConfigError::Invalid {
            path: path.to_path_buf(),
            line: None,
            reason: format!("`{key}` is not a table"),
        })
    };
    let mut document = source.parse::<toml_edit::DocumentMut>().map_err(|err| {
        FilingWriteError::Config(ConfigError::Invalid {
            path: path.to_path_buf(),
            line: err
                .span()
                .and_then(|span| line_at(source.as_bytes(), span.start)),
            reason: err.message().trim().to_owned(),
        })
    })?;
    let modules = sub_table(document.as_table_mut(), "modules", true)
        .ok_or_else(|| not_a_table("modules"))?;
    let filing =
        sub_table(modules, "filing", false).ok_or_else(|| not_a_table("modules.filing"))?;
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
fn sub_table<'a>(
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
fn replace_file(target: &Path, content: &str) -> Result<(), FilingWriteError> {
    use std::io::Write as _;
    let folder = target.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(".config.toml.")
        .suffix(".tmp")
        .tempfile_in(folder)
        .map_err(io_error(folder))?;
    if let Ok(previous) = std::fs::metadata(target) {
        temporary
            .as_file()
            .set_permissions(previous.permissions())
            .map_err(io_error(target))?;
    }
    temporary
        .write_all(content.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(io_error(target))?;
    temporary
        .persist(target)
        .map_err(|err| io_error(target)(err.error))?;
    Ok(())
}

fn resolve_load(value: Option<&Spanned<LoadValue>>) -> Result<Load, FilingError> {
    let Some(spanned) = value else {
        return Ok(Load::default());
    };
    let resolved = match spanned.get_ref() {
        LoadValue::Name(name) => match name.as_str() {
            "gentle" => Some(Load::Gentle),
            "balanced" => Some(Load::Balanced),
            "full" => Some(Load::Full),
            _ => None,
        },
        LoadValue::Count(count) => usize::try_from(*count)
            .ok()
            .and_then(NonZeroUsize::new)
            .map(Load::Workers),
    };
    resolved.ok_or_else(|| FilingError {
        span: spanned.span(),
        reason: r#"load must be "gentle", "balanced", "full" or a number of workers from 1 up"#
            .to_owned(),
    })
}

/// `~` and `~/rest` become the home directory; everything else must already
/// be absolute. A `..` component would make the overlap check wrong.
fn expand_location_path(text: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let expanded = match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(is_separator) => {
            let known_home = home.ok_or("no home directory to expand `~`")?;
            known_home.join(rest.trim_start_matches(is_separator))
        }
        _ => PathBuf::from(text),
    };
    if !expanded.is_absolute() {
        return Err(format!(
            "the path \"{text}\" must be absolute or start with `~/`"
        ));
    }
    if expanded
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(format!("the path \"{text}\" must not contain `..`"));
    }
    Ok(expanded)
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

    fn target_of(filing: &Filing) -> Option<&FilingLocation> {
        filing.locations.iter().find(|location| location.is_target)
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
    fn the_start_language_is_read_and_optional() {
        assert_eq!(load_text("lang = \"de\"\n").lang, Some(Language::German));
        assert_eq!(load_text("lang = \"en\"\n").lang, Some(Language::English));
        assert_eq!(load_text("").lang, None);
    }

    #[test]
    fn an_unknown_language_names_its_line_and_the_allowed_values() {
        let (line, reason) = error_line("version = 1\nlang = \"fr\"\n");
        assert_eq!(line, Some(2));
        assert!(reason.contains("\"fr\""), "{reason}");
        assert!(reason.contains("\"en\", \"de\""), "{reason}");
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

    fn dirs<'a>(existing: &'a [&'a str]) -> impl Fn(&Path) -> bool + 'a {
        move |path| existing.iter().any(|dir| Path::new(dir) == path)
    }

    #[test]
    fn a_data_folder_next_to_the_exe_makes_it_portable() {
        let exe = Path::new("/home/user/ncrs/ncrs");
        assert_eq!(
            portable_data_dir_of(exe, dirs(&["/home/user/ncrs/data"])),
            Some(PathBuf::from("/home/user/ncrs/data"))
        );
        assert_eq!(portable_data_dir_of(exe, dirs(&[])), None);
    }

    #[test]
    fn a_data_file_or_a_data_folder_elsewhere_is_not_a_marker() {
        let exe = Path::new("/home/user/ncrs/ncrs");
        // A file named `data` is not a directory, and is_dir says so.
        assert_eq!(portable_data_dir_of(exe, dirs(&[])), None);
        assert_eq!(
            portable_data_dir_of(exe, dirs(&["/home/user/data", "/home/user/ncrs/sub/data"])),
            None
        );
    }

    #[test]
    fn a_real_file_named_data_does_not_count() {
        let dir = std::env::temp_dir().join(format!("ncrs-portable-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("data"), b"x").unwrap();
        let found = portable_data_dir_of(&dir.join("ncrs"), Path::is_dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found, None);
    }

    #[test]
    fn inside_a_macos_app_bundle_a_data_folder_is_ignored() {
        let exe = Path::new("/Applications/ncrs.app/Contents/MacOS/ncrs");
        let everything = |_: &Path| true;
        assert_eq!(portable_data_dir_of(exe, everything), None);
        let plain = Path::new("/home/user/MacOS/ncrs");
        assert!(portable_data_dir_of(plain, everything).is_some());
    }

    #[test]
    fn portable_mode_wins_over_the_profile_directories() {
        let portable = Some(PathBuf::from("/home/user/ncrs/data"));
        let vars = [
            ("APPDATA", r"C:\Users\user\AppData\Roaming"),
            ("XDG_CONFIG_HOME", "/home/user/cfg"),
            ("XDG_DATA_HOME", "/home/user/share"),
            ("HOME", "/home/user"),
        ];
        assert_eq!(
            config_path_in(portable.clone(), env(&vars)),
            Some(PathBuf::from("/home/user/ncrs/data/config.toml"))
        );
        assert_eq!(
            index_path_in(portable, env(&vars)),
            Some(PathBuf::from("/home/user/ncrs/data/index.db"))
        );
    }

    #[test]
    fn config_and_index_share_the_mode() {
        let vars = [
            ("HOME", "/home/user"),
            ("APPDATA", r"C:\Users\user\AppData\Roaming"),
        ];
        let normal_config = config_path_in(None, env(&vars)).unwrap();
        let normal_index = index_path_in(None, env(&vars)).unwrap();
        assert!(!normal_config.starts_with("/home/user/ncrs/data"));
        assert!(!normal_index.starts_with("/home/user/ncrs/data"));

        let portable = Some(PathBuf::from("/home/user/ncrs/data"));
        let config = config_path_in(portable.clone(), env(&vars)).unwrap();
        let index = index_path_in(portable, env(&vars)).unwrap();
        assert_eq!(config.parent(), index.parent());
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

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn linux_index_prefers_an_absolute_xdg_data_home() {
        let found = index_path_from_env(env(&[("XDG_DATA_HOME", "/x/data"), ("HOME", "/h")]));
        assert_eq!(found, Some(PathBuf::from("/x/data/ncrs/index.db")));
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn linux_index_falls_back_to_home_when_xdg_is_relative_or_empty() {
        for xdg in ["rel/data", ""] {
            let found = index_path_from_env(env(&[("XDG_DATA_HOME", xdg), ("HOME", "/h")]));
            assert_eq!(found, Some(PathBuf::from("/h/.local/share/ncrs/index.db")));
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn no_usable_home_means_no_index_path() {
        assert_eq!(index_path_from_env(env(&[])), None);
        assert_eq!(index_path_from_env(env(&[("HOME", "")])), None);
        assert_eq!(index_path_from_env(env(&[("HOME", "rel")])), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_index_lives_in_application_support_and_ignores_xdg() {
        let found = index_path_from_env(env(&[("XDG_DATA_HOME", "/x/data"), ("HOME", "/h")]));
        assert_eq!(
            found,
            Some(PathBuf::from(
                "/h/Library/Application Support/ncrs/index.db"
            ))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_index_uses_appdata() {
        let found = index_path_from_env(env(&[("APPDATA", r"C:\Users\u\AppData\Roaming")]));
        assert_eq!(
            found,
            Some(PathBuf::from(r"C:\Users\u\AppData\Roaming\ncrs\index.db"))
        );
        assert_eq!(index_path_from_env(env(&[("APPDATA", "rel")])), None);
        assert_eq!(index_path_from_env(env(&[])), None);
    }

    fn load_filing(content: &str, home: Option<&Path>) -> Result<Config, ConfigError> {
        let dir = tempfile::tempdir().map_err(|err| ConfigError::Unreadable {
            path: PathBuf::new(),
            reason: err.to_string(),
        })?;
        load_with_home(&write(&dir, content.as_bytes()), home)
    }

    /// A TOML literal string, so Windows backslashes stay as they are.
    fn quoted(path: &Path) -> String {
        format!("'{}'", path.display())
    }

    fn location(path: &str, rest: &str) -> String {
        format!("[[modules.filing.locations]]\npath = {path}\n{rest}\n")
    }

    fn filing_text(enabled: bool, locations: &[String]) -> String {
        let mut text = format!("[modules.filing]\nenabled = {enabled}\n");
        text.extend(locations.iter().map(String::as_str));
        text
    }

    /// 1-based number of the last line that contains `needle`.
    fn last_line_with(text: &str, needle: &str) -> usize {
        text.lines()
            .enumerate()
            .filter(|(_, line)| line.contains(needle))
            .last()
            .unwrap()
            .0
            + 1
    }

    fn filing_error(content: &str, home: Option<&Path>) -> (Option<usize>, String) {
        let err = load_filing(content, home).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err:?}");
        (err.line(), err.reason().to_owned())
    }

    #[test]
    fn a_full_filing_section_is_read() {
        let base = tempfile::tempdir().unwrap();
        let (inbox, archive_a, archive_b) = (
            base.path().join("inbox"),
            base.path().join("archive-a"),
            base.path().join("archive-b"),
        );
        let text = filing_text(
            true,
            &[
                location(&quoted(&inbox), "mode = \"inbox\""),
                location(
                    &quoted(&archive_a),
                    "mode = \"archive\"\nimages = true\ntarget = true",
                ),
                location(&quoted(&archive_b), "mode = \"archive\"\ntarget = false"),
            ],
        );
        let filing = load_filing(&text, None).unwrap().filing;
        assert!(filing.enabled);
        let found: Vec<_> = filing
            .locations
            .iter()
            .map(|l| (l.path.clone(), l.mode, l.images, l.is_target))
            .collect();
        assert_eq!(
            found,
            [
                (inbox, LocationMode::Inbox, false, false),
                (archive_a.clone(), LocationMode::Archive, true, true),
                (archive_b, LocationMode::Archive, false, false),
            ]
        );
        assert_eq!(target_of(&filing).unwrap().path, archive_a);
    }

    #[test]
    fn folders_that_do_not_exist_load_without_error() {
        let base = tempfile::tempdir().unwrap();
        let gone = base.path().join("unplugged").join("archive-a");
        for enabled in [true, false] {
            let text = filing_text(
                enabled,
                &[location(
                    &quoted(&gone),
                    "mode = \"archive\"\ntarget = true",
                )],
            );
            let filing = load_filing(&text, None).unwrap().filing;
            assert_eq!(filing.enabled, enabled);
            assert_eq!(target_of(&filing).unwrap().path, gone);
        }
    }

    #[test]
    fn a_missing_section_means_disabled_without_locations() {
        let filing = load_text("version = 1\n").filing;
        assert!(!filing.enabled);
        assert!(filing.locations.is_empty());
        assert!(target_of(&filing).is_none());
    }

    #[test]
    fn enabled_without_locations_loads() {
        let filing = load_filing("[modules.filing]\nenabled = true\n", None)
            .unwrap()
            .filing;
        assert!(filing.enabled);
        assert!(filing.locations.is_empty());
    }

    #[test]
    fn a_leading_tilde_is_the_home_directory() {
        let home = tempfile::tempdir().unwrap();
        let archive = "mode = \"archive\"\ntarget = true";
        let in_home = filing_text(true, &[location("'~/archive-a'", archive)]);
        let filing = load_filing(&in_home, Some(home.path())).unwrap().filing;
        assert_eq!(filing.locations[0].path, home.path().join("archive-a"));

        let home_itself = filing_text(true, &[location("'~'", archive)]);
        let bare = load_filing(&home_itself, Some(home.path())).unwrap().filing;
        assert_eq!(bare.locations[0].path, home.path());

        let climbing = filing_text(true, &[location("'~/../x'", archive)]);
        let (line, _) = filing_error(&climbing, Some(home.path()));
        assert_eq!(line, Some(4));
    }

    fn load_of(setting: &str) -> Load {
        let text = format!("[modules.filing]\nenabled = true\n{setting}");
        load_filing(&text, None).unwrap().filing.load
    }

    #[test]
    fn the_load_is_gentle_unless_set_and_reads_names_and_numbers() {
        let workers = |count| Load::Workers(NonZeroUsize::new(count).unwrap());
        assert_eq!(load_of(""), Load::Gentle);
        assert_eq!(load_of("load = \"gentle\"\n"), Load::Gentle);
        assert_eq!(load_of("load = \"balanced\"\n"), Load::Balanced);
        assert_eq!(load_of("load = \"full\"\n"), Load::Full);
        assert_eq!(load_of("load = 3\n"), workers(3));
        assert_eq!(load_of("load = 500\n"), workers(500));
    }

    #[test]
    fn a_load_that_is_no_name_and_no_positive_number_names_its_line() {
        for setting in ["load = \"fast\"", "load = 0", "load = -2", "load = true"] {
            let text = format!("[modules.filing]\nenabled = true\n{setting}\n");
            let (line, reason) = filing_error(&text, None);
            assert_eq!(line, Some(3), "{setting}");
            assert!(reason.contains("gentle"), "{setting}: {reason}");
        }
    }

    #[test]
    fn filing_errors_name_the_exact_line() {
        let base = tempfile::tempdir().unwrap();
        let at = |name: &str| quoted(&base.path().join(name));
        let archive = |name: &str| location(&at(name), "mode = \"archive\"\ntarget = true");
        let plain_archive = |name: &str| location(&at(name), "mode = \"archive\"");
        let with_slash = quoted(&base.path().join("archive-a").join(""));
        let cases: Vec<(&str, String, &str)> = vec![
            (
                "relative path",
                filing_text(true, &[location("'archive-a'", "mode = \"archive\"")]),
                "archive-a",
            ),
            (
                "tilde without home",
                filing_text(true, &[location("'~/archive-a'", "mode = \"archive\"")]),
                "~/archive-a",
            ),
            (
                "parent component",
                filing_text(
                    true,
                    &[location(
                        &quoted(&base.path().join("..").join("x")),
                        "mode = \"archive\"",
                    )],
                ),
                "..",
            ),
            (
                "unknown mode",
                filing_text(true, &[location(&at("a"), "mode = \"outbox\"")]),
                "outbox",
            ),
            (
                "unknown field in a location",
                filing_text(
                    true,
                    &[location(&at("a"), "mode = \"archive\"\ncolour = 1")],
                ),
                "colour",
            ),
            (
                "unknown field in the section",
                "[modules.filing]\nenabled = true\nnope = 1\n".to_owned(),
                "nope",
            ),
            (
                "target on an inbox",
                filing_text(
                    true,
                    &[
                        archive("archive-a"),
                        location(&at("inbox"), "mode = \"inbox\"\ntarget = true"),
                    ],
                ),
                "target = true",
            ),
            (
                "two targets",
                filing_text(true, &[archive("archive-a"), archive("archive-b")]),
                "target = true",
            ),
            (
                "no target",
                filing_text(false, &[plain_archive("archive-a")]),
                "[[modules.filing.locations]]",
            ),
            (
                "nested locations",
                filing_text(
                    true,
                    &[archive("archive-a"), plain_archive("archive-a/inside")],
                ),
                "inside",
            ),
            (
                "outer after inner",
                filing_text(
                    true,
                    &[archive("archive-a/inside"), plain_archive("archive-a")],
                ),
                "archive-a'",
            ),
            (
                "the same folder twice",
                filing_text(true, &[archive("archive-a"), plain_archive("archive-a")]),
                "archive-a'",
            ),
            (
                "the same folder twice with a trailing slash",
                filing_text(
                    true,
                    &[
                        archive("archive-a"),
                        location(&with_slash, "mode = \"archive\""),
                    ],
                ),
                "archive-a",
            ),
        ];
        for (name, text, marker) in &cases {
            let (found, reason) = filing_error(text, None);
            let expected = match *name {
                "no target" => 3,
                _ => last_line_with(text, marker),
            };
            assert_eq!(found, Some(expected), "{name}: {reason}\n{text}");
        }
    }

    #[test]
    fn filing_reasons_say_what_is_wrong() {
        let base = tempfile::tempdir().unwrap();
        let path = quoted(&base.path().join("a"));
        let cases = [
            (
                filing_text(true, &[location("'rel'", "mode = \"archive\"")]),
                "must be absolute",
            ),
            (
                filing_text(true, &[location("'~/a'", "mode = \"archive\"")]),
                "no home directory to expand `~`",
            ),
            (
                filing_text(true, &[location(&path, "mode = \"outbox\"")]),
                "inbox",
            ),
            (
                filing_text(true, &[location(&path, "mode = \"outbox\"")]),
                "archive",
            ),
            (
                filing_text(true, &[location(&path, "mode = \"inbox\"\ntarget = true")]),
                "inbox cannot be the target",
            ),
            (
                filing_text(true, &[location(&path, "mode = \"archive\"")]),
                "no location has `target = true`",
            ),
        ];
        for (text, expected) in cases {
            let (_, found) = filing_error(&text, None);
            assert!(found.contains(expected), "{found}");
        }
    }

    /// Three existing folders below a temporary directory, canonical the way
    /// the setup hands them over.
    struct SetupFolders {
        base: tempfile::TempDir,
        inbox: PathBuf,
        archive: PathBuf,
        target: PathBuf,
    }

    fn setup_folders() -> SetupFolders {
        let base = tempfile::tempdir().unwrap();
        let canonical = |name: &str| {
            let folder = base.path().join(name);
            std::fs::create_dir(&folder).unwrap();
            std::fs::canonicalize(folder).unwrap()
        };
        let (inbox, archive, target) = (
            canonical("inbox"),
            canonical("archive"),
            canonical("target"),
        );
        SetupFolders {
            base,
            inbox,
            archive,
            target,
        }
    }

    fn chosen(path: &Path, mode: LocationMode, images: bool, is_target: bool) -> FilingLocation {
        FilingLocation {
            path: path.to_path_buf(),
            mode,
            images,
            is_target,
        }
    }

    fn one_of_each(folders: &SetupFolders) -> Vec<FilingLocation> {
        vec![
            chosen(&folders.inbox, LocationMode::Inbox, true, false),
            chosen(&folders.archive, LocationMode::Archive, false, false),
            chosen(&folders.target, LocationMode::Archive, true, true),
        ]
    }

    #[test]
    fn the_setup_writes_the_locations_and_they_load_back() {
        let folders = setup_folders();
        let path = folders.base.path().join("config.toml");
        std::fs::write(&path, "lang = \"de\"\n").unwrap();
        let chosen = one_of_each(&folders);

        let written = write_filing_locations_with_home(&path, &chosen, None).unwrap();

        assert!(written.filing.enabled);
        assert_eq!(written.filing.locations, chosen);
        assert_eq!(written.lang, Some(Language::German));
        let reloaded = load_with_home(&path, None).unwrap();
        assert_eq!(reloaded, written);
    }

    #[test]
    fn comments_and_other_sections_survive_the_write() {
        let folders = setup_folders();
        let path = folders.base.path().join("config.toml");
        let old_location = folders.base.path().join("old");
        let original = format!(
            "# my settings\nlang = \"en\" # English for now\n\n\
             [open]\n# the viewer I like\nview = [\"viewer\"]\n\n\
             [modules.filing]\n# off until the drive is set up\nenabled = false # later\n\
             load = \"gentle\"\n\n\
             # where documents live\n[[modules.filing.locations]]\npath = {}\nmode = \"archive\"\ntarget = true\n\n\
             [theme]\n# the older look\npreset = \"classic\"\n",
            quoted(&old_location)
        );
        std::fs::write(&path, &original).unwrap();

        let written =
            write_filing_locations_with_home(&path, &one_of_each(&folders), None).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        for kept in [
            "# my settings\n",
            "lang = \"en\" # English for now\n",
            "[open]\n# the viewer I like\nview = [\"viewer\"]\n",
            "# off until the drive is set up\nenabled = true # later\n",
            "load = \"gentle\"\n",
            "# where documents live\n[[modules.filing.locations]]\n",
            "[theme]\n# the older look\npreset = \"classic\"\n",
        ] {
            assert!(text.contains(kept), "{kept:?} is gone from:\n{text}");
        }
        assert!(
            !text.contains(&old_location.display().to_string()),
            "{text}"
        );
        assert_eq!(written.filing.load, Load::Gentle);
        assert_eq!(written.filing.locations, one_of_each(&folders));
        assert_eq!(written.open.view.unwrap().program(), "viewer");
    }

    #[test]
    fn a_filing_section_written_inline_becomes_a_table() {
        let folders = setup_folders();
        let path = folders.base.path().join("config.toml");
        std::fs::write(&path, "modules = { filing = { enabled = false } }\n").unwrap();

        let written =
            write_filing_locations_with_home(&path, &one_of_each(&folders), None).unwrap();

        assert!(written.filing.enabled);
        assert_eq!(written.filing.locations, one_of_each(&folders));
    }

    #[test]
    fn an_invalid_choice_is_rejected_before_writing() {
        let folders = setup_folders();
        let path = folders.base.path().join("config.toml");
        let original = "# untouched\nlang = \"en\"\n";
        std::fs::write(&path, original).unwrap();
        let nested = folders.target.join("inside");
        std::fs::create_dir(&nested).unwrap();
        let cases = [
            (Vec::new(), FilingRuleViolation::NoTarget),
            (
                vec![
                    chosen(&folders.inbox, LocationMode::Inbox, false, false),
                    chosen(&folders.archive, LocationMode::Archive, false, false),
                ],
                FilingRuleViolation::NoTarget,
            ),
            (
                vec![
                    chosen(&folders.archive, LocationMode::Archive, false, true),
                    chosen(&folders.target, LocationMode::Archive, false, true),
                ],
                FilingRuleViolation::SecondTarget { index: 1 },
            ),
            (
                vec![chosen(&folders.inbox, LocationMode::Inbox, false, true)],
                FilingRuleViolation::InboxAsTarget { index: 0 },
            ),
            (
                vec![
                    chosen(&folders.target, LocationMode::Archive, false, true),
                    chosen(&nested, LocationMode::Inbox, false, false),
                ],
                FilingRuleViolation::Overlap {
                    earlier: 0,
                    later: 1,
                },
            ),
        ];
        for (locations, expected) in cases {
            let result = write_filing_locations_with_home(&path, &locations, None);
            assert_eq!(result, Err(FilingWriteError::Rule(expected)));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
    }

    #[test]
    fn a_config_that_does_not_load_is_not_overwritten() {
        let folders = setup_folders();
        let path = folders.base.path().join("config.toml");
        for broken in [
            "[open\nview = [\"viewer\"]\n",
            "lang = \"xx\"\n",
            "colour = 1\n",
        ] {
            std::fs::write(&path, broken).unwrap();
            let result = write_filing_locations_with_home(&path, &one_of_each(&folders), None);
            assert!(
                matches!(
                    result,
                    Err(FilingWriteError::Config(ConfigError::Invalid { .. }))
                ),
                "{broken:?}: {result:?}"
            );
            assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
        }
    }

    #[test]
    fn a_missing_config_is_created_with_its_folder() {
        let folders = setup_folders();
        let path = folders.base.path().join("ncrs").join("config.toml");

        let written =
            write_filing_locations_with_home(&path, &one_of_each(&folders), None).unwrap();

        assert_eq!(written.filing.locations, one_of_each(&folders));
        assert_eq!(load_with_home(&path, None).unwrap(), written);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_config_stays_a_symlink_and_its_target_is_written() {
        let folders = setup_folders();
        let real = folders.base.path().join("dotfiles").join("config.toml");
        std::fs::create_dir(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "lang = \"en\"\n").unwrap();
        let link = folders.base.path().join("config.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        write_filing_locations_with_home(&link, &one_of_each(&folders), None).unwrap();

        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        let reloaded = load_with_home(&real, None).unwrap();
        assert_eq!(reloaded.filing.locations, one_of_each(&folders));
    }

    #[cfg(unix)]
    #[test]
    fn the_permissions_of_the_old_file_are_kept() {
        use std::os::unix::fs::PermissionsExt as _;
        let folders = setup_folders();
        let path = folders.base.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

        write_filing_locations_with_home(&path, &one_of_each(&folders), None).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }
}
