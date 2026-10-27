//! The settings tab: a feature list on the left, the settings of the chosen
//! feature on the right, laid out like the pages of the other cards. It holds
//! only settings the config file has; every change is written there.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::Path;

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, scrollable, text, Column};
use iced::{alignment, Color, Element, Font, Length, Padding};
use ncrs_intake::LocationMode;
use ncrs_pipeline::Load;

use super::format::ellipsize_middle;
use super::frame::{SCROLLBAR_MARGIN, SCROLLBAR_WIDTH};
use super::kit::{self, Check, Tone};
use super::layout::{
    SETTINGS_NAV_WIDTH, SETTINGS_PAGE_MAX_WIDTH, SETTINGS_WIDE_CONTROL_WIDTH, STATUSBAR_HEIGHT,
};
use super::theme::{self, font_size, size, spacing};
use super::titlebar::tab_label;
use crate::app::fill_template;
use crate::config::{
    check_ai_endpoint, AiEndpointProblem, AiProviderChoice, AiProviderName, AiSection, AiSetting,
    Config, ConfigError, Filing, ProgramLine, ProgramLineProblem,
};
use crate::filing::AiStatus;
use crate::filing_setup::shown_path;
use crate::fs::OpenKind;
use crate::i18n::{Language, Msg};
use crate::messages::{
    AiModelMessage, FilingSetupMessage, KeyBindingsMessage, Message, OcrModelsMessage,
    SettingChange, SettingField, SettingsFeature, SettingsMessage, ThemeFetchMessage,
};
use crate::palette::{hex, parse_color, ColorRole, Palette, ThemePreset};

/// The id of the search field above the feature list.
pub const SEARCH_FIELD_ID: &str = "settings-search";

/// The section or key of the config file a feature writes to, shown at the
/// right of its heading. These are file syntax, not words to translate.
const THEME_SECTION: &str = "[theme]";
const LANGUAGE_KEY: &str = "lang";
const FILING_SECTION: &str = "[modules.filing]";
const OPEN_SECTION: &str = "[open]";
const KEYS_SECTION: &str = "[keys]";

const VIEWER_FIELD_ID: &str = "settings-program-view";
const EDITOR_FIELD_ID: &str = "settings-program-edit";
const THREADS_FIELD_ID: &str = "settings-ai-threads";
const ENDPOINT_FIELD_ID: &str = "settings-ai-endpoint";
const MODEL_FIELD_ID: &str = "settings-ai-model";
const SWATCH_SIZE: f32 = 16.0;

/// How many characters of a folder fit beside its button.
const FOLDER_CHARS: usize = 22;
/// How many characters of the theme file name fit beside its button.
const THEME_FILE_CHARS: usize = 26;
/// Room the feature list keeps around its entries.
const NAV_PADDING_Y: f32 = 10.0;
const NAV_ENTRY_GAP: f32 = 1.0;

/// What the tab remembers while it is open.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsState {
    pub feature: SettingsFeature,
    pub query: String,
    /// Settings changed since the tab was opened.
    pub changed: usize,
    /// Whether the folder of the text recognition models has files; looked up
    /// when the tab opens and after a download, not on every frame.
    pub ocr_models_present: bool,
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
            changed: 0,
            ocr_models_present,
            file,
            drafts: HashMap::new(),
            problems: HashMap::new(),
            provider: None,
            sandboxed,
        }
    }

    fn config(&self) -> Option<&Config> {
        self.file.as_deref().ok()
    }

    fn ai_section(&self) -> Option<&AiSection> {
        self.config()?
            .modules
            .filing
            .ai
            .as_ref()
            .map(|section| section.get_ref())
    }

    fn saved_provider(&self) -> AiProviderName {
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

    fn saved_text(&self, field: SettingField) -> String {
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
        }
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
    fn is_shown(&self, field: SettingField) -> bool {
        match field {
            SettingField::AiThreads => self.provider() == AiProviderName::Local,
            SettingField::AiEndpoint | SettingField::AiModel => {
                self.provider() == AiProviderName::Http
            }
            SettingField::Program(_) | SettingField::Color(_) => true,
        }
    }

    fn http_change(&self) -> Result<SettingChange, (SettingField, FieldProblem)> {
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
        | SettingChange::VscodeTheme(_) => Vec::new(),
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

/// Everything the tab shows that does not live in [`SettingsState`].
pub struct SettingsView<'a> {
    pub state: &'a SettingsState,
    pub preset: ThemePreset,
    pub vscode_theme: Option<&'a str>,
    pub filing: &'a Filing,
    /// `None` while the filing module is not running.
    pub ai_status: Option<AiStatus>,
    pub home: &'a Path,
    /// How many actions have keys other than their built-in ones.
    pub keys_changed: usize,
    pub lang: Language,
    pub palette: &'a Palette,
}

struct SettingRow<'a> {
    label: String,
    theme_name: Option<String>,
    description: Option<String>,
    /// What is wrong with the value, under the description.
    problem: Option<String>,
    control: Element<'a, Message>,
    control_width: Option<f32>,
}

struct Group<'a> {
    title: String,
    rows: Vec<SettingRow<'a>>,
}

fn change(setting: SettingChange) -> Message {
    Message::Settings(SettingsMessage::Change(setting))
}

fn settings_message(message: SettingsMessage) -> Message {
    Message::Settings(message)
}

/// A text field of the tab: typing keeps a draft, Enter checks and writes it.
fn setting_field<'a>(
    field: SettingField,
    id: &'static str,
    placeholder: &str,
    typing: bool,
    view: &SettingsView<'_>,
) -> Element<'a, Message> {
    let on_input =
        typing.then_some(move |text| settings_message(SettingsMessage::Edit { field, text }));
    kit::mono_text_field(
        placeholder,
        &view.state.field_text(field),
        id,
        on_input,
        settings_message(SettingsMessage::Submit(field)),
        view.palette,
    )
}

