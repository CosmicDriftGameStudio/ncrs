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

/// The `[columns]` section, unchecked.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnsSection {
    #[serde(default)]
    pub same_in_both: Option<bool>,
    /// Checked by `resolve_columns`: a name that is no column is dropped.
    #[serde(default)]
    pub left: Option<Vec<String>>,
    #[serde(default)]
    pub right: Option<Vec<String>>,
    /// Checked by `resolve_columns`: a name that is no column, and a width
    /// that is no pixel count, is dropped.
    #[serde(default)]
    pub widths: std::collections::BTreeMap<String, i64>,
    #[serde(default)]
    pub date_format: Option<DateFormat>,
    #[serde(default)]
    pub size_format: Option<SizeFormat>,
}

/// The `[git_view]` section, unchecked.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitViewSection {
    /// Checked by `resolve_git_view`: a width that is no pixel count is dropped.
    #[serde(default)]
    pub author_width: Option<i64>,
    #[serde(default)]
    pub date_width: Option<i64>,
    #[serde(default)]
    pub sha_width: Option<i64>,
    #[serde(default)]
    pub folded: std::collections::BTreeMap<String, Vec<String>>,
}

/// The `[appearance]` section, unchecked.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppearanceSection {
    /// Empty or missing is the system font.
    #[serde(default)]
    pub font: Option<String>,
    /// Checked by `resolve_appearance`: a size outside the range is the system's.
    #[serde(default)]
    pub font_size: Option<i64>,
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
    /// A key file for this server, as typed; `~` leads to the home folder.
    #[serde(default)]
    pub key_file: Option<Spanned<String>>,
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
    /// The key file to log in with, as typed.
    pub key_file: Option<String>,
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

/// `servers` with `server` saved: an entry that is there takes the start folder
/// and key file that were given, otherwise one is added. A start folder that is
/// not an absolute path on the server is not kept. Never holds a password.
pub fn merge_saved_server(
    servers: &[ServerEntry],
    server: &ncrs_fs::remote::ServerId,
    typed_path: Option<&str>,
    key_file: Option<String>,
) -> Vec<ServerEntry> {
    let path = typed_path
        .filter(|text| text.starts_with('/'))
        .map(str::to_owned);
    let mut merged = servers.to_vec();
    match merged.iter_mut().find(|entry| entry.server_id() == *server) {
        Some(existing) => {
            if path.is_some() {
                existing.path = path;
            }
            if key_file.is_some() {
                existing.key_file = key_file;
            }
        }
        None => merged.push(ServerEntry {
            name: None,
            host: server.host.clone(),
            port: server.port,
            user: server.user.clone(),
            path,
            key_file,
        }),
    }
    merged
}

#[cfg(test)]
mod merge_saved_server_tests {
    use super::*;

    fn entry(path: Option<&str>) -> ServerEntry {
        ServerEntry {
            name: Some("Office".to_owned()),
            host: "files.example".to_owned(),
            port: 22,
            user: "anna".to_owned(),
            path: path.map(str::to_owned),
            key_file: None,
        }
    }

    #[test]
    fn a_saved_server_keeps_its_name_and_takes_the_new_folder() {
        let existing = entry(Some("/old"));
        let server = existing.server_id();
        let merged = merge_saved_server(&[existing], &server, Some("/new"), None);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name.as_deref(), Some("Office"));
        assert_eq!(merged[0].path.as_deref(), Some("/new"));
    }

    #[test]
    fn a_relative_folder_leaves_the_saved_one_alone() {
        let existing = entry(Some("/old"));
        let server = existing.server_id();
        let merged = merge_saved_server(&[existing], &server, Some("docs"), None);
        assert_eq!(merged[0].path.as_deref(), Some("/old"));
    }

    #[test]
    fn an_unknown_server_is_added_with_the_key_file() {
        let other = entry(None);
        let server = ncrs_fs::remote::ServerId::new("bob", "other.example", 2222);
        let merged = merge_saved_server(
            &[other],
            &server,
            Some("/srv"),
            Some("~/.ssh/id".to_owned()),
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[1].host, "other.example");
        assert_eq!(merged[1].port, 2222);
        assert_eq!(merged[1].key_file.as_deref(), Some("~/.ssh/id"));
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
    /// `None` when the file does not say: only then is it a first run.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Keeps the position of the whole array for the "no target" error.
    #[serde(default)]
    pub locations: Option<Spanned<Vec<LocationEntry>>>,
    /// `Some` even for an empty list: once `sources` or `targets` is in the
    /// file, `locations` no longer says anything about them.
    #[serde(default)]
    pub targets: Option<Spanned<Vec<TargetEntry>>>,
    #[serde(default)]
    pub sources: Option<Spanned<Vec<SourceEntry>>>,
    /// The number the next `t<n>` target id starts from; ids of deleted
    /// targets are never handed out again.
    #[serde(default)]
    pub next_target: Option<u32>,
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

/// The `[hermes]` section: the one switch for every call to a model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HermesSection {
    /// A file without the key has Hermes on.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Chats of the Hermes chat unused for this many days are deleted when
    /// the app starts; a file without the key never deletes them.
    #[serde(default)]
    pub chat_keep_days: Option<u32>,
}

