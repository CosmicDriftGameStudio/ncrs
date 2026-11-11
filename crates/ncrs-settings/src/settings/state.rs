use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::facts::{LoadedFacts, ModelKind};
use crate::message::{SettingChange, SettingField, TerminalChoice};
use ncrs_config::config::{
    check_ai_endpoint, AiEndpointProblem, AiProviderChoice, AiProviderName, AiSection, Config,
    ConfigError, OwnDetail, ProgramLine, ProgramLineProblem,
};
use ncrs_config::file_list_tags::FileListTags;
use ncrs_config::panel_columns::{Appearance, PanelColumns};
use ncrs_config::system_monitor::SystemSettings;
use ncrs_config::SettingsFeature;
use ncrs_core::PanelSide;
use ncrs_fs::{KnownTerminal, OpenKind};
use ncrs_ui_kit::font::SystemFont;
use ncrs_ui_kit::palette::{parse_color, ColorRole};
use ncrs_ui_kit::theme::spacing;

/// The id of the search field above the feature list.
pub const SEARCH_FIELD_ID: &str = "settings-search";

/// The section or key of the config file a feature writes to, shown at the
/// right of its heading. These are file syntax, not words to translate.
pub const THEME_SECTION: &str = "[theme]";
pub const LANGUAGE_KEY: &str = "lang";
/// The top-level key behind the Favourites page's switch.
pub const FAVOURITES_BAR_KEY: &str = "favourites_bar";
pub const FILING_SECTION: &str = "[modules.filing]";
pub const AI_PAGE_SECTIONS: &str = "[hermes], [modules.filing.ai]";
pub const OPEN_SECTION: &str = "[open]";
pub const KEYS_SECTION: &str = "[keys]";
pub const SERVERS_SECTION: &str = "[[servers]]";
pub const SYSTEM_SECTION: &str = "[system]";
pub const COLUMNS_SECTION: &str = "[columns]";

pub const VIEWER_FIELD_ID: &str = "settings-program-view";
pub const EDITOR_FIELD_ID: &str = "settings-program-edit";
pub const TERMINAL_FIELD_ID: &str = "settings-program-terminal";
pub const ENDPOINT_FIELD_ID: &str = "settings-ai-endpoint";
pub const MODEL_FIELD_ID: &str = "settings-ai-model";
pub const OWN_NAMES_FIELD_ID: &str = "settings-own-names";
pub const OWN_IBANS_FIELD_ID: &str = "settings-own-ibans";
pub const OWN_TAX_IDS_FIELD_ID: &str = "settings-own-tax-ids";
pub const SWATCH_SIZE: f32 = 16.0;

/// How many characters of the theme file name fit beside its button.
pub const THEME_FILE_CHARS: usize = 26;
/// Room the feature list keeps around its entries.
pub const NAV_PADDING_Y: f32 = spacing::L;

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
    /// Whether the AI model file is in place; looked up like the above, and
    /// it decides whether the Index page offers to turn the AI on or to load
    /// the model.
    pub ai_model_present: bool,
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
    /// The terminals found on this computer when the tab opened; looked up
    /// then, not on every frame.
    pub terminals: Vec<KnownTerminal>,
    /// "Own command" was picked in the terminal list but nothing is saved
    /// for it yet.
    pub terminal_own: bool,
    /// The read folder whose dialog is open.
    pub folder_dialog: Option<PathBuf>,
    /// The model the user is asked about removing.
    pub model_to_remove: Option<ModelKind>,
    /// The Order section lists every area, not the first three.
    pub all_areas: bool,
    /// The History section lists every run it knows, not the last three.
    pub all_runs: bool,
    /// Why the last change of the folder lists was refused, worded.
    pub folder_note: Option<String>,
    /// What the app read for the Index and Hermes pages; `None` until it did.
    pub loaded: Option<LoadedFacts>,
    /// The app was asked to read it, so it does not ask again.
    pub facts_requested: bool,
    /// A read of the facts is under way; the tick does not start another one.
    pub facts_in_flight: bool,
    /// The list of the Columns page that is edited while each panel has its own.
    pub columns_side: PanelSide,
    /// The monospace families found when the tab opened, for the font list.
    pub installed_fonts: Vec<String>,
    /// What the desktop says about its fonts, for the "System" entries.
    pub system_font: Option<SystemFont>,
    /// Where the keyboard is: the list of pages or the rows of the page.
    pub focus: crate::focus::Focus,
    /// The API key typed on the AI page and not kept yet.
    /// `Secret` prints as `***`, so the state can be logged.
    pub ai_key: Option<ncrs_ai::Secret>,
    pub ai_check: crate::settings::AiCheck,
    /// The period the Usage page shows, in days.
    pub usage_days: u32,
    /// The sums of the usage log per period; `None` until it was read.
    pub usage: Option<Vec<(u32, ncrs_ai::UsageSummary)>>,
}