fn problem_text(problem: &FieldProblem, lang: Language) -> String {
    let message = match problem {
        FieldProblem::Program(ProgramLineProblem::NoProgram | ProgramLineProblem::EmptyProgram) => {
            Msg::SettingsProblemProgramEmpty
        }
        FieldProblem::Program(ProgramLineProblem::Nul) => Msg::SettingsProblemProgramNul,
        FieldProblem::Program(ProgramLineProblem::RelativePath) => {
            Msg::SettingsProblemProgramRelative
        }
        FieldProblem::Program(ProgramLineProblem::UnclosedQuote) => {
            Msg::SettingsProblemUnclosedQuote
        }
        FieldProblem::OnlyApps => Msg::SettingsProblemOnlyApps,
        FieldProblem::PathNotText => Msg::SettingsProblemPathNotText,
        FieldProblem::Color => Msg::SettingsProblemColor,
        FieldProblem::Threads => Msg::SettingsProblemThreads,
        FieldProblem::Endpoint(AiEndpointProblem::Unusable(reason)) => {
            return fill_template(
                lang.text(Msg::SettingsProblemEndpoint),
                &[("{reason}", reason)],
            );
        }
        FieldProblem::Endpoint(AiEndpointProblem::NotLocal) => Msg::SettingsProblemEndpointNotLocal,
        FieldProblem::Needed => Msg::SettingsProblemNeeded,
        FieldProblem::Write(failure) => return failure.clone(),
    };
    lang.text(message).to_owned()
}

fn field_problem(field: SettingField, view: &SettingsView<'_>) -> Option<String> {
    view.state
        .problems
        .get(&field)
        .map(|problem| problem_text(problem, view.lang))
}

fn feature_label(feature: SettingsFeature) -> Msg {
    match feature {
        SettingsFeature::Look => Msg::SettingsFeatureLook,
        SettingsFeature::Language => Msg::SettingsFeatureLanguage,
        SettingsFeature::Programs => Msg::SettingsFeaturePrograms,
        SettingsFeature::Keys => Msg::SettingsFeatureKeys,
        SettingsFeature::Hermes => Msg::SettingsFeatureHermes,
    }
}

fn feature_theme_name(feature: SettingsFeature) -> Option<Msg> {
    match feature {
        SettingsFeature::Hermes => Some(Msg::GroupHermesTheme),
        SettingsFeature::Look
        | SettingsFeature::Language
        | SettingsFeature::Programs
        | SettingsFeature::Keys => None,
    }
}

fn feature_intro(feature: SettingsFeature) -> Msg {
    match feature {
        SettingsFeature::Look => Msg::SettingsFeatureLookIntro,
        SettingsFeature::Language => Msg::SettingsFeatureLanguageIntro,
        SettingsFeature::Programs => Msg::SettingsFeatureProgramsIntro,
        SettingsFeature::Keys => Msg::SettingsFeatureKeysIntro,
        SettingsFeature::Hermes => Msg::SettingsFeatureHermesIntro,
    }
}

fn feature_config_place(feature: SettingsFeature) -> &'static str {
    match feature {
        SettingsFeature::Look => THEME_SECTION,
        SettingsFeature::Language => LANGUAGE_KEY,
        SettingsFeature::Programs => OPEN_SECTION,
        SettingsFeature::Keys => KEYS_SECTION,
        SettingsFeature::Hermes => FILING_SECTION,
    }
}

fn preset_label(preset: ThemePreset) -> Msg {
    match preset {
        ThemePreset::Cosmic => Msg::SettingsPresetCosmic,
        ThemePreset::Teal => Msg::SettingsPresetTeal,
        ThemePreset::Classic => Msg::SettingsPresetClassic,
        ThemePreset::Dracula => Msg::SettingsPresetDracula,
    }
}

fn mono<'a>(content: String, color: Color) -> Element<'a, Message> {
    text(content)
        .size(font_size::FILE)
        .font(Font::MONOSPACE)
        .color(color)
        .wrapping(Wrapping::None)
        .into()
}

fn status_text<'a>(content: String, color: Color) -> Element<'a, Message> {
    text(content)
        .size(font_size::BODY)
        .color(color)
        .wrapping(Wrapping::None)
        .into()
}

fn with_button<'a>(
    value: Element<'a, Message>,
    label: &str,
    on_press: Message,
    palette: &Palette,
) -> Element<'a, Message> {
    row![
        value,
        kit::button(label, Some(on_press), Tone::Secondary, palette)
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn switch<'a>(on: bool, setting: SettingChange, view: &SettingsView<'_>) -> Element<'a, Message> {
    let state = if on { Check::On } else { Check::Off };
    let word = view.lang.text(if on {
        Msg::SettingsOn
    } else {
        Msg::SettingsOff
    });
    row![
        kit::checkbox(state, Some(change(setting)), view.palette),
        status_text(word.to_owned(), view.palette.dim_text),
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn simple_row<'a>(
    label: Msg,
    note: Option<Msg>,
    control: Element<'a, Message>,
    lang: Language,
) -> SettingRow<'a> {
    SettingRow {
        label: lang.text(label).to_owned(),
        theme_name: None,
        description: note.map(|msg| lang.text(msg).to_owned()),
        problem: None,
        control,
        control_width: None,
    }
}

