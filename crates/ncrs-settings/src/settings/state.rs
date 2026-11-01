use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::Path;

use crate::message::{SettingChange, SettingField};
use ncrs_config::config::{
    check_ai_endpoint, AiEndpointProblem, AiProviderChoice, AiProviderName, AiSection, Config,
    ConfigError, OwnDetail, ProgramLine, ProgramLineProblem,
};
use ncrs_config::file_list_tags::FileListTags;
use ncrs_config::SettingsFeature;
use ncrs_fs::OpenKind;
use ncrs_ui_kit::palette::{parse_color, ColorRole};

/// The id of the search field above the feature list.
pub const SEARCH_FIELD_ID: &str = "settings-search";

/// The section or key of the config file a feature writes to, shown at the
/// right of its heading. These are file syntax, not words to translate.
pub const THEME_SECTION: &str = "[theme]";
pub const LANGUAGE_KEY: &str = "lang";
pub const FILING_SECTION: &str = "[modules.filing]";
pub const OPEN_SECTION: &str = "[open]";
pub const KEYS_SECTION: &str = "[keys]";
pub const SERVERS_SECTION: &str = "[[servers]]";

pub const VIEWER_FIELD_ID: &str = "settings-program-view";
pub const EDITOR_FIELD_ID: &str = "settings-program-edit";
pub const THREADS_FIELD_ID: &str = "settings-ai-threads";
pub const ENDPOINT_FIELD_ID: &str = "settings-ai-endpoint";
pub const MODEL_FIELD_ID: &str = "settings-ai-model";
pub const OWN_NAMES_FIELD_ID: &str = "settings-own-names";
pub const OWN_IBANS_FIELD_ID: &str = "settings-own-ibans";
pub const OWN_TAX_IDS_FIELD_ID: &str = "settings-own-tax-ids";
pub const SWATCH_SIZE: f32 = 16.0;

/// How many characters of the theme file name fit beside its button.
pub const THEME_FILE_CHARS: usize = 26;
/// Room the feature list keeps around its entries.
pub const NAV_PADDING_Y: f32 = 10.0;
pub const NAV_ENTRY_GAP: f32 = 1.0;

/// What the tab remembers while it is open.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsState {
    pub feature: SettingsFeature,
    pub query: String,
    /// The row a hit of the command list asked for, drawn marked until the
    /// next search or page.
    pub marked_row: Option<String>,
    /// Settings changed since the tab was opened.
    pub changed: usize,
    /// Whether the folder of the text recognition models has files; looked up
    /// when the tab opens and after a download, not on every frame.
    pub ocr_models_present: bool,
    /// The system reads scans itself (macOS), so there is nothing to download.
    pub text_recognition_built_in: bool,
    /// The config as read when the tab opened or written since: the values
    /// as the file spells them, which the app keeps only resolved. The error
    /// when it does not load; then nothing can be written.
    pub file: Result<Box<Config>, ConfigError>,
    /// Text typed into a field and not written yet.
    pub drafts: HashMap<SettingField, String>,
    pub problems: HashMap<SettingField, FieldProblem>,
    /// A provider picked but not written: an own server needs its address
    /// and model first.
    pub provider: Option<AiProviderName>,
    /// The macOS App Store build, which may only start apps.
    pub sandboxed: bool,
    /// The user opened the folded rows of the page.
    pub advanced_open: bool,
}

/// Why the value of a field was not written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldProblem {
    Program(ProgramLineProblem),
    OnlyApps,
    PathNotText,
    Color,
    Threads,
    Endpoint(AiEndpointProblem),
    /// Empty, but an own server needs it.
    Needed,
    NotAnIban,
    /// The write failed; the text is already worded.
    Write(String),
}

