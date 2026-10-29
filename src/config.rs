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
use std::num::{NonZeroI64, NonZeroU32, NonZeroUsize};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use ncrs_intake::{pattern_is_valid, LocationMode};
use ncrs_pipeline::Load;
use serde::de::{Deserializer, Error as _, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_spanned::Spanned;

use crate::fs::OpenKind;
use crate::i18n::Language;
use crate::keymap::{KeyAction, Keymap};
use crate::palette::{ColorRole, Palette, ThemePreset, PRESET_KEY, VSCODE_KEY};
use crate::vscode_theme::VscodeTheme;

const FIRST_VERSION: u32 = 1;

/// The newest version this build reads. A change that would reject or
/// reinterpret what an older file says raises it and converts the older form
/// while loading, in memory; the file is never rewritten to migrate it.
pub const CURRENT_VERSION: u32 = 1;

/// A config is a few lines; anything bigger is not one, and reading it would
/// only delay the start. A VS Code theme file is held to the same limit.
pub const SIZE_LIMIT: u64 = 1024 * 1024;

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
    /// The `[[servers]]` entries as written; `load` checks them into `servers`.
    #[serde(default, rename = "servers")]
    pub server_sections: Vec<ServerSection>,
    /// The saved SFTP servers, in the order of the file.
    #[serde(skip)]
    pub servers: Vec<ServerEntry>,
}

/// One `[[servers]]` entry, unchecked. The password is not part of it: it is
/// asked for, or comes from the keychain.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSection {
    #[serde(default)]
    pub name: Option<Spanned<String>>,
    pub host: Spanned<String>,
    #[serde(default)]
    pub port: Option<Spanned<i64>>,
    pub user: Spanned<String>,
    /// The folder to open first, an absolute path on the server.
    #[serde(default)]
    pub path: Option<Spanned<String>>,
}

/// A saved SFTP server as `load` checked it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEntry {
    /// What the drive menu and the settings call it; the address when none.
    pub name: Option<String>,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub path: Option<String>,
}

impl ServerEntry {
    pub fn server_id(&self) -> crate::fs::remote::ServerId {
        crate::fs::remote::ServerId::new(&self.user, &self.host, self.port)
    }

    /// The name, else `user@host`.
    pub fn label(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| self.server_id().to_string())
    }

    /// The panel path the entry opens: its start folder, else the root.
    pub fn url(&self) -> PathBuf {
        let server = self.server_id();
        match &self.path {
            Some(path) => crate::fs::remote::RemotePath::new(server, path).to_path_buf(),
            None => server.root_path(),
        }
    }
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
    /// Keeps the position of the table for an error about a missing key.
    #[serde(default)]
    pub ai: Option<Spanned<AiSection>>,
    /// Checked by `resolve_filing`: a pattern Hermes cannot render is ignored.
    #[serde(default)]
    pub default_pattern: Option<String>,
}

/// The `[modules.filing.ai]` section, unchecked.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiSection {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub provider: Option<Spanned<AiProviderName>>,
    #[serde(default)]
    pub threads: Option<Spanned<i64>>,
    #[serde(default)]
    pub endpoint: Option<Spanned<String>>,
    #[serde(default)]
    pub model: Option<Spanned<String>>,
}

/// The spelling of `provider` in the file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiProviderName {
    #[default]
    Local,
    Http,
}

/// What the filing module may ask for a document the rules could not read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AiSetting {
    #[default]
    Off,
    /// The model on this machine. `None` leaves the thread count to the module.
    Local {
        threads: Option<NonZeroU32>,
    },
    Http {
        endpoint: String,
        model: String,
    },
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
    pub ai: AiSetting,
    /// The one pattern proposed for documents no rule covers, instead of the
    /// built-in ones; only ever a pattern `pattern_is_valid` accepts.
    pub default_pattern: Option<String>,
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

/// The `version` key: a missing one is the first version. Anything outside
/// `FIRST_VERSION..=CURRENT_VERSION` is refused, so a file from a newer ncrs
/// is not half understood.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version(u32);

impl Default for Version {
    fn default() -> Self {
        Self(FIRST_VERSION)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let number = u32::deserialize(deserializer)?;
        if number < FIRST_VERSION {
            Err(D::Error::custom(format!(
                "version {number} does not exist, the first version is {FIRST_VERSION}"
            )))
        } else if number > CURRENT_VERSION {
            Err(D::Error::custom(format!(
                "config.toml is version {number}, written by a newer ncrs; this ncrs reads up to version {CURRENT_VERSION}"
            )))
        } else {
            Ok(Self(number))
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

    /// The program, then the arguments: the array `[open]` holds.
    pub fn words(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.program.as_str()).chain(self.args.iter().map(String::as_str))
    }

    /// The line as the settings tab shows it: words split at spaces, a word
    /// with a space or a quote in quotes. There are no escapes: a word with
    /// both kinds of quote is written as quoted runs next to each other,
    /// which [`Self::from_text`] joins back into the one word.
    pub fn to_text(&self) -> String {
        self.words().map(quoted_word).collect::<Vec<_>>().join(" ")
    }

    /// The line typed in the settings tab, read as [`Self::to_text`] writes it.
    pub fn from_text(line: &str) -> Result<Self, ProgramLineProblem> {
        Self::parse(split_words(line)?)
    }

    /// Opens a file with the app bundle at `app` through `open -a`, the one
    /// way the sandboxed macOS build may start another program.
    pub fn opening_app(app: &str) -> Self {
        Self::new("open", &["-a", app])
    }

    fn parse(mut words: Vec<String>) -> Result<Self, ProgramLineProblem> {
        if words.is_empty() {
            return Err(ProgramLineProblem::NoProgram);
        }
        let program = words.remove(0);
        if program.trim().is_empty() {
            return Err(ProgramLineProblem::EmptyProgram);
        }
        if program.contains('\0') || words.iter().any(|arg| arg.contains('\0')) {
            return Err(ProgramLineProblem::Nul);
        }
        if depends_on_working_directory(&program) {
            return Err(ProgramLineProblem::RelativePath);
        }
        Ok(Self {
            program,
            args: words,
        })
    }
}

/// Why a program line cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramLineProblem {
    NoProgram,
    EmptyProgram,
    Nul,
    RelativePath,
    /// Only typed text has quotes.
    UnclosedQuote,
}

impl ProgramLineProblem {
    /// The reason in a config error, which is not translated.
    pub fn reason(self) -> &'static str {
        match self {
            Self::NoProgram => "needs at least the program",
            Self::EmptyProgram => "the program is empty",
            Self::Nul => "contains a NUL character",
            Self::RelativePath => "use a program name from PATH or an absolute path",
            Self::UnclosedQuote => "a quote is not closed",
        }
    }
}

const QUOTES: [char; 2] = ['"', '\''];

fn quoted_word(word: &str) -> String {
    let needs_quotes =
        word.is_empty() || word.contains(char::is_whitespace) || word.contains(QUOTES);
    if !needs_quotes {
        return word.to_owned();
    }
    if !word.contains('"') {
        return format!("\"{word}\"");
    }
    if !word.contains('\'') {
        return format!("'{word}'");
    }
    let mut quoted = String::new();
    let mut run_quote: Option<char> = None;
    for c in word.chars() {
        let quote = if c == '"' { '\'' } else { '"' };
        if run_quote != Some(quote) {
            quoted.extend(run_quote);
            quoted.push(quote);
            run_quote = Some(quote);
        }
        quoted.push(c);
    }
    quoted.extend(run_quote);
    quoted
}