fn look_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let options: Vec<(&str, Message)> = ThemePreset::ALL
        .iter()
        .map(|&preset| {
            (
                lang.text(preset_label(preset)),
                change(SettingChange::Preset(preset)),
            )
        })
        .collect();
    let chosen = ThemePreset::ALL
        .iter()
        .position(|&preset| preset == view.preset)
        .unwrap_or_default();
    let mut preset_row = simple_row(
        Msg::SettingsRowPreset,
        Some(Msg::SettingsRowPresetNote),
        kit::segmented(&options, chosen, true, palette),
        lang,
    );
    preset_row.control_width = Some(SETTINGS_WIDE_CONTROL_WIDTH);

    let open_vsx_row = simple_row(
        Msg::SettingsRowOpenVsx,
        Some(Msg::SettingsRowOpenVsxNote),
        kit::button(
            lang.text(Msg::SettingsVscodeFetch),
            Some(Message::ThemeFetch(ThemeFetchMessage::Open)),
            Tone::Secondary,
            palette,
        ),
        lang,
    );
    vec![
        Group {
            title: String::new(),
            rows: vec![preset_row, vscode_row(view), open_vsx_row],
        },
        Group {
            title: lang.text(Msg::SettingsGroupColors).to_owned(),
            rows: ColorRole::ALL
                .iter()
                .map(|&role| color_row(role, view))
                .collect(),
        },
    ]
}

fn vscode_row<'a>(view: &SettingsView<'a>) -> SettingRow<'a> {
    let lang = view.lang;
    let palette = view.palette;
    let file = match view.vscode_theme {
        Some(path) => mono(ellipsize_middle(path, THEME_FILE_CHARS), palette.text),
        None => status_text(
            lang.text(Msg::SettingsVscodeNone).to_owned(),
            palette.dim_text,
        ),
    };
    let mut control = row![
        container(file).width(Length::Fill),
        kit::button(
            lang.text(Msg::SettingsChoose),
            Some(settings_message(SettingsMessage::PickThemeFile)),
            Tone::Secondary,
            palette,
        ),
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center);
    if view.vscode_theme.is_some() {
        control = control.push(kit::button(
            lang.text(Msg::SettingsRemove),
            Some(change(SettingChange::VscodeTheme(None))),
            Tone::Secondary,
            palette,
        ));
    }
    let warning = view
        .state
        .config()
        .and_then(|config| config.vscode_warning.as_ref())
        .map(|warning| {
            fill_template(
                lang.text(Msg::SettingsVscodeProblem),
                &[("{reason}", &warning.reason)],
            )
        });
    SettingRow {
        problem: warning,
        control_width: Some(SETTINGS_WIDE_CONTROL_WIDTH),
        ..simple_row(
            Msg::SettingsRowVscode,
            Some(Msg::SettingsRowVscodeNote),
            control.into(),
            lang,
        )
    }
}

fn color_label(role: ColorRole) -> Msg {
    match role {
        ColorRole::Background => Msg::SettingsColorBackground,
        ColorRole::Panel => Msg::SettingsColorPanel,
        ColorRole::Text => Msg::SettingsColorText,
        ColorRole::DimText => Msg::SettingsColorDimText,
        ColorRole::Directory => Msg::SettingsColorDirectory,
        ColorRole::Cursor => Msg::SettingsColorCursor,
        ColorRole::CursorText => Msg::SettingsColorCursorText,
        ColorRole::Tagged => Msg::SettingsColorTagged,
        ColorRole::Border => Msg::SettingsColorBorder,
        ColorRole::Bar => Msg::SettingsColorBar,
        ColorRole::BarInactive => Msg::SettingsColorBarInactive,
        ColorRole::StatusBar => Msg::SettingsColorStatusBar,
        ColorRole::Dialog => Msg::SettingsColorDialog,
        ColorRole::Menu => Msg::SettingsColorMenu,
        ColorRole::Accent => Msg::SettingsColorAccent,
        ColorRole::Success => Msg::SettingsColorSuccess,
        ColorRole::Error => Msg::SettingsColorError,
    }
}

/// The contrast warning of the file, at the row of its text color.
fn contrast_problem(role: ColorRole, view: &SettingsView<'_>) -> Option<String> {
    let warning = view.state.config()?.contrast_warning.as_ref()?;
    (warning.foreground == role).then(|| {
        fill_template(
            view.lang.text(Msg::SettingsContrastLow),
            &[
                (
                    "{background}",
                    view.lang.text(color_label(warning.background)),
                ),
                ("{ratio}", &format!("{:.1}", warning.ratio)),
            ],
        )
    })
}

fn color_row<'a>(role: ColorRole, view: &SettingsView<'a>) -> SettingRow<'a> {
    let palette = view.palette;
    let field = SettingField::Color(role);
    let shown = palette.color(role);
    let swatch = container("")
        .width(SWATCH_SIZE)
        .height(SWATCH_SIZE)
        .style(theme::swatch(shown, palette.border_inactive));
    let control = row![
        swatch,
        setting_field(field, role.field_id(), &hex(shown), true, view)
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center);
    SettingRow {
        label: view.lang.text(color_label(role)).to_owned(),
        theme_name: Some(role.name().to_owned()),
        description: None,
        problem: field_problem(field, view).or_else(|| contrast_problem(role, view)),
        control: control.into(),
        control_width: None,
    }
}

fn program_row<'a>(kind: OpenKind, view: &SettingsView<'a>) -> SettingRow<'a> {
    let lang = view.lang;
    let palette = view.palette;
    let state = view.state;
    let field = SettingField::Program(kind);
    let (label, note, id) = match kind {
        OpenKind::View => (
            Msg::SettingsRowViewer,
            Msg::SettingsRowViewerNote,
            VIEWER_FIELD_ID,
        ),
        OpenKind::Edit => (
            Msg::SettingsRowEditor,
            Msg::SettingsRowEditorNote,
            EDITOR_FIELD_ID,
        ),
    };
    let mut control = row![
        setting_field(
            field,
            id,
            lang.text(Msg::SettingsProgramPlaceholder),
            !state.sandboxed,
            view,
        ),
        kit::button(
            lang.text(Msg::SettingsChoose),
            Some(settings_message(SettingsMessage::PickProgram(kind))),
            Tone::Secondary,
            palette,
        ),
    ]
    .spacing(spacing::S)
    .align_y(alignment::Vertical::Center);
    if state.saved_program(kind).is_some() {
        control = control.push(kit::button(
            lang.text(Msg::SettingsDefault),
            Some(change(SettingChange::Program { kind, line: None })),
            Tone::Secondary,
            palette,
        ));
    }
    let mut description = lang.text(note).to_owned();
    if state.sandboxed {
        description.push(' ');
        description.push_str(lang.text(Msg::SettingsProgramsSandboxNote));
    }
    SettingRow {
        label: lang.text(label).to_owned(),
        theme_name: None,
        description: Some(description),
        problem: field_problem(field, view),
        control: control.into(),
        control_width: Some(SETTINGS_WIDE_CONTROL_WIDTH),
    }
}

