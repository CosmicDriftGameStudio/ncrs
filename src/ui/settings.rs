//! The settings tab: a feature list on the left, the settings of the chosen
//! feature on the right, laid out like the pages of the other cards. It holds
//! only settings the config file has; every change is written there.

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
use crate::config::{AiSetting, Filing};
use crate::filing::AiStatus;
use crate::i18n::{Language, Msg};
use crate::messages::{
    AiModelMessage, FilingSetupMessage, Message, OcrModelsMessage, SettingChange, SettingsFeature,
    SettingsMessage, ThemeFetchMessage,
};
use crate::palette::{Palette, ThemePreset};

/// The id of the search field above the feature list.
pub const SEARCH_FIELD_ID: &str = "settings-search";

/// The section or key of the config file a feature writes to, shown at the
/// right of its heading. These are file syntax, not words to translate.
const THEME_SECTION: &str = "[theme]";
const LANGUAGE_KEY: &str = "lang";
const FILING_SECTION: &str = "[modules.filing]";

/// How many characters of a folder fit beside its button.
const FOLDER_CHARS: usize = 22;
/// How many characters of the theme file name fit beside its button.
const THEME_FILE_CHARS: usize = 26;
/// Room the feature list keeps around its entries.
const NAV_PADDING_Y: f32 = 10.0;
const NAV_ENTRY_GAP: f32 = 1.0;

/// What the tab remembers while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsState {
    pub feature: SettingsFeature,
    pub query: String,
    /// Settings changed since the tab was opened.
    pub changed: usize,
    /// Whether the folder of the text recognition models has files; looked up
    /// when the tab opens and after a download, not on every frame.
    pub ocr_models_present: bool,
}

impl SettingsState {
    pub fn new(ocr_models_present: bool) -> Self {
        Self {
            feature: SettingsFeature::Look,
            query: String::new(),
            changed: 0,
            ocr_models_present,
        }
    }
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
    pub lang: Language,
    pub palette: &'a Palette,
}

struct SettingRow<'a> {
    label: String,
    theme_name: Option<String>,
    description: Option<String>,
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

fn feature_label(feature: SettingsFeature) -> Msg {
    match feature {
        SettingsFeature::Look => Msg::SettingsFeatureLook,
        SettingsFeature::Language => Msg::SettingsFeatureLanguage,
        SettingsFeature::Hermes => Msg::SettingsFeatureHermes,
    }
}

fn feature_theme_name(feature: SettingsFeature) -> Option<Msg> {
    match feature {
        SettingsFeature::Hermes => Some(Msg::GroupHermesTheme),
        SettingsFeature::Look | SettingsFeature::Language => None,
    }
}

fn feature_intro(feature: SettingsFeature) -> Msg {
    match feature {
        SettingsFeature::Look => Msg::SettingsFeatureLookIntro,
        SettingsFeature::Language => Msg::SettingsFeatureLanguageIntro,
        SettingsFeature::Hermes => Msg::SettingsFeatureHermesIntro,
    }
}

fn feature_config_place(feature: SettingsFeature) -> &'static str {
    match feature {
        SettingsFeature::Look => THEME_SECTION,
        SettingsFeature::Language => LANGUAGE_KEY,
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

    let file = match view.vscode_theme {
        Some(path) => mono(ellipsize_middle(path, THEME_FILE_CHARS), palette.dim_text),
        None => status_text(
            lang.text(Msg::SettingsVscodeNone).to_owned(),
            palette.dim_text,
        ),
    };
    let vscode_row = simple_row(
        Msg::SettingsRowVscode,
        Some(Msg::SettingsRowVscodeNote),
        with_button(
            file,
            lang.text(Msg::SettingsVscodeFetch),
            Message::ThemeFetch(ThemeFetchMessage::Open),
            palette,
        ),
        lang,
    );
    vec![Group {
        title: String::new(),
        rows: vec![preset_row, vscode_row],
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
    let provider = match &view.filing.ai {
        AiSetting::Off => return rows,
        AiSetting::Local { .. } => Msg::SettingsProviderLocal,
        AiSetting::Http { .. } => Msg::SettingsProviderHttp,
    };
    rows.push(simple_row(
        Msg::SettingsRowAiProvider,
        Some(Msg::SettingsRowAiProviderNote),
        status_text(lang.text(provider).to_owned(), palette.text),
        lang,
    ));
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
    let mut first = true;
    for group in groups_of(feature, view) {
        let rows: Vec<Element<'a, Message>> = group
            .rows
            .into_iter()
            .filter(|setting| whole_feature || row_matches(setting, &query))
            .map(|setting| {
                kit::form_row_wide(
                    &setting.label,
                    setting.theme_name.as_deref(),
                    setting.description.as_deref(),
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