/// Why the value of a field was not written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldProblem {
    Program(ProgramLineProblem),
    OnlyApps,
    PathNotText,
    Color,
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
            ai_model_present: false,
            file,
            drafts: HashMap::new(),
            problems: HashMap::new(),
            provider: None,
            sandboxed,
            terminals: Vec::new(),
            terminal_own: false,
            folder_dialog: None,
            model_to_remove: None,
            all_areas: false,
            all_runs: false,
            folder_note: None,
            loaded: None,
            facts_requested: false,
            facts_in_flight: false,
            columns_side: PanelSide::Left,
            installed_fonts: Vec::new(),
            system_font: None,
            focus: crate::focus::Focus::default(),
            ai_key: None,
            ai_check: crate::settings::AiCheck::Idle,
            usage_days: crate::settings::DEFAULT_USAGE_DAYS,
            usage: None,
        }
    }

    /// A small dialog of the Index or Hermes page is up; Escape closes it
    /// and not the page.
    pub fn dialog_is_open(&self) -> bool {
        self.folder_dialog.is_some() || self.model_to_remove.is_some()
    }

    pub fn close_dialog(&mut self) {
        self.folder_dialog = None;
        self.model_to_remove = None;
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
            OpenKind::Terminal => open.terminal.as_ref(),
        }
    }

    /// The entry of the terminal list that shows: a saved line that is one
    /// of the terminals found is that terminal, any other saved line is the
    /// own command, and nothing saved is Automatic.
    pub fn terminal_choice(&self) -> TerminalChoice {
        if self.terminal_own {
            return TerminalChoice::Own;
        }
        match self.saved_program(OpenKind::Terminal) {
            Some(saved) => self
                .terminals
                .iter()
                .position(|terminal| terminal.line == *saved)
                .map_or(TerminalChoice::Own, TerminalChoice::Detected),
            None => TerminalChoice::Automatic,
        }
    }

    /// A click in the terminal list. A found terminal is saved as the line
    /// that starts it; Automatic drops the line; Own only opens the field.
    pub fn choose_terminal(&mut self, choice: TerminalChoice) -> Option<SettingChange> {
        self.terminal_own = choice == TerminalChoice::Own;
        let line = match choice {
            TerminalChoice::Automatic => None,
            TerminalChoice::Detected(index) => Some(self.terminals.get(index)?.line.clone()),
            TerminalChoice::Own => return None,
        };
        Some(SettingChange::Program {
            kind: OpenKind::Terminal,
            line,
        })
    }

    /// Whether the row of `kind` offers the way back to the system default:
    /// a program is saved, or something is typed in the field that Enter or
    /// leaving the page would save.
    pub fn can_reset_program(&self, kind: OpenKind) -> bool {
        self.saved_program(kind).is_some()
            || !self
                .field_text(SettingField::Program(kind))
                .trim()
                .is_empty()
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

    pub fn system_settings(&self) -> SystemSettings {
        self.config()
            .map(|config| config.system.clone())
            .unwrap_or_default()
    }

    /// The `[columns]` section as the file says.
    pub fn panel_columns(&self) -> PanelColumns {
        self.config()
            .map(|config| config.columns.clone())
            .unwrap_or_default()
    }

    /// The `[appearance]` section as the file says.
    pub fn appearance(&self) -> Appearance {
        self.config()
            .map(|config| config.appearance.clone())
            .unwrap_or_default()
    }

    /// The list of the Columns page that is edited: the left one while both
    /// panels share it.
    pub fn edited_columns_side(&self) -> PanelSide {
        if self.panel_columns().same_in_both {
            PanelSide::Left
        } else {
            self.columns_side
        }
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
        if field == SettingField::AiEndpoint {
            self.ai_check = crate::settings::AiCheck::Idle;
        }
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

    /// Endpoint and model name belong to an own server: they are not on the
    /// page for the model on this machine.
    pub fn is_shown(&self, field: SettingField) -> bool {
        match field {
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
            consented_origins: self
                .ai_section()
                .map(AiSection::consented)
                .unwrap_or_default(),
        }))
    }

    /// Whether the user agreed to send to the server shown on the AI page:
    /// `None` while the model on this computer is chosen or the address is
    /// none off this computer.
    pub fn server_agreed(&self) -> Option<bool> {
        if self.provider() != AiProviderName::Http {
            return None;
        }
        let consented = self
            .ai_section()
            .map(AiSection::consented)
            .unwrap_or_default();
        crate::ai_consent::server_agreed(&self.typed_endpoint(), &consented)
    }

    /// "Allow" on the AI page: the server as typed is saved with its origin
    /// among the ones agreed to. A wrong address or an empty model shows at
    /// its row instead.
    pub fn agree_to_server(&mut self) -> Option<SettingChange> {
        match self.http_change() {
            Ok(SettingChange::AiProvider(choice)) => {
                let agreed = match crate::ai_consent::ConsentAsk::needed_for(&choice) {
                    Some(ask) => ask.agreed(),
                    None => choice,
                };
                Some(SettingChange::AiProvider(agreed))
            }
            Ok(other) => Some(other),
            Err((field, problem)) => {
                self.problems.insert(field, problem);
                None
            }
        }
    }

    /// A click on a provider. The model on this machine needs nothing more
    /// and is written at once; an own server waits for its address and model.
    pub fn choose_provider(&mut self, chosen: AiProviderName) -> Option<SettingChange> {
        let saved = self.saved_provider();
        self.provider = (chosen != saved).then_some(chosen);
        if chosen == AiProviderName::Local {
            self.ai_key = None;
            self.ai_check = crate::settings::AiCheck::Idle;
        }
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
        if matches!(
            change,
            SettingChange::Program {
                kind: OpenKind::Terminal,
                line: None
            }
        ) {
            self.terminal_own = false;
        }
        if matches!(change, SettingChange::Locations(_)) {
            self.folder_note = None;
            self.facts_requested = false;
        }
        self.file = Ok(Box::new(config.clone()));
        self.changed += 1;
    }

    /// Shows a failed write at the row of `change`. `false` when it has no
    /// field, or its row is on a page that is not shown (a write that was
    /// started on the way out of it), so the failure goes to the status bar.
    pub fn refused(&mut self, change: &SettingChange, failure: String) -> bool {
        if matches!(change, SettingChange::Locations(_)) {
            self.folder_note = Some(failure);
            return matches!(
                self.feature,
                SettingsFeature::Index | SettingsFeature::Hermes
            );
        }
        let Some(&field) = fields_of(change).first() else {
            return false;
        };
        self.problems.insert(field, FieldProblem::Write(failure));
        page_of(field) == self.feature
    }
}