fn programs_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    vec![Group {
        title: String::new(),
        rows: vec![
            program_row(OpenKind::View, view),
            program_row(OpenKind::Edit, view),
        ],
    }]
}

fn keys_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let state = if view.keys_changed == 0 {
        lang.text(Msg::SettingsKeysBuiltIn).to_owned()
    } else {
        fill_template(
            lang.text(Msg::SettingsKeysChanged),
            &[("{count}", &view.keys_changed.to_string())],
        )
    };
    vec![Group {
        title: String::new(),
        rows: vec![simple_row(
            Msg::SettingsRowKeys,
            Some(Msg::SettingsRowKeysNote),
            with_button(
                status_text(state, palette.dim_text),
                lang.text(Msg::SettingsKeysEdit),
                Message::KeyBindings(KeyBindingsMessage::Open),
                palette,
            ),
            lang,
        )],
    }]
}

fn language_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let labels: Vec<String> = Language::ALL
        .iter()
        .map(|language| language.code().to_uppercase())
        .collect();
    let options: Vec<(&str, Message)> = Language::ALL
        .iter()
        .zip(&labels)
        .map(|(&language, label)| (label.as_str(), change(SettingChange::Language(language))))
        .collect();
    let chosen = Language::ALL
        .iter()
        .position(|&language| language == lang)
        .unwrap_or_default();
    vec![Group {
        title: String::new(),
        rows: vec![simple_row(
            Msg::SettingsRowLanguage,
            Some(Msg::SettingsRowLanguageNote),
            kit::segmented(&options, chosen, true, view.palette),
            lang,
        )],
    }]
}

fn folder_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let open_setup = || Message::FilingSetup(FilingSetupMessage::Toggle);
    if view.filing.locations.is_empty() {
        return vec![SettingRow {
            label: lang.text(Msg::SettingsFoldersEmpty).to_owned(),
            theme_name: None,
            description: None,
            problem: None,
            control: kit::button(
                lang.text(Msg::SettingsChange),
                Some(open_setup()),
                Tone::Secondary,
                palette,
            ),
            control_width: None,
        }];
    }
    view.filing
        .locations
        .iter()
        .map(|location| {
            let (label, theme_name, note) = match (location.mode, location.is_target) {
                (LocationMode::Inbox, _) => (
                    Msg::SettingsRowInbox,
                    Msg::SettingsRowInboxTheme,
                    Msg::SettingsRowInboxNote,
                ),
                (LocationMode::Archive, true) => (
                    Msg::SettingsRowCentral,
                    Msg::FilingSetupKindCentral,
                    Msg::SettingsRowCentralNote,
                ),
                (LocationMode::Archive, false) => (
                    Msg::SettingsRowRecords,
                    Msg::FilingSetupKindRecords,
                    Msg::SettingsRowRecordsNote,
                ),
            };
            let mut description = lang.text(note).to_owned();
            if location.images {
                description.push(' ');
                description.push_str(lang.text(Msg::SettingsFolderImages));
            }
            let path = tab_label(&location.path, view.home, false, FOLDER_CHARS);
            SettingRow {
                label: lang.text(label).to_owned(),
                theme_name: Some(lang.text(theme_name).to_owned()),
                description: Some(description),
                problem: None,
                control: with_button(
                    mono(path, palette.text),
                    lang.text(Msg::SettingsChange),
                    open_setup(),
                    palette,
                ),
                control_width: None,
            }
        })
        .collect()
}

fn work_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let enabled_row = simple_row(
        Msg::SettingsRowHermesEnabled,
        Some(Msg::SettingsRowHermesEnabledNote),
        switch(
            view.filing.enabled,
            SettingChange::HermesEnabled(!view.filing.enabled),
            view,
        ),
        lang,
    );

    let load = view.filing.load;
    let mut options: Vec<(String, Message)> = [
        (Msg::FilingStatusLoadGentle, Load::Gentle),
        (Msg::FilingStatusLoadBalanced, Load::Balanced),
        (Msg::FilingStatusLoadFull, Load::Full),
    ]
    .into_iter()
    .map(|(name, preset)| {
        (
            lang.text(name).to_owned(),
            change(SettingChange::Load(preset)),
        )
    })
    .collect();
    if let Load::Workers(count) = load {
        options.push((
            fill_template(
                lang.text(Msg::SettingsLoadWorkers),
                &[("{count}", &count.to_string())],
            ),
            change(SettingChange::Load(load)),
        ));
    }
    let chosen = match load {
        Load::Gentle => 0,
        Load::Balanced => 1,
        Load::Full => 2,
        Load::Workers(_) => options.len().saturating_sub(1),
    };
    let shown: Vec<(&str, Message)> = options
        .iter()
        .map(|(label, message)| (label.as_str(), message.clone()))
        .collect();
    let mut load_row = simple_row(
        Msg::SettingsRowLoad,
        Some(Msg::SettingsRowLoadNote),
        kit::segmented(&shown, chosen, true, palette),
        lang,
    );
    load_row.control_width = Some(SETTINGS_WIDE_CONTROL_WIDTH);
    vec![enabled_row, load_row]
}

