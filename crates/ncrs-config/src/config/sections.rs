use super::*;

/// One `[[kinds]]` entry, unchecked: an ID (new, or a built-in one to extend),
/// the words it is shown as and further words that name it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindSection {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub de: Option<String>,
    #[serde(default)]
    pub en: Option<String>,
    #[serde(default)]
    pub words: Vec<String>,
}

/// The `[file_list_tags]` section, unchecked.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileListTagsSection {
    #[serde(default)]
    pub show: Option<bool>,
    /// Checked by `resolve_file_list_tags`: a name that is no group is dropped.
    #[serde(default)]
    pub groups: Option<Vec<String>>,
    /// Older files list tag names here; they live in the index now, so the
    /// key is accepted, whatever it holds, and never written.
    #[serde(default, rename = "only")]
    pub _only: Option<serde::de::IgnoredAny>,
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
    pub fn server_id(&self) -> ncrs_fs::remote::ServerId {
        ncrs_fs::remote::ServerId::new(&self.user, &self.host, self.port)
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
            Some(path) => ncrs_fs::remote::RemotePath::new(server, path).to_path_buf(),
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
    /// Checked by `resolve_filing`: empty entries are dropped.
    #[serde(default)]
    pub own_names: Vec<String>,
    /// Checked by `resolve_filing`: an entry that is no IBAN is dropped.
    #[serde(default)]
    pub own_ibans: Vec<String>,
    #[serde(default)]
    pub own_tax_ids: Vec<String>,
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
    /// Names and companies of the user; never taken for a sender.
    pub own_names: Vec<String>,
    pub own_ibans: Vec<String>,
    pub own_tax_ids: Vec<String>,
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

/// The `lang` entry names a language that does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageWarning {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub code: String,
}

/// `[[kinds]]` entries or words that were left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindWarning {
    pub path: PathBuf,
    pub skips: Vec<KindSkip>,
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
pub enum Chords {
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