impl SettingsState {
    pub fn new(
        ocr_models_present: bool,
        file: Result<Box<Config>, ConfigError>,
        sandboxed: bool,
    ) -> Self {
        Self {
            feature: SettingsFeature::Look,
            query: String::new(),
            marked_row: None,
            changed: 0,
            ocr_models_present,
            text_recognition_built_in: false,
            file,
            drafts: HashMap::new(),
            problems: HashMap::new(),
            provider: None,
            sandboxed,
            advanced_open: false,
        }
    }

    /// Brings a page, or one row of it, into view: the row's label becomes the
    /// search, so the tab's own filter leaves it on the page.
    pub fn show(&mut self, feature: SettingsFeature, row: Option<String>) {
        self.feature = feature;
        self.query = row.clone().unwrap_or_default();
        self.marked_row = row;
    }

    pub fn config(&self) -> Option<&Config> {
        self.file.as_deref().ok()
    }

    pub fn ai_section(&self) -> Option<&AiSection> {
        self.config()?
            .modules
            .filing
            .ai
            .as_ref()
            .map(|section| section.get_ref())
    }

    pub fn saved_provider(&self) -> AiProviderName {
        self.ai_section()
            .and_then(|ai| ai.provider.as_ref())
            .map_or_else(AiProviderName::default, |named| *named.get_ref())
    }

    /// The provider the AI rows show: the one picked, else the file's.
    pub fn provider(&self) -> AiProviderName {
        self.provider.unwrap_or_else(|| self.saved_provider())
    }

    pub fn saved_program(&self, kind: OpenKind) -> Option<&ProgramLine> {
        let open = &self.config()?.open;
        match kind {
            OpenKind::View => open.view.as_ref(),
            OpenKind::Edit => open.edit.as_ref(),
        }
    }

    /// The color `[theme]` gives `role`, as written.
    pub fn saved_color(&self, role: ColorRole) -> Option<&str> {
        self.config()?
            .theme
            .entries
            .iter()
            .find(|entry| entry.key.get_ref() == role.name())
            .map(|entry| entry.value.get_ref().as_str())
    }

    pub fn saved_text(&self, field: SettingField) -> String {
        let ai = self.ai_section();
        let ai_text = |key: Option<&serde_spanned::Spanned<String>>| {
            key.map(|given| given.get_ref().clone()).unwrap_or_default()
        };
        match field {
            SettingField::Program(kind) => self
                .saved_program(kind)
                .map(ProgramLine::to_text)
                .unwrap_or_default(),
            SettingField::Color(role) => self.saved_color(role).unwrap_or_default().to_owned(),
            SettingField::AiThreads => ai
                .and_then(|section| section.threads.as_ref())
                .map(|threads| threads.get_ref().to_string())
                .unwrap_or_default(),
            SettingField::AiEndpoint => ai_text(ai.and_then(|section| section.endpoint.as_ref())),
            SettingField::AiModel => ai_text(ai.and_then(|section| section.model.as_ref())),
            SettingField::Own(_) => String::new(),
        }
    }

    /// The tags of the file lists as the file says.
    pub fn file_list_tags(&self) -> FileListTags {
        self.config()
            .map(|config| config.file_list_tags.clone())
            .unwrap_or_default()
    }

    /// What the file lists for `detail`.
    pub fn own_entries(&self, detail: OwnDetail) -> Vec<String> {
        self.config()
            .map(|config| detail.entries_in(&config.modules.filing))
            .unwrap_or_default()
    }

    /// What the field shows: the draft, else the value in the file.
    pub fn field_text(&self, field: SettingField) -> String {
        self.drafts
            .get(&field)
            .cloned()
            .unwrap_or_else(|| self.saved_text(field))
    }

    pub fn edit(&mut self, field: SettingField, text: String) {
        self.problems.remove(&field);
        self.drafts.insert(field, text);
    }