fn ocr_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let (state, color) = if view.state.ocr_models_present {
        (Msg::SettingsModelsPresent, palette.success)
    } else {
        (Msg::SettingsModelsMissing, palette.accent)
    };
    vec![simple_row(
        Msg::SettingsRowOcrModels,
        Some(Msg::SettingsRowOcrModelsNote),
        with_button(
            status_text(lang.text(state).to_owned(), color),
            lang.text(Msg::SettingsModelsDownload),
            Message::OcrModels(OcrModelsMessage::Open),
            palette,
        ),
        lang,
    )]
}

fn ai_model_state(view: &SettingsView<'_>) -> (String, Color) {
    let lang = view.lang;
    let palette = view.palette;
    match &view.ai_status {
        None => (
            lang.text(Msg::SettingsModelUnknown).to_owned(),
            palette.dim_text,
        ),
        Some(AiStatus::Off) => (lang.text(Msg::SettingsOff).to_owned(), palette.dim_text),
        Some(AiStatus::NotDownloaded) => (
            lang.text(Msg::FilingStatusAiNotDownloaded).to_owned(),
            palette.accent,
        ),
        Some(AiStatus::NotLoadedYet) => (
            lang.text(Msg::FilingStatusAiNotLoaded).to_owned(),
            palette.success,
        ),
        Some(AiStatus::Loaded { name, .. }) => (name.clone(), palette.success),
        Some(AiStatus::Failed(reason)) => ((*reason).to_owned(), palette.error),
        Some(AiStatus::Http { model, .. }) => (model.clone(), palette.text),
    }
}

/// A text row of the AI group.
fn ai_field_row<'a>(
    field: SettingField,
    (label, note, id): (Msg, Msg, &'static str),
    placeholder: &str,
    view: &SettingsView<'a>,
) -> SettingRow<'a> {
    SettingRow {
        problem: field_problem(field, view),
        ..simple_row(
            label,
            Some(note),
            setting_field(field, id, placeholder, true, view),
            view.lang,
        )
    }
}

/// The provider and the keys that belong to it. They can be set while the
/// AI is off, so that switching it on finds them ready.
fn provider_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let providers = [
        (Msg::SettingsProviderLocal, AiProviderName::Local),
        (Msg::SettingsProviderHttp, AiProviderName::Http),
    ];
    let options: Vec<(&str, Message)> = providers
        .iter()
        .map(|&(label, provider)| {
            (
                lang.text(label),
                settings_message(SettingsMessage::ChooseProvider(provider)),
            )
        })
        .collect();
    let provider = view.state.provider();
    let chosen = providers
        .iter()
        .position(|&(_, listed)| listed == provider)
        .unwrap_or_default();
    let mut rows = vec![simple_row(
        Msg::SettingsRowAiProvider,
        Some(Msg::SettingsRowAiProviderNote),
        kit::segmented(&options, chosen, true, view.palette),
        lang,
    )];
    match provider {
        AiProviderName::Local => rows.push(ai_field_row(
            SettingField::AiThreads,
            (
                Msg::SettingsRowAiThreads,
                Msg::SettingsRowAiThreadsNote,
                THREADS_FIELD_ID,
            ),
            lang.text(Msg::SettingsThreadsPlaceholder),
            view,
        )),
        AiProviderName::Http => {
            rows.push(ai_field_row(
                SettingField::AiEndpoint,
                (
                    Msg::SettingsRowAiEndpoint,
                    Msg::SettingsRowAiEndpointNote,
                    ENDPOINT_FIELD_ID,
                ),
                "",
                view,
            ));
            rows.push(ai_field_row(
                SettingField::AiModel,
                (
                    Msg::SettingsRowAiModelName,
                    Msg::SettingsRowAiModelNameNote,
                    MODEL_FIELD_ID,
                ),
                "",
                view,
            ));
        }
    }
    rows
}

fn ai_rows<'a>(view: &SettingsView<'a>) -> Vec<SettingRow<'a>> {
    let lang = view.lang;
    let palette = view.palette;
    let on = view.filing.ai != AiSetting::Off;
    let mut rows = vec![simple_row(
        Msg::SettingsRowAiEnabled,
        Some(Msg::SettingsRowAiEnabledNote),
        switch(on, SettingChange::AiEnabled(!on), view),
        lang,
    )];
    rows.extend(provider_rows(view));
    if !on {
        return rows;
    }
    let (state, color) = ai_model_state(view);
    let shown = status_text(state, color);
    let control = match view.filing.ai {
        AiSetting::Local { .. } => with_button(
            shown,
            lang.text(Msg::SettingsModelsDownload),
            Message::AiModel(AiModelMessage::Open),
            palette,
        ),
        AiSetting::Off | AiSetting::Http { .. } => shown,
    };
    rows.push(simple_row(
        Msg::SettingsRowAiModel,
        Some(Msg::SettingsRowAiModelNote),
        control,
        lang,
    ));
    rows
}

fn hermes_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    vec![
        Group {
            title: lang.text(Msg::SettingsGroupFolders).to_owned(),
            rows: folder_rows(view),
        },
        Group {
            title: lang.text(Msg::SettingsGroupWork).to_owned(),
            rows: work_rows(view),
        },
        Group {
            title: lang.text(Msg::SettingsGroupOcr).to_owned(),
            rows: ocr_rows(view),
        },
        Group {
            title: lang.text(Msg::SettingsGroupAi).to_owned(),
            rows: ai_rows(view),
        },
    ]
}

fn groups_of<'a>(feature: SettingsFeature, view: &SettingsView<'a>) -> Vec<Group<'a>> {
    match feature {
        SettingsFeature::Look => look_groups(view),
        SettingsFeature::Language => language_groups(view),
        SettingsFeature::Programs => programs_groups(view),
        SettingsFeature::Keys => keys_groups(view),
        SettingsFeature::Hermes => hermes_groups(view),
    }
}

fn contains_query(haystack: &str, query: &str) -> bool {
    haystack.to_lowercase().contains(query)
}