/// The `[modules.filing.ai]` section, unchecked.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiSection {
    /// Older files switch the model here; the switch in `[hermes]` decides
    /// now. Never written, only read: it tells whether a file from before the
    /// consent dialog meant its server to get documents.
    #[serde(default, rename = "enabled")]
    pub legacy_enabled: Option<bool>,
    #[serde(default)]
    pub provider: Option<Spanned<AiProviderName>>,
    #[serde(default)]
    pub threads: Option<Spanned<i64>>,
    #[serde(default)]
    pub endpoint: Option<Spanned<String>>,
    #[serde(default)]
    pub model: Option<Spanned<String>>,
    /// The servers (`scheme://host[:port]`) the user agreed documents may be
    /// sent to. Absent in a file from before the dialog asked for it.
    #[serde(default)]
    pub consented_origins: Option<Vec<String>>,
    /// The servers new documents may be sent to. A file from before the
    /// three agreements has `consented_origins` instead, which counts for none.
    #[serde(default)]
    pub consented_inbox: Option<Vec<String>>,
    /// The servers texts and data of filed documents may be sent to.
    #[serde(default)]
    pub consented_archive: Option<Vec<String>>,
    /// The servers folder structure and file names may be sent to.
    #[serde(default)]
    pub consented_folders: Option<Vec<String>>,
}

impl AiSection {
    /// The servers agreed to for each kind of data. The single agreement of
    /// older files (`consented_origins`, or an AI switched on in the file)
    /// grants nothing: the user is asked again for each kind.
    pub fn consents(&self) -> ConsentedOrigins {
        let of = |own: &Option<Vec<String>>| own.clone().unwrap_or_default();
        ConsentedOrigins {
            inbox: of(&self.consented_inbox),
            archive: of(&self.consented_archive),
            folders: of(&self.consented_folders),
        }
    }

    /// An older file agreed to a server in one go, and no kind has been
    /// agreed to since: the page tells the user that the agreement is dropped.
    pub fn legacy_agreement_dropped(&self) -> bool {
        let has_own = self.consented_inbox.is_some()
            || self.consented_archive.is_some()
            || self.consented_folders.is_some();
        let agreed_before = self
            .consented_origins
            .as_ref()
            .is_some_and(|listed| !listed.is_empty())
            || (self.consented_origins.is_none() && self.legacy_enabled == Some(true));
        agreed_before && !has_own
    }
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
/// `Off` is the switch of the `[hermes]` section: no model is called at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiSetting {
    Off,
    /// The model on this machine. `None` leaves the thread count to the module.
    Local {
        threads: Option<NonZeroU32>,
    },
    Http {
        endpoint: String,
        model: String,
        /// The servers the user agreed to send each kind of data to.
        consented_origins: ConsentedOrigins,
    },
}

/// Hermes on, with the model of this machine, as a file without a word about
/// it has it.
impl Default for AiSetting {
    fn default() -> Self {
        Self::Local { threads: None }
    }
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
    #[serde(default)]
    pub transfer: Option<TransferName>,
    #[serde(default)]
    pub continuous: bool,
}

/// The spelling of `transfer` in a location; `load` maps it to `LocationTransfer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferName {
    Copy,
    Move,
}

impl From<TransferName> for LocationTransfer {
    fn from(name: TransferName) -> Self {
        match name {
            TransferName::Copy => Self::Copy,
            TransferName::Move => Self::Move,
        }
    }
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filing {
    pub enabled: bool,
    /// The config says nothing at all about filing: Hermes has never been
    /// switched on or off, so the first run turns it on.
    pub first_run: bool,
    /// The folders as the old filing path sees them. Goes with the old filing
    /// path: derived from `targets` and `sources` once those are in the file.
    pub locations: Vec<FilingLocation>,
    /// Where files are put, in the order of the file.
    pub targets: Vec<TargetSetting>,
    /// Folders files are taken from, in the order of the file.
    pub sources: Vec<SourceSetting>,
    /// Target entries that could not be used at all (no valid id), each with
    /// the reason.
    pub dropped_entries: Vec<String>,
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

impl Filing {
    /// The `[hermes]` switch, from the one value `enabled` is read from; a
    /// first run has it on while there is nothing to watch yet.
    pub fn hermes_on(&self) -> bool {
        self.enabled || self.first_run
    }
}

/// What a config with nothing about filing resolves to.
impl Default for Filing {
    fn default() -> Self {
        Self {
            enabled: false,
            first_run: true,
            locations: Vec::new(),
            targets: Vec::new(),
            sources: Vec::new(),
            dropped_entries: Vec::new(),
            load: Load::default(),
            ai: AiSetting::default(),
            default_pattern: None,
            own_names: Vec::new(),
            own_ibans: Vec::new(),
            own_tax_ids: Vec::new(),
        }
    }
}

/// A location with an expanded, absolute, `..`-free path. Not looked up on
/// disk: whether the folder exists is for the module to find out when it starts.
// Goes with the old filing path
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingLocation {
    pub path: PathBuf,
    pub mode: LocationMode,
    pub images: bool,
    pub is_target: bool,
    pub transfer: LocationTransfer,
    /// Hermes files the new files of a read folder by itself, with `transfer`.
    pub continuous: bool,
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