/// Words split at whitespace; a run in double or single quotes is one word
/// without its quotes.
fn split_words(line: &str) -> Result<Vec<String>, ProgramLineProblem> {
    let mut words = Vec::new();
    let mut current: Option<String> = None;
    let mut open_quote: Option<char> = None;
    for c in line.chars() {
        match open_quote {
            Some(quote) if c == quote => open_quote = None,
            Some(_) => current.get_or_insert_with(String::new).push(c),
            None if QUOTES.contains(&c) => {
                open_quote = Some(c);
                current.get_or_insert_with(String::new);
            }
            None if c.is_whitespace() => words.extend(current.take()),
            None => current.get_or_insert_with(String::new).push(c),
        }
    }
    if open_quote.is_some() {
        return Err(ProgramLineProblem::UnclosedQuote);
    }
    words.extend(current);
    Ok(words)
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
        Self::parse(words).map_err(|problem| D::Error::custom(problem.reason()))
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

/// Where the remembered session (panels, sort, window) lives, or `None` when
/// the variables that say so are missing. The only place that decides it, so a
/// sandboxed build has one path to change.
pub fn default_state_path() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::default_state_path");
    state_path_in(portable_data_dir(), |name| std::env::var_os(name))
}

fn state_path_in(
    portable: Option<PathBuf>,
    var: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    index_path_in(portable, var).map(|index| index.with_file_name("state.toml"))
}

/// Where the record of what ncrs moved to the trash lives (macOS can not list
/// its trash folder), or `None` when the variables that say so are missing.
pub fn default_journal_path() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::default_journal_path");
    journal_path_in(portable_data_dir(), |name| std::env::var_os(name))
}

fn journal_path_in(
    portable: Option<PathBuf>,
    var: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    index_path_in(portable, var).map(|index| index.with_file_name("journal.toml"))
}

/// Where the host keys of SFTP servers the user confirmed in ncrs are kept, or
/// `None` when the variables that say so are missing. In the data folder next
/// to the index, so the sandboxed build keeps it in its container.
pub fn default_known_hosts_path() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::default_known_hosts_path");
    known_hosts_path_in(portable_data_dir(), |name| std::env::var_os(name))
}

fn known_hosts_path_in(
    portable: Option<PathBuf>,
    var: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    index_path_in(portable, var).map(|index| index.with_file_name("known_hosts"))
}

/// The user's own OpenSSH `known_hosts`, which ncrs only reads. Inside the App
/// Sandbox it can be read once the home folder has been granted.
pub fn default_ssh_known_hosts_path() -> PathBuf {
    forbid_user_machine_in_tests("config::default_ssh_known_hosts_path");
    crate::fs::home_dir().join(".ssh").join("known_hosts")
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

/// Where F3 puts the copy of a file it unpacks from an archive, or `None`
/// when the variables that say so are missing. A cache folder: the copies
/// are temporary and nothing there is worth a backup.
pub fn default_view_copies_dir() -> Option<PathBuf> {
    forbid_user_machine_in_tests("config::default_view_copies_dir");
    view_copies_dir_in(portable_data_dir(), |name| std::env::var_os(name))
}

fn view_copies_dir_in(
    portable: Option<PathBuf>,
    var: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    match portable {
        Some(dir) => Some(dir.join("view")),
        None => view_copies_dir_from_env(var),
    }
}

/// Windows: `%LOCALAPPDATA%\ncrs\view`.
#[cfg(windows)]
fn view_copies_dir_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("LOCALAPPDATA")).map(|dir| dir.join("ncrs").join("view"))
}

/// macOS: `~/Library/Caches/ncrs/view`.
#[cfg(target_os = "macos")]
fn view_copies_dir_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("HOME")).map(|home| {
        home.join("Library")
            .join("Caches")
            .join("ncrs")
            .join("view")
    })
}

/// Other Unix: `$XDG_CACHE_HOME` if set and absolute, else `~/.cache`.
#[cfg(not(any(target_os = "macos", windows)))]
fn view_copies_dir_from_env(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(var("XDG_CACHE_HOME"))
        .or_else(|| absolute(var("HOME")).map(|home| home.join(".cache")))
        .map(|dir| dir.join("ncrs").join("view"))
}