/// Whether `feature` is in the list for `query`: its own name, or the name of
/// any of its settings, matches.
fn feature_matches(feature: SettingsFeature, query: &str, view: &SettingsView<'_>) -> bool {
    if query.is_empty() || contains_query(view.lang.text(feature_label(feature)), query) {
        return true;
    }
    groups_of(feature, view)
        .iter()
        .flat_map(|group| &group.rows)
        .any(|setting| row_matches(setting, query))
}

fn row_matches(setting: &SettingRow<'_>, query: &str) -> bool {
    contains_query(&setting.label, query)
        || setting
            .theme_name
            .as_deref()
            .is_some_and(|name| contains_query(name, query))
}

/// The features the search leaves, in their order.
fn listed_features(view: &SettingsView<'_>) -> Vec<SettingsFeature> {
    let query = view.state.query.trim().to_lowercase();
    SettingsFeature::ALL
        .into_iter()
        .filter(|&feature| feature_matches(feature, &query, view))
        .collect()
}

fn nav<'a>(
    listed: &[SettingsFeature],
    shown: Option<SettingsFeature>,
    view: &SettingsView<'a>,
) -> Element<'a, Message> {
    let palette = view.palette;
    let lang = view.lang;
    let message = |query: String| Message::Settings(SettingsMessage::SearchChanged(query));
    let search = kit::text_field(
        lang.text(Msg::SettingsSearchPlaceholder),
        &view.state.query,
        SEARCH_FIELD_ID,
        Some(message),
        message(view.state.query.clone()),
        palette,
    );
    let mut entries = Column::new().spacing(NAV_ENTRY_GAP);
    for &feature in listed {
        let mut label = row![text(lang.text(feature_label(feature)).to_owned())
            .size(font_size::BODY)
            .wrapping(Wrapping::None)]
        .spacing(spacing::S)
        .align_y(alignment::Vertical::Center);
        if let Some(theme_name) = feature_theme_name(feature) {
            label = label.push(
                text(lang.text(theme_name).to_owned())
                    .size(font_size::SMALL)
                    .color(palette.dim_text)
                    .wrapping(Wrapping::None),
            );
        }
        entries = entries.push(
            button(container(label).center_y(Length::Fill))
                .on_press(Message::Settings(SettingsMessage::Select(feature)))
                .width(Length::Fill)
                .height(size::SIDEBAR_ITEM)
                .padding([0.0, spacing::CELL_PADDING_X])
                .style(theme::feature_item(palette, shown == Some(feature))),
        );
    }
    let footer = text(lang.text(Msg::SettingsNavFooter))
        .size(font_size::SMALL)
        .color(palette.dim_text)
        .wrapping(Wrapping::Word);
    container(
        column![
            container(search).padding(Padding {
                bottom: spacing::M,
                ..Padding::ZERO
            }),
            entries,
            container("").height(Length::Fill),
            container(footer).padding(spacing::M),
        ]
        .height(Length::Fill),
    )
    .padding([NAV_PADDING_Y, spacing::M])
    .width(SETTINGS_NAV_WIDTH)
    .height(Length::Fill)
    .style(theme::statusbar(palette))
    .into()
}

fn heading<'a>(feature: SettingsFeature, view: &SettingsView<'a>) -> Element<'a, Message> {
    let palette = view.palette;
    let lang = view.lang;
    let mut title = row![text(lang.text(feature_label(feature)).to_owned())
        .size(font_size::FIGURE)
        .font(Font::DEFAULT)]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    if let Some(theme_name) = feature_theme_name(feature) {
        title = title.push(
            text(lang.text(theme_name).to_owned())
                .size(font_size::TITLE)
                .color(palette.dim_text),
        );
    }
    title = title.push(container("").width(Length::Fill)).push(mono(
        feature_config_place(feature).to_owned(),
        palette.dim_text,
    ));
    column![
        title,
        kit::dim(lang.text(feature_intro(feature)).to_owned(), palette)
    ]
    .spacing(spacing::XS)
    .into()
}

fn page<'a>(shown: Option<SettingsFeature>, view: &SettingsView<'a>) -> Element<'a, Message> {
    let palette = view.palette;
    let lang = view.lang;
    let Some(feature) = shown else {
        return kit::dim(lang.text(Msg::SettingsNothingFound).to_owned(), palette);
    };
    let query = view.state.query.trim().to_lowercase();
    let whole_feature =
        query.is_empty() || contains_query(lang.text(feature_label(feature)), &query);
    let mut body = Column::new().push(heading(feature, view));
    if let Err(broken) = &view.state.file {
        body = body.push(
            container(kit::warning_box(config_broken_text(broken, view), palette)).padding(
                Padding {
                    top: spacing::M,
                    ..Padding::ZERO
                },
            ),
        );
    }
    let mut first = true;
    for group in groups_of(feature, view) {
        let rows: Vec<Element<'a, Message>> = group
            .rows
            .into_iter()
            .filter(|setting| whole_feature || row_matches(setting, &query))
            .map(|setting| {
                kit::form_row_with_problem(
                    &setting.label,
                    setting.theme_name.as_deref(),
                    setting.description.as_deref(),
                    setting.problem.as_deref(),
                    setting.control,
                    setting.control_width.unwrap_or(size::CONTROL_WIDTH),
                    palette,
                )
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        let content = Column::with_children(rows);
        // A group without a title would only repeat its row's label.
        if group.title.is_empty() {
            body = body.push(container(content).padding(Padding {
                top: spacing::M,
                ..Padding::ZERO
            }));
            first = false;
            continue;
        }
        body = body.push(if first {
            kit::first_section(&group.title, content, palette)
        } else {
            kit::section(&group.title, content, palette)
        });
        first = false;
    }
    body.spacing(spacing::XS).into()
}

fn config_broken_text(broken: &ConfigError, view: &SettingsView<'_>) -> String {
    let path = shown_path(broken.path(), view.home);
    match broken.line() {
        Some(line) => fill_template(
            view.lang.text(Msg::SettingsConfigBrokenAtLine),
            &[
                ("{path}", &path),
                ("{line}", &line.to_string()),
                ("{reason}", broken.reason()),
            ],
        ),
        None => fill_template(
            view.lang.text(Msg::SettingsConfigBroken),
            &[("{path}", &path), ("{reason}", broken.reason())],
        ),
    }
}

/// The window area under the title bar and above the status bar.
pub fn view<'a>(view: &SettingsView<'a>) -> Element<'a, Message> {
    let palette = view.palette;
    let listed = listed_features(view);
    let shown = listed
        .iter()
        .copied()
        .find(|&feature| feature == view.state.feature)
        .or_else(|| listed.first().copied());
    let content = scrollable(
        container(page(shown, view))
            .max_width(SETTINGS_PAGE_MAX_WIDTH)
            .padding(Padding {
                top: spacing::XL,
                bottom: spacing::XL,
                left: spacing::XL * 2.0,
                right: spacing::XL * 2.0,
            }),
    )
    .direction(Direction::Vertical(
        Scrollbar::new()
            .width(SCROLLBAR_WIDTH)
            .scroller_width(SCROLLBAR_WIDTH)
            .margin(SCROLLBAR_MARGIN)
            .spacing(0.0),
    ))
    .width(Length::Fill)
    .height(Length::Fill);
    row![
        nav(&listed, shown, view),
        theme::vertical_rule(palette.border_inactive),
        content,
    ]
    .height(Length::Fill)
    .into()
}