    /// Enter in `field`: the change to write, or `None` when nothing was
    /// typed or the value is wrong, which then shows at its row.
    pub fn submit(&mut self, field: SettingField) -> Option<SettingChange> {
        if !self.drafts.contains_key(&field) {
            return None;
        }
        let text = self.field_text(field);
        let checked = match field {
            SettingField::Program(kind) => program_change(kind, text.trim()),
            SettingField::Color(role) => color_change(role, text.trim()),
            SettingField::AiThreads => threads_change(text.trim()),
            SettingField::AiEndpoint | SettingField::AiModel => self.http_change(),
            SettingField::Own(detail) => {
                let entry = text.trim();
                if entry.is_empty() {
                    self.drafts.remove(&field);
                    return None;
                }
                own_detail_change(detail, entry, self.own_entries(detail))
            }
        };
        match checked {
            Ok(change) => Some(change),
            Err((wrong, problem)) => {
                self.problems.insert(wrong, problem);
                None
            }
        }
    }

    /// Before the page changes or the tab closes: the values typed without
    /// Enter that differ from the file, to be written. `None` when one of them
    /// is wrong; it then shows at its row and the page stays.
    pub fn leave(&mut self) -> Option<Vec<SettingChange>> {
        let mut fields: Vec<SettingField> = self
            .drafts
            .iter()
            .filter(|(field, text)| text.trim() != self.saved_text(**field).trim())
            .map(|(field, _)| *field)
            .filter(|field| self.is_shown(*field))
            .collect();
        fields.sort_unstable();
        // Endpoint and model are written together.
        if fields.contains(&SettingField::AiEndpoint) {
            fields.retain(|field| *field != SettingField::AiModel);
        }
        let mut changes = Vec::with_capacity(fields.len());
        let mut all_valid = true;
        for field in fields {
            match self.submit(field) {
                Some(change) => changes.push(change),
                None => all_valid = false,
            }
        }
        all_valid.then_some(changes)
    }

    /// Threads belong to the model on this machine, endpoint and model name
    /// to an own server: the other provider's fields are not on the page.
    pub fn is_shown(&self, field: SettingField) -> bool {
        match field {
            SettingField::AiThreads => self.provider() == AiProviderName::Local,
            SettingField::AiEndpoint | SettingField::AiModel => {
                self.provider() == AiProviderName::Http
            }
            SettingField::Program(_) | SettingField::Color(_) | SettingField::Own(_) => true,
        }
    }

    pub fn http_change(&self) -> Result<SettingChange, (SettingField, FieldProblem)> {
        let endpoint = self.field_text(SettingField::AiEndpoint).trim().to_owned();
        let model = self.field_text(SettingField::AiModel).trim().to_owned();
        if endpoint.is_empty() {
            return Err((SettingField::AiEndpoint, FieldProblem::Needed));
        }
        check_ai_endpoint(&endpoint)
            .map_err(|problem| (SettingField::AiEndpoint, FieldProblem::Endpoint(problem)))?;
        if model.is_empty() {
            return Err((SettingField::AiModel, FieldProblem::Needed));
        }
        Ok(SettingChange::AiProvider(AiProviderChoice::Http {
            endpoint,
            model,
        }))
    }

    /// A click on a provider. The model on this machine needs nothing more
    /// and is written at once; an own server waits for its address and model.
    pub fn choose_provider(&mut self, chosen: AiProviderName) -> Option<SettingChange> {
        let saved = self.saved_provider();
        self.provider = (chosen != saved).then_some(chosen);
        match chosen {
            AiProviderName::Local if saved != AiProviderName::Local => {
                Some(SettingChange::AiProvider(AiProviderChoice::Local {
                    threads: None,
                }))
            }
            AiProviderName::Local | AiProviderName::Http => None,
        }
    }