/// The home directory `~` stands for. Like the config path, only an absolute
/// one counts. In the App Sandbox it is the real home, not the container that
/// `$HOME` names: `~` in a config value means the user's folders.
fn home_from_env() -> Option<PathBuf> {
    #[cfg(windows)]
    let variable = "USERPROFILE";
    #[cfg(not(windows))]
    let variable = "HOME";
    crate::fs::preferred_home(crate::fs::sandbox_home(), || {
        absolute(std::env::var_os(variable))
    })
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

/// The config `source` describes, as if read from `config.toml`, for tests
/// that need a loaded one without a file.
#[cfg(test)]
pub fn from_text(source: &str) -> Result<Config, ConfigError> {
    parse_config(source, Path::new("config.toml"), None)
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
    config.servers =
        resolve_servers(&config.server_sections).map_err(|err| ConfigError::Invalid {
            path: path.to_path_buf(),
            line: line_at(source.as_bytes(), err.span.start),
            reason: err.reason,
        })?;
    Ok(config)
}

/// Checks the `[[servers]]` entries as text; nothing is looked up or opened.
fn resolve_servers(sections: &[ServerSection]) -> Result<Vec<ServerEntry>, FilingError> {
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
        ai: resolve_ai(section.ai.as_ref())?,
        default_pattern: section
            .default_pattern
            .as_deref()
            .map(str::trim)
            .filter(|pattern| pattern_is_valid(pattern))
            .map(str::to_owned),
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

fn write_servers_with_home(
    path: &Path,
    servers: &[ServerEntry],
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    write_edited(path, home, |source| with_servers(source, path, servers))
}

/// `source` with `servers` as its `[[servers]]` entries.
fn with_servers(
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
fn write_edited(
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

fn write_vscode_theme_with_home(
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
fn with_vscode_theme(
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

/// One value the settings page changes: the tables it sits in, its key and
/// what it becomes, `None` to drop its line. Only the `*_setting` functions
/// below build one; what they cannot rule out the loader catches before the
/// file is replaced.
struct Setting {
    tables: &'static [&'static str],
    key: &'static str,
    value: Option<toml_edit::Value>,
}

const THEME_TABLE: &str = "theme";
const MODULES_TABLE: &str = "modules";
const FILING_TABLE: &str = "filing";
const AI_TABLE: &str = "ai";
const OPEN_TABLE: &str = "open";
const AI_TABLES: &[&str] = &[MODULES_TABLE, FILING_TABLE, AI_TABLE];

fn theme_preset_setting(preset: ThemePreset) -> Setting {
    Setting {
        tables: &[THEME_TABLE],
        key: PRESET_KEY,
        value: Some(preset.name().into()),
    }
}

fn language_setting(language: Language) -> Setting {
    Setting {
        tables: &[],
        key: "lang",
        value: Some(language.code().into()),
    }
}

fn filing_enabled_setting(enabled: bool) -> Setting {
    Setting {
        tables: &[MODULES_TABLE, FILING_TABLE],
        key: "enabled",
        value: Some(enabled.into()),
    }
}

fn filing_load_setting(load: Load) -> Setting {
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

fn default_pattern_setting(pattern: Option<&str>) -> Setting {
    Setting {
        tables: &[MODULES_TABLE, FILING_TABLE],
        key: "default_pattern",
        value: pattern.map(toml_edit::Value::from),
    }
}

fn ai_enabled_setting(enabled: bool) -> Setting {
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

/// Sets `load` in `[modules.filing]`; same guarantees as [`write_theme_preset`].
pub fn write_filing_load(path: &Path, load: Load) -> Result<Config, FilingWriteError> {
    write_setting(path, filing_load_setting(load), home_from_env().as_deref())
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

/// Sets `enabled` in `[modules.filing.ai]`; same guarantees as [`write_theme_preset`].
pub fn write_ai_enabled(path: &Path, enabled: bool) -> Result<Config, FilingWriteError> {
    write_setting(
        path,
        ai_enabled_setting(enabled),
        home_from_env().as_deref(),
    )
}

fn program_setting(kind: OpenKind, line: Option<&ProgramLine>) -> Setting {
    Setting {
        tables: &[OPEN_TABLE],
        key: match kind {
            OpenKind::View => "view",
            OpenKind::Edit => "edit",
        },
        value: line.map(|words| toml_edit::Value::Array(words.words().collect())),
    }
}

fn vscode_theme_setting(file: Option<&str>) -> Setting {
    Setting {
        tables: &[THEME_TABLE],
        key: VSCODE_KEY,
        value: file.map(toml_edit::Value::from),
    }
}

fn color_setting(role: ColorRole, color: Option<&str>) -> Setting {
    Setting {
        tables: &[THEME_TABLE],
        key: role.name(),
        value: color.map(toml_edit::Value::from),
    }
}

/// The provider and the keys that belong to it; the keys of the other
/// provider are dropped, since the loader refuses them next to this one.
fn ai_provider_settings(choice: &AiProviderChoice) -> [Setting; 4] {
    let (provider, threads, endpoint, model) = match choice {
        AiProviderChoice::Local { threads } => (
            AiProviderName::Local,
            threads.map(|count| NonZeroI64::from(count).get().into()),
            None,
            None,
        ),
        AiProviderChoice::Http { endpoint, model } => (
            AiProviderName::Http,
            None,
            Some(endpoint.as_str().into()),
            Some(model.as_str().into()),
        ),
    };
    let setting = |key, value| Setting {
        tables: AI_TABLES,
        key,
        value,
    };
    [
        setting("provider", Some(provider_text(provider).into())),
        setting("threads", threads),
        setting("endpoint", endpoint),
        setting("model", model),
    ]
}

/// What the settings tab sets in `[modules.filing.ai]` besides `enabled`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiProviderChoice {
    /// `None` leaves the thread count to the module.
    Local {
        threads: Option<NonZeroU32>,
    },
    Http {
        endpoint: String,
        model: String,
    },
}

/// Sets `view` or `edit` in `[open]`, `None` drops it so that the system
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

fn write_setting(
    path: &Path,
    setting: Setting,
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    write_settings(path, &[setting], home)
}

fn write_settings(
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

const KEYS_TABLE: &str = "keys";

/// Writes the lines of `changes` to `[keys]` and leaves every other action,
/// line and comment as written. The result must load, so a clash or a key
/// the keymap refuses leaves the file unchanged and comes back as `Config`.
pub fn write_keys(path: &Path, changes: &[KeyChange]) -> Result<Config, FilingWriteError> {
    write_keys_with_home(path, changes, home_from_env().as_deref())
}

fn write_keys_with_home(
    path: &Path,
    changes: &[KeyChange],
    home: Option<&Path>,
) -> Result<Config, FilingWriteError> {
    write_edited(path, home, |source| with_keys(source, path, changes))
}

/// `source` with the lines of `changes` in `[keys]`. The table is created
/// only for a line to write: dropping lines from a file without one leaves
/// it as it is.
fn with_keys(source: &str, path: &Path, changes: &[KeyChange]) -> Result<String, FilingWriteError> {
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
fn with_settings(
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
fn put_setting(
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

impl Config {
    /// The preset `[theme]` names; the built-in look without one.
    pub fn theme_preset(&self) -> ThemePreset {
        self.theme_text(PRESET_KEY)
            .and_then(ThemePreset::from_name)
            .unwrap_or(ThemePreset::DEFAULT)
    }

    /// The VS Code theme file `[theme]` names, as written.
    pub fn vscode_theme_path(&self) -> Option<&str> {
        self.theme_text(VSCODE_KEY)
    }

    fn theme_text(&self, key: &str) -> Option<&str> {
        self.theme
            .entries
            .iter()
            .find(|entry| entry.key.get_ref() == key)
            .map(|entry| entry.value.get_ref().as_str())
    }
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

/// The config text as an editable document that keeps its comments and layout.
fn parse_document(source: &str, path: &Path) -> Result<toml_edit::DocumentMut, FilingWriteError> {
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

fn not_a_table(path: &Path, key: &str) -> FilingWriteError {
    FilingWriteError::Config(ConfigError::Invalid {
        path: path.to_path_buf(),
        line: None,
        reason: format!("`{key}` is not a table"),
    })
}

/// `source` with `enabled = true` and `locations` in `[modules.filing]`.
fn with_filing_locations(
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
    replace_file_atomically(target, ".config.toml.", content)
        .map_err(|(path, err)| io_error(&path)(err))
}

/// The body of `replace_file`, shared with the session state. The path in the
/// error says which one it belongs to: the folder when the temporary file could
/// not be created, the target otherwise.
pub fn replace_file_atomically(
    target: &Path,
    temp_prefix: &str,
    content: &str,
) -> Result<(), (PathBuf, std::io::Error)> {
    use std::io::Write as _;
    let folder = target.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(temp_prefix)
        .suffix(".tmp")
        .tempfile_in(folder)
        .map_err(|err| (folder.to_path_buf(), err))?;
    if let Ok(previous) = std::fs::metadata(target) {
        temporary
            .as_file()
            .set_permissions(previous.permissions())
            .map_err(|err| (target.to_path_buf(), err))?;
    }
    temporary
        .write_all(content.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|err| (target.to_path_buf(), err))?;
    temporary
        .persist(target)
        .map_err(|err| (target.to_path_buf(), err.error))?;
    Ok(())
}

fn resolve_ai(section: Option<&Spanned<AiSection>>) -> Result<AiSetting, FilingError> {
    let Some(spanned) = section else {
        return Ok(AiSetting::Off);
    };
    let ai = spanned.get_ref();
    let provider = ai
        .provider
        .as_ref()
        .map_or_else(AiProviderName::default, |named| *named.get_ref());
    let only_for = |key: &Option<Spanned<String>>, name: &str, wanted: AiProviderName| match key {
        Some(given) if provider != wanted => Err(FilingError {
            span: given.span(),
            reason: format!(
                "{name} is only for provider = \"{}\"",
                provider_text(wanted)
            ),
        }),
        _ => Ok(()),
    };
    let threads = match &ai.threads {
        Some(given) if provider == AiProviderName::Http => {
            return Err(FilingError {
                span: given.span(),
                reason: "threads is only for provider = \"local\"".to_owned(),
            });
        }
        Some(given) => Some(
            u32::try_from(*given.get_ref())
                .ok()
                .and_then(NonZeroU32::new)
                .ok_or_else(|| FilingError {
                    span: given.span(),
                    reason: "threads must be a number from 1 up".to_owned(),
                })?,
        ),
        None => None,
    };
    only_for(&ai.endpoint, "endpoint", AiProviderName::Http)?;
    only_for(&ai.model, "model", AiProviderName::Http)?;
    if !ai.enabled {
        return Ok(AiSetting::Off);
    }
    match provider {
        AiProviderName::Local => Ok(AiSetting::Local { threads }),
        AiProviderName::Http => {
            let missing = |key: &str| FilingError {
                span: spanned.span(),
                reason: format!("provider = \"http\" needs {key}"),
            };
            let endpoint = ai.endpoint.as_ref().ok_or_else(|| missing("endpoint"))?;
            let model = ai.model.as_ref().ok_or_else(|| missing("model"))?;
            check_ai_endpoint(endpoint.get_ref()).map_err(|problem| FilingError {
                span: endpoint.span(),
                reason: problem.reason(),
            })?;
            Ok(AiSetting::Http {
                endpoint: endpoint.get_ref().clone(),
                model: model.get_ref().clone(),
            })
        }
    }
}

/// Why `endpoint` cannot be the address of `provider = "http"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiEndpointProblem {
    /// Not an address the client takes; the text says why.
    Unusable(String),
    /// Not on this machine.
    NotLocal,
}

impl AiEndpointProblem {
    /// The reason in a config error, which is not translated.
    fn reason(&self) -> String {
        match self {
            Self::Unusable(problem) => format!("endpoint is not usable: {problem}"),
            Self::NotLocal => {
                "endpoint must be on this machine (localhost, 127.0.0.1 or ::1) for now".to_owned()
            }
        }
    }
}

/// The checks `load` makes on `endpoint`, for the settings tab to make them
/// before anything is written.
pub fn check_ai_endpoint(endpoint: &str) -> Result<(), AiEndpointProblem> {
    let unusable =
        |problem: ncrs_ai::EndpointProblem| AiEndpointProblem::Unusable(problem.to_string());
    ncrs_ai::check_endpoint(endpoint).map_err(unusable)?;
    if ncrs_ai::endpoint_is_local(endpoint).map_err(unusable)? {
        Ok(())
    } else {
        Err(AiEndpointProblem::NotLocal)
    }
}

const fn provider_text(provider: AiProviderName) -> &'static str {
    match provider {
        AiProviderName::Local => "local",
        AiProviderName::Http => "http",
    }
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
    fn a_missing_version_means_version_1() {
        let config = load_text("[open]\nview = [\"viewer\"]\n");
        assert_eq!(config.version, Version(1));
        assert_eq!(load_text("version = 1\n").version, Version(1));
    }

    #[test]
    fn version_0_does_not_exist() {
        let (line, reason) = error_line("version = 0\n");
        assert_eq!(line, Some(1));
        assert_eq!(reason, "version 0 does not exist, the first version is 1");
    }

    // Absolute on each platform, or a `~/` path below it is refused.
    #[cfg(windows)]
    const FIXTURE_HOME: &str = r"C:\Users\test";
    #[cfg(not(windows))]
    const FIXTURE_HOME: &str = "/home/test";

    fn load_fixture(text: &str) -> Config {
        parse_config(
            text,
            Path::new("config.toml"),
            Some(Path::new(FIXTURE_HOME)),
        )
        .unwrap()
    }

    fn pressed(
        config: &Config,
        key: iced::keyboard::key::Named,
    ) -> Option<crate::messages::Message> {
        config.keymap.map_key(
            &iced::keyboard::Key::Named(key),
            iced::keyboard::Modifiers::default(),
            crate::keymap::KeyScope::Panel,
        )
    }

    fn assert_the_0_3_0_settings(config: &Config) {
        use crate::messages::{Message, TransferKind};
        use iced::keyboard::key::Named;
        assert_eq!(config.version, Version(1));
        assert_eq!(config.lang, Some(Language::German));
        let view = config.open.view.as_ref().unwrap();
        assert_eq!((view.program(), view.args()), ("open", &[] as &[String]));
        let edit = config.open.edit.as_ref().unwrap();
        assert_eq!(
            (edit.program(), edit.args()),
            ("open", &["-t".to_owned()][..])
        );
        assert_eq!(
            pressed(config, Named::F2),
            Some(Message::Transfer(TransferKind::Copy))
        );
        assert_eq!(pressed(config, Named::F5), None);
        assert_eq!(
            pressed(config, Named::F3),
            Some(Message::OpenExternal(crate::fs::OpenKind::Edit))
        );
        assert_eq!(
            pressed(config, Named::F4),
            Some(Message::OpenExternal(crate::fs::OpenKind::View))
        );
        assert_eq!(config.keymap.bindings_of(KeyAction::Quit).count(), 2);
        assert_eq!(
            config
                .keymap
                .bindings_of(KeyAction::DeletePermanently)
                .count(),
            0
        );
        let classic = ThemePreset::Classic.palette();
        assert_eq!(config.palette.background, classic.background);
        assert_eq!(
            config.palette.directory,
            iced::Color::from_rgb8(0xff, 0xff, 0x55)
        );
        assert_eq!(
            config.palette.tagged,
            iced::Color::from_rgb8(0xaa, 0x55, 0x00)
        );
    }

    fn assert_the_filing_settings(config: &Config) {
        let home = Path::new(FIXTURE_HOME);
        let filing = &config.filing;
        assert!(filing.enabled);
        assert_eq!(filing.load, Load::Balanced);
        assert_eq!(
            filing.locations,
            [
                FilingLocation {
                    path: home.join("Inbox"),
                    mode: LocationMode::Inbox,
                    images: false,
                    is_target: false,
                },
                FilingLocation {
                    path: home.join("Documents/Archive"),
                    mode: LocationMode::Archive,
                    images: true,
                    is_target: true,
                },
            ]
        );
        assert_ne!(filing.ai, AiSetting::Off);
    }

    #[test]
    fn the_config_of_0_3_0_still_loads() {
        let config = load_fixture(include_str!("../tests/fixtures/config/v0.3.0.toml"));
        assert_the_0_3_0_settings(&config);
        assert!(!config.filing.enabled);
    }

    #[test]
    fn the_config_of_0_5_0_still_loads() {
        let config = load_fixture(include_str!("../tests/fixtures/config/v0.5.0.toml"));
        assert_the_0_3_0_settings(&config);
        assert_the_filing_settings(&config);
    }

    #[test]
    fn the_config_of_0_7_0_still_loads() {
        use crate::messages::{Message, TransferKind};
        use iced::keyboard::key::Named;
        let config = load_fixture(include_str!("../tests/fixtures/config/v0.7.0.toml"));
        assert_eq!(config.lang, Some(Language::German));
        assert_eq!(
            pressed(&config, Named::F12),
            Some(Message::Transfer(TransferKind::Copy))
        );
        assert_eq!(pressed(&config, Named::F5), None);
        assert_eq!(
            pressed(&config, Named::F3),
            Some(Message::OpenExternal(crate::fs::OpenKind::Edit))
        );
        assert_eq!(config.keymap.bindings_of(KeyAction::Quit).count(), 2);
        assert_eq!(
            config.palette.directory,
            iced::Color::from_rgb8(0xff, 0xff, 0x55)
        );
        assert_the_filing_settings(&config);
    }

    fn default_pattern_of(content: &str) -> Option<String> {
        load_filing(content, None).unwrap().filing.default_pattern
    }

    #[test]
    fn a_valid_default_pattern_is_read_and_an_invalid_one_is_ignored() {
        assert_eq!(default_pattern_of("[modules.filing]\n"), None);
        assert_eq!(
            default_pattern_of("[modules.filing]\ndefault_pattern = \"{year}/{sender}\"\n"),
            Some("{year}/{sender}".to_owned())
        );
        for invalid in ["{nope}/x", "", "/abs/{year}", "{year"] {
            let text = format!("[modules.filing]\ndefault_pattern = '{invalid}'\n");
            assert_eq!(default_pattern_of(&text), None, "{invalid}");
        }
    }

    #[test]
    fn the_default_pattern_is_written_changed_and_dropped_keeping_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "version = 1\n# my note\nlang = \"en\"\n\n[modules.filing]\nenabled = true\n",
        )
        .unwrap();

        let written = write_default_pattern(&path, Some("{year}/{number}")).unwrap();
        assert_eq!(
            written.filing.default_pattern.as_deref(),
            Some("{year}/{number}")
        );
        let changed = write_default_pattern(&path, Some("{date}_{sender}")).unwrap();
        assert_eq!(
            changed.filing.default_pattern.as_deref(),
            Some("{date}_{sender}")
        );
        let dropped = write_default_pattern(&path, None).unwrap();
        assert_eq!(dropped.filing.default_pattern, None);

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my note"));
        assert!(text.contains("enabled = true"));
        assert!(!text.contains("default_pattern"));
    }

    #[test]
    fn a_writer_refuses_a_file_of_a_newer_version_and_leaves_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        for text in ["version = 2\nlang = \"en\"\n", "version = 0\n"] {
            let path = write(&dir, text.as_bytes());
            let err = write_language(&path, Language::German).unwrap_err();
            assert!(
                matches!(
                    err,
                    FilingWriteError::Config(ConfigError::Invalid { line: Some(1), .. })
                ),
                "{err:?}"
            );
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
    }

    #[test]
    fn a_writer_does_not_touch_the_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, b"# mine\nversion = 1\n");
        write_language(&path, Language::German).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("# mine\nversion = 1\n"), "{written}");
        let without_version = write(&dir, b"lang = \"en\"\n");
        write_language(&without_version, Language::German).unwrap();
        assert!(!std::fs::read_to_string(&without_version)
            .unwrap()
            .contains("version"));
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
                "config.toml is version 2, written by a newer ncrs; this ncrs reads up to version 1",
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
    fn the_state_lives_next_to_the_index() {
        let vars = [
            ("HOME", "/home/user"),
            ("APPDATA", r"C:\Users\user\AppData"),
        ];
        let portable = Some(PathBuf::from("/home/user/ncrs/data"));
        assert_eq!(
            state_path_in(portable, env(&vars)),
            Some(PathBuf::from("/home/user/ncrs/data/state.toml"))
        );
        let index = index_path_in(None, env(&vars)).unwrap();
        let state = state_path_in(None, env(&vars)).unwrap();
        assert_eq!(state.parent(), index.parent());
        assert_eq!(state.file_name().unwrap(), "state.toml");
    }

    #[test]
    fn the_trash_journal_lives_next_to_the_index() {
        let vars = [
            ("HOME", "/home/user"),
            ("APPDATA", r"C:\Users\user\AppData"),
        ];
        let index = index_path_in(None, env(&vars)).unwrap();
        let journal = journal_path_in(None, env(&vars)).unwrap();
        assert_eq!(journal.parent(), index.parent());
        assert_eq!(journal.file_name().unwrap(), "journal.toml");
    }

    #[test]
    fn the_known_hosts_of_ncrs_live_next_to_the_index() {
        let vars = [
            ("HOME", "/home/user"),
            ("APPDATA", r"C:\Users\user\AppData"),
        ];
        let index = index_path_in(None, env(&vars)).unwrap();
        let hosts = known_hosts_path_in(None, env(&vars)).unwrap();
        assert_eq!(hosts.parent(), index.parent());
        assert_eq!(hosts.file_name().unwrap(), "known_hosts");
        let portable = Some(PathBuf::from("data"));
        assert_eq!(
            known_hosts_path_in(portable, env(&vars)),
            Some(PathBuf::from("data").join("known_hosts"))
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

    #[test]
    fn portable_mode_keeps_the_view_copies_in_the_data_folder() {
        let portable = std::env::temp_dir().join("ncrs").join("data");
        let vars = [("HOME", "/home/user"), ("LOCALAPPDATA", "/local")];
        assert_eq!(
            view_copies_dir_in(Some(portable.clone()), env(&vars)),
            Some(portable.join("view"))
        );
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn linux_view_copies_prefer_an_absolute_xdg_cache_home() {
        let found =
            view_copies_dir_from_env(env(&[("XDG_CACHE_HOME", "/x/cache"), ("HOME", "/h")]));
        assert_eq!(found, Some(PathBuf::from("/x/cache/ncrs/view")));
        for xdg in ["rel/cache", ""] {
            let fallback =
                view_copies_dir_from_env(env(&[("XDG_CACHE_HOME", xdg), ("HOME", "/h")]));
            assert_eq!(fallback, Some(PathBuf::from("/h/.cache/ncrs/view")));
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn no_usable_home_means_no_view_copies_dir() {
        assert_eq!(view_copies_dir_from_env(env(&[])), None);
        assert_eq!(view_copies_dir_from_env(env(&[("HOME", "")])), None);
        assert_eq!(view_copies_dir_from_env(env(&[("HOME", "rel")])), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_view_copies_live_in_library_caches_and_ignore_xdg() {
        let found =
            view_copies_dir_from_env(env(&[("XDG_CACHE_HOME", "/x/cache"), ("HOME", "/h")]));
        assert_eq!(found, Some(PathBuf::from("/h/Library/Caches/ncrs/view")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_view_copies_use_local_appdata() {
        let found = view_copies_dir_from_env(env(&[("LOCALAPPDATA", r"C:\Users\u\AppData\Local")]));
        assert_eq!(
            found,
            Some(PathBuf::from(r"C:\Users\u\AppData\Local\ncrs\view"))
        );
        assert_eq!(
            view_copies_dir_from_env(env(&[("LOCALAPPDATA", "rel")])),
            None
        );
        assert_eq!(view_copies_dir_from_env(env(&[])), None);
    }

    fn load_filing(content: &str, home: Option<&Path>) -> Result<Config, ConfigError> {
        let dir = tempfile::tempdir().map_err(|err| ConfigError::Unreadable {
            path: PathBuf::new(),
            reason: err.to_string(),
        })?;
        load_with_home(&write(&dir, content.as_bytes()), home)
    }

    #[test]
    fn a_vscode_theme_is_found_under_the_home_folder_by_tilde() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("themes")).unwrap();
        std::fs::write(
            home.path().join("themes/t.json"),
            r##"{"colors": {"editor.background": "#112233"}}"##,
        )
        .unwrap();
        let config = load_filing(
            "[open]\n\n[theme]\nvscode = \"~/themes/t.json\"\n",
            Some(home.path()),
        )
        .unwrap();
        assert_eq!(config.vscode_warning, None);
        assert_eq!(
            config.palette.background,
            iced::Color::from_rgb8(0x11, 0x22, 0x33)
        );
    }

    #[test]
    fn a_tilde_without_a_home_folder_is_a_warning_and_not_an_error() {
        let config = load_filing("[open]\n\n[theme]\nvscode = \"~/t.json\"\n", None).unwrap();
        assert!(config.vscode_warning.is_some());
        assert_eq!(
            config.palette.background,
            crate::palette::ThemePreset::DEFAULT.palette().background
        );
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

    #[test]
    fn a_tilde_is_the_real_home_when_the_sandbox_knows_it() {
        let real = tempfile::tempdir().unwrap();
        let container = tempfile::tempdir().unwrap();
        let home = crate::fs::preferred_home(Some(real.path().to_path_buf()), || {
            Some(container.path().to_path_buf())
        });
        let archive = "mode = \"archive\"\ntarget = true";
        let text = filing_text(true, &[location("'~/archive-a'", archive)]);

        let filing = load_filing(&text, home.as_deref()).unwrap().filing;

        assert_eq!(filing.locations[0].path, real.path().join("archive-a"));
    }

    fn entry(name: Option<&str>, host: &str, port: u16, path: Option<&str>) -> ServerEntry {
        ServerEntry {
            name: name.map(str::to_owned),
            host: host.to_owned(),
            port,
            user: "marc".to_owned(),
            path: path.map(str::to_owned),
        }
    }

    #[test]
    fn servers_section_round_trips_and_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            &dir,
            b"# my config\nlang = \"en\"\n\n# the servers\n[[servers]]\nname = \"Work\"\nhost = \"files.example.org\"\nuser = \"marc\"\n\n[theme]\npreset = \"classic\"\n",
        );
        let loaded = load_with_home(&path, None).unwrap();
        assert_eq!(
            loaded.servers,
            [entry(Some("Work"), "files.example.org", 22, None)]
        );

        let wanted = vec![
            entry(Some("Work"), "files.example.org", 22, None),
            entry(None, "backup.example.org", 2222, Some("/srv/backup")),
        ];
        let written = write_servers_with_home(&path, &wanted, None).unwrap();

        assert_eq!(written.servers, wanted);
        let text = std::fs::read_to_string(&path).unwrap();
        for kept in [
            "# my config",
            "# the servers",
            "preset = \"classic\"",
            "lang = \"en\"",
        ] {
            assert!(text.contains(kept), "{kept} is gone from\n{text}");
        }
        assert!(text.contains("port = 2222") && !text.contains("port = 22\n"));
        assert_eq!(load_with_home(&path, None).unwrap().servers, wanted);

        let emptied = write_servers_with_home(&path, &[], None).unwrap();
        assert!(emptied.servers.is_empty());
        let left = std::fs::read_to_string(&path).unwrap();
        assert!(
            !left.contains("servers") && left.contains("# my config"),
            "{left}"
        );
    }

    #[test]
    fn writing_servers_to_a_missing_file_creates_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let wanted = vec![entry(None, "h.example.org", 22, None)];

        let written = write_servers_with_home(&path, &wanted, None).unwrap();

        assert_eq!(written.servers, wanted);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("[[servers]]"));
    }

    #[test]
    fn server_with_bad_port_or_relative_path_is_a_config_error() {
        let with = |host: &str, user: &str, extra: &str| {
            format!("[[servers]]\nhost = \"{host}\"\nuser = \"{user}\"\n{extra}\n")
        };
        let port_text = "port must be a number from 1 to 65535";
        let bad = [
            (with("h", "u", "port = 0"), port_text),
            (with("h", "u", "port = 70000"), port_text),
            (with("h", "u", "port = -1"), port_text),
            (
                with("h", "u", "path = \"srv/files\""),
                "path must be absolute",
            ),
            (with("a b", "u", ""), "host must not be empty"),
            (with("", "u", ""), "host must not be empty"),
            (with("a/b", "u", ""), "host must not be empty"),
            (with("h", "a@b", ""), "user must not be empty"),
            (with("h", "a:secret", ""), "user must not be empty"),
        ];
        for (text, reason) in bad {
            let (line, found) = filing_error(&text, None);
            assert!(found.contains(reason), "{text}: {found}");
            assert!(line.is_some(), "{text}");
        }
    }

    #[test]
    fn a_server_without_host_or_user_is_a_config_error() {
        for text in ["[[servers]]\nuser = \"u\"\n", "[[servers]]\nhost = \"h\"\n"] {
            let (line, reason) = filing_error(text, None);
            assert!(reason.contains("missing field"), "{reason}");
            assert!(line.is_some());
        }
    }

    #[test]
    fn a_password_has_no_place_in_the_config() {
        let text = "[[servers]]\nhost = \"h\"\nuser = \"u\"\npassword = \"secret\"\n";
        let (_, reason) = filing_error(text, None);
        assert!(reason.contains("unknown field"), "{reason}");
        assert!(!reason.contains("secret"));
    }

    #[test]
    fn a_saved_server_opens_at_its_start_folder_or_the_root() {
        assert_eq!(
            entry(None, "H.example.org", 22, Some("/srv")).url(),
            PathBuf::from("sftp://marc@h.example.org/srv")
        );
        assert_eq!(
            entry(None, "h.example.org", 2222, None).url(),
            PathBuf::from("sftp://marc@h.example.org:2222/")
        );
        assert_eq!(entry(Some("Work"), "h", 22, None).label(), "Work");
        assert_eq!(entry(None, "h", 22, None).label(), "marc@h");
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

    fn ai_setting(table: &str) -> AiSetting {
        load_filing(&format!("[modules.filing.ai]\n{table}"), None)
            .unwrap()
            .filing
            .ai
    }

    #[test]
    fn without_an_ai_table_or_when_not_enabled_the_ai_is_off() {
        assert_eq!(
            load_filing("[modules.filing]\nenabled = true\n", None)
                .unwrap()
                .filing
                .ai,
            AiSetting::Off
        );
        assert_eq!(ai_setting(""), AiSetting::Off);
        assert_eq!(ai_setting("provider = \"local\"\n"), AiSetting::Off);
        assert_eq!(ai_setting("enabled = false\nthreads = 2\n"), AiSetting::Off);
    }

    #[test]
    fn the_local_provider_is_the_default_and_threads_are_optional() {
        assert_eq!(
            ai_setting("enabled = true\n"),
            AiSetting::Local { threads: None }
        );
        assert_eq!(
            ai_setting("enabled = true\nprovider = \"local\"\nthreads = 3\n"),
            AiSetting::Local {
                threads: NonZeroU32::new(3)
            }
        );
    }

    #[test]
    fn the_http_provider_needs_an_endpoint_and_a_model() {
        assert_eq!(
            ai_setting(
                "enabled = true\nprovider = \"http\"\nendpoint = \"http://localhost:8080/v1\"\nmodel = \"qwen\"\n"
            ),
            AiSetting::Http {
                endpoint: "http://localhost:8080/v1".to_owned(),
                model: "qwen".to_owned()
            }
        );
    }

    #[test]
    fn the_http_provider_accepts_every_way_to_say_this_machine() {
        for endpoint in [
            "http://localhost:8080",
            "http://127.0.0.1:8080/v1",
            "http://[::1]:8080",
            "https://localhost:8443",
        ] {
            let text = format!(
                "enabled = true\nprovider = \"http\"\nmodel = \"q\"\nendpoint = \"{endpoint}\"\n"
            );
            assert!(
                matches!(ai_setting(&text), AiSetting::Http { .. }),
                "{endpoint}"
            );
        }
    }

    #[test]
    fn ai_errors_name_the_exact_line() {
        let cases: [(&str, &str, usize, &str); 9] = [
            (
                "http without an endpoint",
                "enabled = true\nprovider = \"http\"\nmodel = \"qwen\"\n",
                1,
                "needs endpoint",
            ),
            (
                "http without a model",
                "enabled = true\nprovider = \"http\"\nendpoint = \"http://localhost:8080\"\n",
                1,
                "needs model",
            ),
            (
                "an endpoint that is not accepted",
                "enabled = true\nprovider = \"http\"\nmodel = \"q\"\nendpoint = \"http://example.com/v1\"\n",
                5,
                "needs https",
            ),
            (
                "an endpoint on another machine",
                "enabled = true\nprovider = \"http\"\nmodel = \"q\"\nendpoint = \"https://api.example.com/v1\"\n",
                5,
                "must be on this machine",
            ),
            (
                "threads of zero",
                "enabled = true\nthreads = 0\n",
                3,
                "from 1 up",
            ),
            (
                "threads for http",
                "provider = \"http\"\nthreads = 2\n",
                3,
                "only for provider = \"local\"",
            ),
            (
                "an endpoint for the local provider",
                "enabled = true\nendpoint = \"http://localhost:8080\"\n",
                3,
                "only for provider = \"http\"",
            ),
            (
                "a model for the local provider",
                "enabled = true\nmodel = \"qwen\"\n",
                3,
                "only for provider = \"http\"",
            ),
            (
                "an unknown provider",
                "enabled = true\nprovider = \"cloud\"\n",
                3,
                "cloud",
            ),
        ];
        for (name, table, expected_line, expected_reason) in cases {
            let (line, reason) = filing_error(&format!("[modules.filing.ai]\n{table}"), None);
            assert_eq!(line, Some(expected_line), "{name}");
            assert!(reason.contains(expected_reason), "{name}: {reason}");
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

    fn theme_file(base: &tempfile::TempDir, relative: &str, background: &str) {
        let file = base.path().join(relative);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            file,
            format!(r#"{{"colors": {{"editor.background": "{background}"}}}}"#),
        )
        .unwrap();
    }

    #[test]
    fn the_theme_is_written_and_its_palette_loads() {
        let base = tempfile::tempdir().unwrap();
        theme_file(&base, "themes/example.dark/dark.json", "#112233");
        let path = base.path().join("config.toml");
        std::fs::write(&path, "lang = \"de\"\n").unwrap();

        let written =
            write_vscode_theme_with_home(&path, "themes/example.dark/dark.json", None).unwrap();

        assert_eq!(written.vscode_warning, None);
        assert_eq!(
            written.palette.background,
            iced::Color::from_rgb8(0x11, 0x22, 0x33)
        );
        assert_eq!(written.lang, Some(Language::German));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("[theme]\nvscode = \"themes/example.dark/dark.json\"\n"),
            "{text}"
        );
        assert_eq!(load_with_home(&path, None).unwrap(), written);
    }

    #[test]
    fn an_existing_vscode_setting_is_replaced_and_the_rest_stays() {
        let base = tempfile::tempdir().unwrap();
        theme_file(&base, "themes/example.dark/dark.json", "#112233");
        let path = base.path().join("config.toml");
        std::fs::write(
            &path,
            "# my settings\nlang = \"en\"\n\n[theme]\n# the look\npreset = \"classic\"\n\
             vscode = \"old/theme.json\" # before\n\n[open]\nview = [\"viewer\"]\n",
        )
        .unwrap();

        let written =
            write_vscode_theme_with_home(&path, "themes/example.dark/dark.json", None).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        for kept in [
            "# my settings\nlang = \"en\"\n",
            "[theme]\n# the look\npreset = \"classic\"\n",
            "vscode = \"themes/example.dark/dark.json\" # before\n",
            "[open]\nview = [\"viewer\"]\n",
        ] {
            assert!(text.contains(kept), "{kept:?} is gone from:\n{text}");
        }
        assert!(!text.contains("old/theme.json"), "{text}");
        assert_eq!(written.open.view.unwrap().program(), "viewer");
    }

    #[test]
    fn a_theme_written_inline_becomes_a_table() {
        let base = tempfile::tempdir().unwrap();
        theme_file(&base, "t.json", "#112233");
        let path = base.path().join("config.toml");
        std::fs::write(&path, "theme = { preset = \"classic\" }\n").unwrap();

        let written = write_vscode_theme_with_home(&path, "t.json", None).unwrap();

        assert_eq!(written.vscode_warning, None);
        assert_eq!(
            written.palette.background,
            iced::Color::from_rgb8(0x11, 0x22, 0x33)
        );
    }

    #[test]
    fn a_missing_config_is_created_for_the_theme() {
        let base = tempfile::tempdir().unwrap();
        theme_file(&base, "ncrs/themes/t.json", "#112233");
        let path = base.path().join("ncrs").join("config.toml");

        let written = write_vscode_theme_with_home(&path, "themes/t.json", None).unwrap();

        assert_eq!(load_with_home(&path, None).unwrap(), written);
        assert_eq!(written.vscode_warning, None);
    }

    #[test]
    fn a_config_that_does_not_load_is_not_overwritten_by_the_theme() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        let broken = "[open\nview = [\"viewer\"]\n";
        std::fs::write(&path, broken).unwrap();

        let result = write_vscode_theme_with_home(&path, "t.json", None);

        assert!(
            matches!(
                result,
                Err(FilingWriteError::Config(ConfigError::Invalid { .. }))
            ),
            "{result:?}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    }

    #[test]
    fn a_theme_file_that_is_missing_is_a_warning_in_the_written_config() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");

        let written = write_vscode_theme_with_home(&path, "themes/gone.json", None).unwrap();

        assert!(written.vscode_warning.is_some());
    }
    fn lines_of(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn the_preset_is_written_in_place_and_the_comments_stay() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(
            &path,
            "# mine\nlang = \"en\"\n\n[theme]\n# the look\npreset = \"classic\" # old\nbackground = \"#112233\"\n",
        )
        .unwrap();

        let written =
            write_setting(&path, theme_preset_setting(ThemePreset::Dracula), None).unwrap();

        let text = lines_of(&path);
        for kept in [
            "# mine\nlang = \"en\"\n",
            "# the look\npreset = \"dracula\" # old\n",
            "background = \"#112233\"\n",
        ] {
            assert!(text.contains(kept), "{kept:?} is gone from:\n{text}");
        }
        assert_eq!(written.theme_preset(), ThemePreset::Dracula);
        assert_eq!(load_with_home(&path, None).unwrap(), written);
    }

    #[test]
    fn a_missing_theme_table_is_created_for_the_preset() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(&path, "lang = \"de\"\n").unwrap();

        let written = write_setting(&path, theme_preset_setting(ThemePreset::Teal), None).unwrap();

        assert!(lines_of(&path).contains("[theme]\npreset = \"default\"\n"));
        assert_eq!(written.theme_preset(), ThemePreset::Teal);
        assert_eq!(written.lang, Some(Language::German));
    }

    #[test]
    fn the_language_is_written_at_the_top_and_round_trips() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(&path, "# first\n[theme]\npreset = \"classic\"\n").unwrap();

        let written = write_setting(&path, language_setting(Language::German), None).unwrap();

        let text = lines_of(&path);
        assert!(text.starts_with("lang = \"de\"\n"), "{text}");
        assert!(
            text.contains("# first\n[theme]\npreset = \"classic\"\n"),
            "{text}"
        );
        assert_eq!(written.lang, Some(Language::German));
        let again = write_setting(&path, language_setting(Language::English), None).unwrap();
        assert_eq!(again.lang, Some(Language::English));
        assert_eq!(lines_of(&path).matches("lang").count(), 1);
    }

    #[test]
    fn filing_enabled_creates_its_table_and_keeps_the_rest() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(&path, "[open]\nview = [\"viewer\"] # mine\n").unwrap();

        let written = write_setting(&path, filing_enabled_setting(true), None).unwrap();

        let text = lines_of(&path);
        assert!(text.contains("view = [\"viewer\"] # mine\n"), "{text}");
        assert!(
            text.contains("[modules.filing]\nenabled = true\n"),
            "{text}"
        );
        assert!(written.filing.enabled);
        let off = write_setting(&path, filing_enabled_setting(false), None).unwrap();
        assert!(!off.filing.enabled);
    }

    #[test]
    fn the_load_is_written_as_a_name_or_a_number_and_reads_back() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(
            &path,
            "[modules.filing]\nenabled = false\nload = \"gentle\" # easy\n",
        )
        .unwrap();
        let workers = Load::Workers(NonZeroUsize::new(3).unwrap());

        for load in [Load::Full, Load::Balanced, workers, Load::Gentle] {
            let written = write_setting(&path, filing_load_setting(load), None).unwrap();
            assert_eq!(written.filing.load, load);
        }
        let text = lines_of(&path);
        assert!(text.contains("load = \"gentle\" # easy\n"), "{text}");
        assert!(text.contains("enabled = false\n"), "{text}");
    }

    #[test]
    fn ai_enabled_creates_the_nested_table_and_keeps_the_filing_keys() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(&path, "[modules.filing]\nenabled = true\nload = \"full\"\n").unwrap();

        let written = write_setting(&path, ai_enabled_setting(true), None).unwrap();

        let text = lines_of(&path);
        assert!(text.contains("load = \"full\"\n"), "{text}");
        assert!(
            text.contains("[modules.filing.ai]\nenabled = true\n"),
            "{text}"
        );
        assert!(matches!(written.filing.ai, AiSetting::Local { .. }));
        let off = write_setting(&path, ai_enabled_setting(false), None).unwrap();
        assert_eq!(off.filing.ai, AiSetting::Off);
    }

    #[test]
    fn a_program_line_reads_back_as_it_is_shown() {
        let line = ProgramLine::from_text("  code --wait  \"{path}:{line}\" ").unwrap();
        assert_eq!(line, ProgramLine::new("code", &["--wait", "{path}:{line}"]));
        let spaced_program = if cfg!(windows) {
            r"C:\Program Files\Sublime Text\subl.exe"
        } else {
            "/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl"
        };
        let spaced = ProgramLine::new(spaced_program, &["", "it's"]);
        assert_eq!(ProgramLine::from_text(&spaced.to_text()), Ok(spaced));
        let quoted_program = if cfg!(windows) {
            r"C:\my editor\run.exe"
        } else {
            "/opt/my editor/run"
        };
        assert_eq!(
            ProgramLine::from_text(&format!("'{quoted_program}' -n")),
            Ok(ProgramLine::new(quoted_program, &["-n"]))
        );
    }

    #[test]
    fn a_word_with_both_quotes_and_a_space_reads_back_the_same() {
        let mixed = ProgramLine::new(
            if cfg!(windows) {
                r"C:\Tools\printf.exe"
            } else {
                "/usr/bin/printf"
            },
            &["say \"it's\" now", "\"'", "'\"", "a\"\"b'c d", "\"\"", "''"],
        );
        let shown = mixed.to_text();
        assert_eq!(ProgramLine::from_text(&shown), Ok(mixed), "{shown}");
    }

    #[test]
    fn a_program_line_the_loader_would_refuse_is_refused_as_text() {
        assert_eq!(
            ProgramLine::from_text("   "),
            Err(ProgramLineProblem::NoProgram)
        );
        assert_eq!(
            ProgramLine::from_text("\"\" -x"),
            Err(ProgramLineProblem::EmptyProgram)
        );
        assert_eq!(
            ProgramLine::from_text("bin/edit"),
            Err(ProgramLineProblem::RelativePath)
        );
        assert_eq!(
            ProgramLine::from_text("vim \"x"),
            Err(ProgramLineProblem::UnclosedQuote)
        );
    }

    #[test]
    fn a_program_is_written_to_open_and_dropped_again() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(&path, "lang = \"en\"\n").unwrap();
        let line = ProgramLine::opening_app("/Applications/TextEdit.app");

        let written =
            write_setting(&path, program_setting(OpenKind::Edit, Some(&line)), None).unwrap();

        assert_eq!(written.open.edit, Some(line));
        let text = lines_of(&path);
        assert!(
            text.contains("[open]\nedit = [\"open\", \"-a\", \"/Applications/TextEdit.app\"]\n"),
            "{text}"
        );
        let dropped = write_setting(&path, program_setting(OpenKind::Edit, None), None).unwrap();
        assert_eq!(dropped.open, OpenPrograms::default());
        assert!(lines_of(&path).starts_with("lang = \"en\"\n"));
    }

    #[test]
    fn a_dropped_value_creates_no_table_and_leaves_a_file_without_it_alone() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        let before = "# mine\nlang = \"de\"\n";
        std::fs::write(&path, before).unwrap();

        write_setting(&path, color_setting(ColorRole::Accent, None), None).unwrap();
        write_setting(&path, vscode_theme_setting(None), None).unwrap();

        assert_eq!(lines_of(&path), before);
    }

    #[test]
    fn a_color_is_set_and_dropped_and_a_bad_one_never_reaches_the_file() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(&path, "[theme]\npreset = \"classic\" # base\n").unwrap();

        let written = write_setting(
            &path,
            color_setting(ColorRole::Accent, Some("#ff0000")),
            None,
        )
        .unwrap();
        assert_eq!(
            written.palette.color(ColorRole::Accent),
            iced::Color::from_rgb8(0xFF, 0, 0)
        );
        let before = lines_of(&path);
        let refused = write_setting(&path, color_setting(ColorRole::Accent, Some("red")), None);
        assert!(matches!(refused, Err(FilingWriteError::Config(_))));
        assert_eq!(lines_of(&path), before);

        let dropped = write_setting(&path, color_setting(ColorRole::Accent, None), None).unwrap();
        assert_eq!(dropped.palette, ThemePreset::Classic.palette());
        assert_eq!(lines_of(&path), "[theme]\npreset = \"classic\" # base\n");
    }

    #[test]
    fn the_vscode_theme_is_dropped_from_its_section() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(
            &path,
            "[theme]\nvscode = \"gone.json\"\naccent = \"#123\"\n",
        )
        .unwrap();

        let written = write_setting(&path, vscode_theme_setting(None), None).unwrap();

        assert_eq!(written.vscode_theme_path(), None);
        assert_eq!(lines_of(&path), "[theme]\naccent = \"#123\"\n");
    }

    #[test]
    fn switching_the_ai_provider_takes_the_keys_of_the_other_one_away() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(
            &path,
            "[modules.filing.ai]\nenabled = true\nprovider = \"local\"\nthreads = 2\n",
        )
        .unwrap();
        let http = AiProviderChoice::Http {
            endpoint: "http://127.0.0.1:11434".to_owned(),
            model: "llama".to_owned(),
        };

        let written = write_settings(&path, &ai_provider_settings(&http), None).unwrap();

        assert_eq!(
            written.filing.ai,
            AiSetting::Http {
                endpoint: "http://127.0.0.1:11434".to_owned(),
                model: "llama".to_owned(),
            }
        );
        assert!(!lines_of(&path).contains("threads"));
        let local = AiProviderChoice::Local {
            threads: NonZeroU32::new(3),
        };
        let back = write_settings(&path, &ai_provider_settings(&local), None).unwrap();
        assert_eq!(
            back.filing.ai,
            AiSetting::Local {
                threads: NonZeroU32::new(3)
            }
        );
        let text = lines_of(&path);
        assert!(
            !text.contains("endpoint") && !text.contains("model"),
            "{text}"
        );
    }

    #[test]
    fn an_inline_ai_table_loses_a_dropped_key_too() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(
            &path,
            "[modules.filing]\nai = { provider = \"http\", endpoint = \"http://localhost:1\", model = \"m\" }\n",
        )
        .unwrap();
        let local = AiProviderChoice::Local { threads: None };

        let written = write_settings(&path, &ai_provider_settings(&local), None).unwrap();

        assert_eq!(written.filing.ai, AiSetting::Off);
        let text = lines_of(&path);
        assert!(
            !text.contains("endpoint") && !text.contains("model"),
            "{text}"
        );
    }

    #[test]
    fn an_endpoint_off_this_machine_is_refused_before_the_write() {
        assert_eq!(check_ai_endpoint("http://localhost:11434"), Ok(()));
        assert_eq!(
            check_ai_endpoint("https://example.com:11434"),
            Err(AiEndpointProblem::NotLocal)
        );
        assert!(matches!(
            check_ai_endpoint("ftp://localhost"),
            Err(AiEndpointProblem::Unusable(_))
        ));
    }

    fn key_change(action: KeyAction, chords: Option<&[&str]>) -> KeyChange {
        KeyChange {
            action,
            chords: chords.map(|list| list.iter().map(|chord| (*chord).to_owned()).collect()),
        }
    }

    #[test]
    fn written_keys_replace_only_their_lines_and_keep_the_comments() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(
            &path,
            "lang = \"de\"\n\n# my keys\n[keys]\nquit = \"f10\" # old habit\nview = [\"f3\"]\n",
        )
        .unwrap();

        let written = write_keys_with_home(
            &path,
            &[
                key_change(KeyAction::Quit, Some(&["primary+q", "f10"])),
                key_change(KeyAction::View, None),
                key_change(KeyAction::Edit, Some(&["f4", "shift+f4"])),
            ],
            None,
        )
        .unwrap();

        let text = lines_of(&path);
        assert_eq!(
            text,
            "lang = \"de\"\n\n# my keys\n[keys]\nquit = [\"primary+q\", \"f10\"] # old habit\nedit = [\"f4\", \"shift+f4\"]\n"
        );
        let names: Vec<&str> = written
            .keys
            .entries
            .iter()
            .map(|entry| entry.action.get_ref().as_str())
            .collect();
        assert_eq!(names, ["quit", "edit"]);
    }

    #[test]
    fn dropping_keys_from_a_file_without_them_adds_no_table() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(&path, "lang = \"en\"\n").unwrap();
        write_keys_with_home(&path, &[key_change(KeyAction::Quit, None)], None).unwrap();
        assert_eq!(lines_of(&path), "lang = \"en\"\n");
    }

    #[test]
    fn keys_that_clash_are_not_written() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        let text = "[keys]\nquit = \"f10\"\n";
        std::fs::write(&path, text).unwrap();
        let result =
            write_keys_with_home(&path, &[key_change(KeyAction::View, Some(&["f10"]))], None);
        assert!(
            matches!(
                result,
                Err(FilingWriteError::Config(ConfigError::Invalid { .. }))
            ),
            "{result:?}"
        );
        assert_eq!(lines_of(&path), text);
    }

    #[test]
    fn a_setting_is_created_with_its_config_and_a_broken_config_is_left_alone() {
        let base = tempfile::tempdir().unwrap();
        let fresh = base.path().join("ncrs").join("config.toml");
        let written = write_setting(&fresh, language_setting(Language::English), None).unwrap();
        assert_eq!(written.lang, Some(Language::English));

        let broken = base.path().join("broken.toml");
        let text = "[open\nview = [\"viewer\"]\n";
        std::fs::write(&broken, text).unwrap();
        let result = write_setting(&broken, filing_enabled_setting(true), None);
        assert!(
            matches!(
                result,
                Err(FilingWriteError::Config(ConfigError::Invalid { .. }))
            ),
            "{result:?}"
        );
        assert_eq!(lines_of(&broken), text);
    }

    #[test]
    fn a_table_that_is_not_one_is_reported_not_replaced() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("config.toml");
        std::fs::write(&path, "theme = \"classic\"\n").unwrap();

        let result = write_setting(&path, theme_preset_setting(ThemePreset::Teal), None);

        assert!(
            matches!(result, Err(FilingWriteError::Config(_))),
            "{result:?}"
        );
        assert_eq!(lines_of(&path), "theme = \"classic\"\n");
    }
}