/// What the status bar shows while the tab is open.
pub struct StatusBar<'a> {
    pub changed: usize,
    /// A failed write, which takes the place of the hint.
    pub failure: Option<&'a str>,
    pub notice: Option<&'a str>,
    pub lang: Language,
    pub palette: &'a Palette,
}

pub fn status_bar<'a>(bar: &StatusBar<'a>) -> Element<'a, Message> {
    let palette = bar.palette;
    let lang = bar.lang;
    let small = |content: String, color: Color| {
        container(
            text(content)
                .size(font_size::SMALL)
                .color(color)
                .wrapping(Wrapping::None),
        )
        .padding([0.0, spacing::L])
    };
    let mode = container(
        text(lang.text(Msg::SettingsMode))
            .size(font_size::SMALL)
            .wrapping(Wrapping::None),
    )
    .padding([0.0, spacing::L])
    .height(Length::Fill)
    .align_y(alignment::Vertical::Center)
    .style(theme::mode_field_accent(palette));
    let middle = match (bar.failure, bar.notice) {
        (Some(failure), _) => small(failure.to_owned(), palette.error),
        (None, Some(notice)) => small(format!("\u{2713} {notice}"), palette.success),
        (None, None) => small(
            lang.text(Msg::SettingsStatusHint).to_owned(),
            palette.dim_text,
        ),
    };
    let mut line =
        row![mode, middle, container("").width(Length::Fill)].align_y(alignment::Vertical::Center);
    if bar.changed > 0 {
        line = line.push(small(
            fill_template(
                lang.text(Msg::SettingsStatusChanged),
                &[("{count}", &bar.changed.to_string())],
            ),
            palette.dim_text,
        ));
    }
    container(column![
        theme::horizontal_rule(palette.border_inactive),
        container(line).width(Length::Fill).height(Length::Fill),
    ])
    .height(STATUSBAR_HEIGHT)
    .width(Length::Fill)
    .style(theme::statusbar(palette))
    .into()
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    fn state_of(source: &str) -> SettingsState {
        SettingsState::new(
            false,
            Ok(Box::new(crate::config::from_text(source).unwrap())),
            false,
        )
    }

    fn typed(state: &mut SettingsState, field: SettingField, text: &str) -> Option<SettingChange> {
        state.edit(field, text.to_owned());
        state.submit(field)
    }

    #[test]
    fn a_field_shows_the_file_until_something_is_typed() {
        let mut state = state_of("[open]\nview = [\"less\", \"-R\"]\n[theme]\naccent = \"#abc\"\n");
        let viewer = SettingField::Program(OpenKind::View);
        assert_eq!(state.field_text(viewer), "less -R");
        assert_eq!(
            state.field_text(SettingField::Color(ColorRole::Accent)),
            "#abc"
        );
        state.edit(viewer, "bat".to_owned());
        assert_eq!(state.field_text(viewer), "bat");
    }

    #[test]
    fn leaving_writes_what_was_typed_without_enter_and_skips_what_did_not_change() {
        let mut state = state_of("[open]\nview = [\"less\", \"-R\"]\n");
        state.edit(
            SettingField::Program(OpenKind::View),
            " less -R ".to_owned(),
        );
        state.edit(SettingField::Color(ColorRole::Accent), "#ff0".to_owned());
        state.edit(SettingField::Program(OpenKind::Edit), "vim".to_owned());
        assert_eq!(
            state.leave(),
            Some(vec![
                SettingChange::Program {
                    kind: OpenKind::Edit,
                    line: Some(ProgramLine::new("vim", &[])),
                },
                SettingChange::Color {
                    role: ColorRole::Accent,
                    color: Some("#ff0".to_owned()),
                },
            ])
        );
    }

    #[test]
    fn leaving_with_a_wrong_value_writes_nothing_and_shows_it() {
        let mut state = state_of("");
        let accent = SettingField::Color(ColorRole::Accent);
        state.edit(accent, "red".to_owned());
        state.edit(SettingField::Program(OpenKind::View), "bat".to_owned());
        assert_eq!(state.leave(), None);
        assert_eq!(state.problems.get(&accent), Some(&FieldProblem::Color));
    }

    #[test]
    fn leaving_writes_an_own_server_once_and_not_the_threads_it_hides() {
        let mut state = state_of("");
        state.edit(SettingField::AiThreads, "4".to_owned());
        assert_eq!(state.choose_provider(AiProviderName::Http), None);
        state.edit(
            SettingField::AiEndpoint,
            "http://localhost:11434".to_owned(),
        );
        state.edit(SettingField::AiModel, "qwen".to_owned());
        assert_eq!(
            state.leave(),
            Some(vec![SettingChange::AiProvider(AiProviderChoice::Http {
                endpoint: "http://localhost:11434".to_owned(),
                model: "qwen".to_owned(),
            })])
        );
    }

    #[test]
    fn enter_without_typing_writes_nothing() {
        let mut state = state_of("");
        assert_eq!(state.submit(SettingField::AiThreads), None);
        assert!(state.problems.is_empty());
    }

    #[test]
    fn a_wrong_value_stays_at_its_row_and_is_not_written() {
        let mut state = state_of("");
        let accent = SettingField::Color(ColorRole::Accent);
        assert_eq!(typed(&mut state, accent, "red"), None);
        assert_eq!(state.problems.get(&accent), Some(&FieldProblem::Color));
        let threads = SettingField::AiThreads;
        assert_eq!(typed(&mut state, threads, "0"), None);
        assert_eq!(state.problems.get(&threads), Some(&FieldProblem::Threads));
        let editor = SettingField::Program(OpenKind::Edit);
        assert_eq!(typed(&mut state, editor, "bin/edit"), None);
        assert_eq!(
            state.problems.get(&editor),
            Some(&FieldProblem::Program(ProgramLineProblem::RelativePath))
        );
        state.edit(accent, "#ff0".to_owned());
        assert!(!state.problems.contains_key(&accent));
    }

    #[test]
    fn a_right_value_becomes_its_change_and_an_empty_one_drops_the_line() {
        let mut state = state_of("");
        assert_eq!(
            typed(&mut state, SettingField::Color(ColorRole::Text), " #FFF "),
            Some(SettingChange::Color {
                role: ColorRole::Text,
                color: Some("#FFF".to_owned()),
            })
        );
        assert_eq!(
            typed(&mut state, SettingField::Program(OpenKind::View), "  "),
            Some(SettingChange::Program {
                kind: OpenKind::View,
                line: None,
            })
        );
        assert_eq!(
            typed(&mut state, SettingField::AiThreads, "4"),
            Some(SettingChange::AiProvider(AiProviderChoice::Local {
                threads: NonZeroU32::new(4),
            }))
        );
    }

    #[test]
    fn an_own_server_waits_for_its_address_and_model() {
        let mut state = state_of("");
        assert_eq!(state.choose_provider(AiProviderName::Http), None);
        assert_eq!(state.provider(), AiProviderName::Http);
        let endpoint = SettingField::AiEndpoint;
        assert_eq!(typed(&mut state, endpoint, "http://localhost:11434"), None);
        assert_eq!(
            state.problems.get(&SettingField::AiModel),
            Some(&FieldProblem::Needed)
        );
        assert_eq!(typed(&mut state, endpoint, "https://example.com"), None);
        assert_eq!(
            state.problems.get(&endpoint),
            Some(&FieldProblem::Endpoint(AiEndpointProblem::NotLocal))
        );
        state.edit(endpoint, "http://127.0.0.1:8080".to_owned());
        assert_eq!(
            typed(&mut state, SettingField::AiModel, "qwen"),
            Some(SettingChange::AiProvider(AiProviderChoice::Http {
                endpoint: "http://127.0.0.1:8080".to_owned(),
                model: "qwen".to_owned(),
            }))
        );
    }

    #[test]
    fn going_back_to_this_machine_is_written_at_once_and_only_when_it_changes() {
        let mut local = state_of("");
        assert_eq!(local.choose_provider(AiProviderName::Local), None);
        let mut http = state_of(
            "[modules.filing.ai]\nprovider = \"http\"\nendpoint = \"http://localhost:1\"\nmodel = \"m\"\n",
        );
        assert_eq!(
            http.choose_provider(AiProviderName::Local),
            Some(SettingChange::AiProvider(AiProviderChoice::Local {
                threads: None
            }))
        );
    }

    #[test]
    fn a_written_change_settles_its_fields_and_a_refused_one_shows_at_them() {
        let mut state = state_of("");
        let field = SettingField::Color(ColorRole::Accent);
        let change = typed(&mut state, field, "#123").unwrap();
        assert!(state.refused(&change, "disk full".to_owned()));
        assert_eq!(
            state.problems.get(&field),
            Some(&FieldProblem::Write("disk full".to_owned()))
        );
        let config = crate::config::from_text("[theme]\naccent = \"#123\"\n").unwrap();
        state.written(&change, &config);
        assert!(state.drafts.is_empty() && state.problems.is_empty());
        assert_eq!(state.saved_color(ColorRole::Accent), Some("#123"));
        assert_eq!(state.changed, 1);
        assert!(!state.refused(&SettingChange::VscodeTheme(None), String::new()));
    }

    #[test]
    fn a_picked_app_is_opened_with_open_and_the_app_store_build_takes_nothing_else() {
        let mut state = state_of("");
        let tool = state.picked_program(OpenKind::Edit, Path::new("/usr/local/bin/hx"));
        assert_eq!(
            tool,
            Some(SettingChange::Program {
                kind: OpenKind::Edit,
                line: Some(ProgramLine::new("/usr/local/bin/hx", &[])),
            })
        );
        state.sandboxed = true;
        assert_eq!(
            state.picked_program(OpenKind::Edit, Path::new("/usr/local/bin/hx")),
            None
        );
        assert_eq!(
            state.problems.get(&SettingField::Program(OpenKind::Edit)),
            Some(&FieldProblem::OnlyApps)
        );
        if cfg!(target_os = "macos") {
            assert_eq!(
                state.picked_program(OpenKind::View, Path::new("/Applications/Preview.app")),
                Some(SettingChange::Program {
                    kind: OpenKind::View,
                    line: Some(ProgramLine::opening_app("/Applications/Preview.app")),
                })
            );
        }
    }
}