fn page_of(field: SettingField) -> SettingsFeature {
    match field {
        SettingField::Color(_) => SettingsFeature::Look,
        SettingField::Program(_) => SettingsFeature::Programs,
        SettingField::AiEndpoint | SettingField::AiModel => SettingsFeature::Ai,
        SettingField::Own(_) => SettingsFeature::Index,
    }
}

/// The fields whose draft a written `change` settles; the first one shows a
/// failed write.
fn fields_of(change: &SettingChange) -> Vec<SettingField> {
    match change {
        SettingChange::Program { kind, .. } => vec![SettingField::Program(*kind)],
        SettingChange::Color { role, .. } => vec![SettingField::Color(*role)],
        SettingChange::AiProvider(_) => vec![SettingField::AiEndpoint, SettingField::AiModel],
        SettingChange::Preset(_)
        | SettingChange::Language(_)
        | SettingChange::CheckForUpdates(_)
        | SettingChange::FavouritesBar(_)
        | SettingChange::Load(_)
        | SettingChange::HermesSwitch(_)
        | SettingChange::VscodeTheme(_)
        | SettingChange::Servers(_)
        | SettingChange::Locations(_)
        | SettingChange::FileListTags(_)
        | SettingChange::Columns(_)
        | SettingChange::Appearance(_)
        | SettingChange::System(_) => Vec::new(),
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

/// What is typed joins the entries of the list; commas and semicolons
/// separate several. One that is there already changes nothing but is still
/// written, which leaves the list as it is. One that is wrong stops them all.
fn own_detail_change(detail: OwnDetail, typed: &str, mut entries: Vec<String>) -> Checked {
    for entry in typed
        .split([',', ';'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        if !detail.accepts(entry) {
            return Err((SettingField::Own(detail), FieldProblem::NotAnIban));
        }
        if !entries.iter().any(|known| known == entry) {
            entries.push(entry.to_owned());
        }
    }
    Ok(SettingChange::OwnDetails { detail, entries })
}