    /// A picked program file: an app bundle opens through `open -a`, any
    /// other file is the program itself, which the App Store build refuses.
    pub fn picked_program(&mut self, kind: OpenKind, path: &Path) -> Option<SettingChange> {
        let field = SettingField::Program(kind);
        let Some(text) = path.to_str() else {
            self.problems.insert(field, FieldProblem::PathNotText);
            return None;
        };
        let is_app = cfg!(target_os = "macos")
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("app"));
        let line = if is_app {
            ProgramLine::opening_app(text)
        } else if self.sandboxed {
            self.problems.insert(field, FieldProblem::OnlyApps);
            return None;
        } else {
            ProgramLine::new(text, &[])
        };
        Some(SettingChange::Program {
            kind,
            line: Some(line),
        })
    }

    /// `config` is what the file says after `change` was written.
    pub fn written(&mut self, change: &SettingChange, config: &Config) {
        for field in fields_of(change) {
            self.drafts.remove(&field);
            self.problems.remove(&field);
        }
        if matches!(change, SettingChange::AiProvider(_)) {
            self.provider = None;
        }
        self.file = Ok(Box::new(config.clone()));
        self.changed += 1;
    }

    /// Shows a failed write at the row of `change`. `false` when it has no
    /// field, so the failure goes to the status bar.
    pub fn refused(&mut self, change: &SettingChange, failure: String) -> bool {
        let Some(&field) = fields_of(change).first() else {
            return false;
        };
        self.problems.insert(field, FieldProblem::Write(failure));
        true
    }
}

/// The fields whose draft a written `change` settles; the first one shows a
/// failed write.
fn fields_of(change: &SettingChange) -> Vec<SettingField> {
    match change {
        SettingChange::Program { kind, .. } => vec![SettingField::Program(*kind)],
        SettingChange::Color { role, .. } => vec![SettingField::Color(*role)],
        SettingChange::AiProvider(AiProviderChoice::Local { .. }) => vec![
            SettingField::AiThreads,
            SettingField::AiEndpoint,
            SettingField::AiModel,
        ],
        SettingChange::AiProvider(AiProviderChoice::Http { .. }) => vec![
            SettingField::AiEndpoint,
            SettingField::AiModel,
            SettingField::AiThreads,
        ],
        SettingChange::Preset(_)
        | SettingChange::Language(_)
        | SettingChange::HermesEnabled(_)
        | SettingChange::Load(_)
        | SettingChange::AiEnabled(_)
        | SettingChange::VscodeTheme(_)
        | SettingChange::Servers(_)
        | SettingChange::FileListTags(_) => Vec::new(),
        SettingChange::OwnDetails { detail, .. } => vec![SettingField::Own(*detail)],
    }
}

type Checked = Result<SettingChange, (SettingField, FieldProblem)>;

fn program_change(kind: OpenKind, text: &str) -> Checked {
    let line = if text.is_empty() {
        None
    } else {
        Some(
            ProgramLine::from_text(text)
                .map_err(|problem| (SettingField::Program(kind), FieldProblem::Program(problem)))?,
        )
    };
    Ok(SettingChange::Program { kind, line })
}

fn color_change(role: ColorRole, text: &str) -> Checked {
    if !text.is_empty() && parse_color(text).is_err() {
        return Err((SettingField::Color(role), FieldProblem::Color));
    }
    Ok(SettingChange::Color {
        role,
        color: (!text.is_empty()).then(|| text.to_owned()),
    })
}

/// `entry` joins the entries of the list; one that is there already changes
/// nothing but is still written, which leaves the list as it is.
fn own_detail_change(detail: OwnDetail, entry: &str, mut entries: Vec<String>) -> Checked {
    if !detail.accepts(entry) {
        return Err((SettingField::Own(detail), FieldProblem::NotAnIban));
    }
    if !entries.iter().any(|known| known == entry) {
        entries.push(entry.to_owned());
    }
    Ok(SettingChange::OwnDetails { detail, entries })
}

fn threads_change(text: &str) -> Checked {
    let threads = if text.is_empty() {
        None
    } else {
        Some(
            text.parse::<NonZeroU32>()
                .map_err(|_not_a_count| (SettingField::AiThreads, FieldProblem::Threads))?,
        )
    };
    Ok(SettingChange::AiProvider(AiProviderChoice::Local {
        threads,
    }))
}
