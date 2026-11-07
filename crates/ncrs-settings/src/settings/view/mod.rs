mod groups;
mod hermes_page;
mod index_page;
mod page;
mod system_page;

pub use groups::server_dialog_body;
use groups::*;
use hermes_page::hermes_groups;
use index_page::index_groups;
use page::*;
use system_page::system_groups;

use crate::settings::*;

use std::path::Path;

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::Wrapping;
use iced::widget::{container, row, scrollable, text, Column};
use iced::{Color, Element, Length, Padding};

use crate::facts::DocumentsFacts;
use crate::message::{SettingChange, SettingField, SettingsMessage};
use ncrs_config::config::{AiEndpointProblem, Filing, ProgramLineProblem, ServerEntry};
use ncrs_config::{SettingsFeature, SettingsGroup};
use ncrs_i18n::fill_template;
use ncrs_i18n::{FuturamaName, Language, Msg};
use ncrs_ui_kit::format::shown_path;
use ncrs_ui_kit::frame::{SCROLLBAR_MARGIN, SCROLLBAR_WIDTH};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::layout::SETTINGS_PAGE_MAX_WIDTH;
use ncrs_ui_kit::page_status_bar::{page_status_bar, PageStatusBar, PageStatusLine};
use ncrs_ui_kit::theme::{self, font_size, spacing};

type Message = SettingsMessage;
use ncrs_ui_kit::palette::{Palette, ThemePreset};

/// How the index of filing is protected: the state, what it means, and the
/// buttons that change it. The app words it; a button sends its position.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtectionPanel {
    pub state: String,
    pub good: bool,
    pub sentence: String,
    pub failure: Option<String>,
    pub buttons: Vec<ProtectionButton>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectionButton {
    pub label: Msg,
    pub enabled: bool,
    pub primary: bool,
}

/// Everything the tab shows that does not live in [`SettingsState`].
#[derive(Debug)]
pub struct SettingsView<'a> {
    pub state: &'a SettingsState,
    pub preset: ThemePreset,
    pub vscode_theme: Option<&'a str>,
    pub filing: &'a Filing,
    /// The saved SFTP servers.
    pub servers: &'a [ServerEntry],
    /// How the index of filing is protected, while filing has a module.
    pub protection: Option<ProtectionPanel>,
    /// What the Index and Hermes pages show that the config does not say.
    pub facts: DocumentsFacts,
    pub home: &'a Path,
    /// How many actions have keys other than their built-in ones.
    pub keys_changed: usize,
    /// Whether the Updates page is in the list; the App Store build makes no
    /// request and leaves it out.
    pub updates_listed: bool,
    pub lang: Language,
    pub palette: &'a Palette,
}

type SettingRow<'a> = ncrs_ui_kit::setting_rows::SettingRow<'a, Message>;
type Group<'a> = ncrs_ui_kit::setting_rows::Group<'a, Message>;

fn change(setting: SettingChange) -> Message {
    SettingsMessage::Change(setting)
}

/// A text field of the tab: typing keeps a draft, Enter checks and writes it.
fn setting_field<'a>(
    field: SettingField,
    id: &'static str,
    placeholder: &str,
    typing: bool,
    view: &SettingsView<'_>,
) -> Element<'a, Message> {
    let on_input = typing.then_some(move |text| SettingsMessage::Edit { field, text });
    kit::mono_text_field(
        placeholder,
        &view.state.field_text(field),
        id,
        on_input,
        SettingsMessage::Submit(field),
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
        FieldProblem::Endpoint(AiEndpointProblem::Unusable(reason)) => {
            return fill_template(
                lang.text(Msg::SettingsProblemEndpoint),
                &[("{reason}", reason)],
            );
        }
        FieldProblem::Endpoint(AiEndpointProblem::NotLocal) => Msg::SettingsProblemEndpointNotLocal,
        FieldProblem::Needed => Msg::SettingsProblemNeeded,
        FieldProblem::NotAnIban => Msg::SettingsProblemIban,
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
        SettingsFeature::Servers => Msg::SettingsFeatureServers,
        SettingsFeature::System => Msg::SettingsFeatureSystem,
        SettingsFeature::Updates => Msg::SettingsFeatureUpdates,
        SettingsFeature::Index => Msg::SettingsFeatureIndex,
        SettingsFeature::Hermes => Msg::SettingsFeatureHermes,
        SettingsFeature::Rules => Msg::SettingsFeatureRules,
        SettingsFeature::Tags => Msg::SettingsFeatureTags,
        SettingsFeature::Reports => Msg::SettingsFeatureReports,
        SettingsFeature::ReadErrors => Msg::SettingsFeatureReadErrors,
    }
}

fn feature_theme_name(feature: SettingsFeature) -> Option<FuturamaName> {
    match feature {
        SettingsFeature::Hermes => Some(FuturamaName::GroupHermes),
        SettingsFeature::Look
        | SettingsFeature::Language
        | SettingsFeature::Programs
        | SettingsFeature::Keys
        | SettingsFeature::Servers
        | SettingsFeature::System
        | SettingsFeature::Updates
        | SettingsFeature::Index
        | SettingsFeature::Rules
        | SettingsFeature::Tags
        | SettingsFeature::Reports
        | SettingsFeature::ReadErrors => None,
    }
}

fn group_label(group: SettingsGroup) -> Msg {
    match group {
        SettingsGroup::General => Msg::SettingsGroupGeneral,
        SettingsGroup::Files => Msg::SettingsGroupFiles,
        SettingsGroup::Documents => Msg::SettingsGroupDocuments,
        SettingsGroup::Hermes => Msg::SettingsGroupHermes,
    }
}

/// The dim word beside the name of the page, in the list and in the heading.
fn feature_suffix(feature: SettingsFeature) -> Option<Msg> {
    match feature {
        SettingsFeature::Hermes => Some(Msg::SettingsFeatureHermesSorting),
        SettingsFeature::Look
        | SettingsFeature::Language
        | SettingsFeature::Programs
        | SettingsFeature::Keys
        | SettingsFeature::Servers
        | SettingsFeature::System
        | SettingsFeature::Updates
        | SettingsFeature::Index
        | SettingsFeature::Rules
        | SettingsFeature::Tags
        | SettingsFeature::Reports
        | SettingsFeature::ReadErrors => None,
    }
}

fn feature_beta_mark(feature: SettingsFeature) -> Option<Msg> {
    match feature {
        SettingsFeature::Hermes => Some(Msg::HermesBeta),
        SettingsFeature::Look
        | SettingsFeature::Language
        | SettingsFeature::Programs
        | SettingsFeature::Keys
        | SettingsFeature::Servers
        | SettingsFeature::System
        | SettingsFeature::Updates
        | SettingsFeature::Index
        | SettingsFeature::Rules
        | SettingsFeature::Tags
        | SettingsFeature::Reports
        | SettingsFeature::ReadErrors => None,
    }
}

fn feature_intro(feature: SettingsFeature) -> Msg {
    match feature {
        SettingsFeature::Look => Msg::SettingsFeatureLookIntro,
        SettingsFeature::Language => Msg::SettingsFeatureLanguageIntro,
        SettingsFeature::Programs => Msg::SettingsFeatureProgramsIntro,
        SettingsFeature::Keys => Msg::SettingsFeatureKeysIntro,
        SettingsFeature::Servers => Msg::SettingsFeatureServersIntro,
        SettingsFeature::System => Msg::SettingsFeatureSystemIntro,
        SettingsFeature::Updates => Msg::SettingsFeatureUpdatesIntro,
        SettingsFeature::Index => Msg::SettingsFeatureIndexIntro,
        SettingsFeature::Hermes => Msg::SettingsFeatureHermesIntro,
        SettingsFeature::Rules => Msg::SettingsFeatureRulesIntro,
        SettingsFeature::Tags => Msg::SettingsFeatureTagsIntro,
        SettingsFeature::Reports => Msg::SettingsFeatureReportsIntro,
        SettingsFeature::ReadErrors => Msg::SettingsFeatureReadErrorsIntro,
    }
}

/// Where the page's values live in the config file; the pages the app fills
/// show data that is not in it.
fn feature_config_place(feature: SettingsFeature) -> Option<&'static str> {
    match feature {
        SettingsFeature::Look => Some(THEME_SECTION),
        SettingsFeature::Language => Some(LANGUAGE_KEY),
        SettingsFeature::Programs => Some(OPEN_SECTION),
        SettingsFeature::Keys => Some(KEYS_SECTION),
        SettingsFeature::Servers => Some(SERVERS_SECTION),
        SettingsFeature::System => Some(SYSTEM_SECTION),
        SettingsFeature::Index | SettingsFeature::Hermes => Some(FILING_SECTION),
        SettingsFeature::Rules
        | SettingsFeature::Tags
        | SettingsFeature::Reports
        | SettingsFeature::ReadErrors
        | SettingsFeature::Updates => None,
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
        .font(ncrs_ui_kit::APP_FONT)
        .color(color)
        .wrapping(Wrapping::None)
        .into()
}

use ncrs_ui_kit::setting_rows::{simple_row, status_text, with_button};

fn switch<'a>(on: bool, setting: SettingChange, view: &SettingsView<'_>) -> Element<'a, Message> {
    ncrs_ui_kit::setting_rows::switch(on, change(setting), view.lang, view.palette)
}

fn groups_of<'a>(feature: SettingsFeature, view: &SettingsView<'a>) -> Vec<Group<'a>> {
    match feature {
        SettingsFeature::Look => look_groups(view),
        SettingsFeature::Language => language_groups(view),
        SettingsFeature::Programs => programs_groups(view),
        SettingsFeature::Keys => keys_groups(view),
        SettingsFeature::Servers => servers_groups(view),
        SettingsFeature::System => system_groups(view),
        SettingsFeature::Index => index_groups(view),
        SettingsFeature::Hermes => hermes_groups(view),
        SettingsFeature::Rules
        | SettingsFeature::Tags
        | SettingsFeature::Reports
        | SettingsFeature::ReadErrors
        | SettingsFeature::Updates => Vec::new(),
    }
}

fn contains_query(haystack: &str, query: &str) -> bool {
    haystack.to_lowercase().contains(query)
}

/// Whether `feature` is in the list for `query`: its own name, or the name of
/// any of its settings, matches.
fn feature_matches(feature: SettingsFeature, query: &str, view: &SettingsView<'_>) -> bool {
    if query.is_empty() || feature_name_matches(feature, query, view.lang) {
        return true;
    }
    groups_of(feature, view)
        .iter()
        .flat_map(|group| &group.rows)
        .any(|setting| row_matches(setting, query))
}

/// The page's label or its theme name contains the query (lower case).
fn feature_name_matches(feature: SettingsFeature, query: &str, lang: Language) -> bool {
    contains_query(lang.text(feature_label(feature)), query)
        || feature_theme_name(feature)
            .and_then(|name| lang.futurama_name(name))
            .is_some_and(|name| contains_query(name, query))
}

fn row_matches(setting: &SettingRow<'_>, query: &str) -> bool {
    contains_query(&setting.label, query)
        || setting
            .theme_name
            .iter()
            .chain(&setting.config_key)
            .any(|name| contains_query(name, query))
}

pub use ncrs_config::SettingsTarget;

/// Every page and every row of every page, from the same rows the tab's search
/// goes through.
pub fn search_targets(view: &SettingsView<'_>) -> Vec<SettingsTarget> {
    let lang = view.lang;
    let mut targets = Vec::new();
    for feature in SettingsFeature::ALL
        .into_iter()
        .filter(|&feature| is_listed(feature, view))
    {
        let page = lang.text(feature_label(feature));
        targets.push(SettingsTarget {
            feature,
            row: None,
            label: fill_template(
                lang.text(Msg::CommandPaletteSettingsPage),
                &[("{page}", page)],
            ),
            theme_name: feature_theme_name(feature)
                .and_then(|name| lang.futurama_name(name))
                .map(str::to_owned),
        });
        let rows = groups_of(feature, view)
            .into_iter()
            .flat_map(|group| group.rows);
        targets.extend(
            rows.filter(|setting| setting.in_command_list)
                .map(|setting| SettingsTarget {
                    feature,
                    row: Some(setting.label.clone()),
                    label: setting.label,
                    theme_name: setting.theme_name,
                }),
        );
    }
    targets
}

/// Every page is in the list but the Updates page of a build that makes no
/// update check.
fn is_listed(feature: SettingsFeature, view: &SettingsView<'_>) -> bool {
    feature != SettingsFeature::Updates || view.updates_listed
}

/// The features the search leaves, in their order.
fn listed_features(view: &SettingsView<'_>) -> Vec<SettingsFeature> {
    let query = view.state.query.trim().to_lowercase();
    SettingsFeature::ALL
        .into_iter()
        .filter(|&feature| is_listed(feature, view))
        .filter(|&feature| feature_matches(feature, &query, view))
        .collect()
}

/// The page the window shows: the chosen one, or the first the search leaves.
pub fn shown_feature(view: &SettingsView<'_>) -> Option<SettingsFeature> {
    let listed = listed_features(view);
    listed
        .iter()
        .copied()
        .find(|&feature| feature == view.state.feature)
        .or_else(|| listed.first().copied())
}

/// The window area under the title bar and above the status bar.
pub fn view<'a>(view: &SettingsView<'a>) -> Element<'a, Message> {
    view_with(view, |message| message, |_| None)
}

/// Like [`view`], with the rows of the pages the app fills (see
/// `SettingsFeature::is_filled_by_app`) put in by `app_page`, whose messages
/// are the caller's own.
pub fn view_with<'a, M: 'a>(
    view: &SettingsView<'a>,
    wrap: impl Fn(Message) -> M + Copy + 'a,
    app_page: impl FnOnce(SettingsFeature) -> Option<Element<'a, M>>,
) -> Element<'a, M> {
    let palette = view.palette;
    let listed = listed_features(view);
    let shown = shown_feature(view);
    let body: Element<'a, M> = match shown {
        Some(feature) if feature.is_filled_by_app() => {
            let mut body = Column::new().push(heading(feature, view).map(wrap));
            if let Some(rows) = app_page(feature) {
                body = body.push(rows);
            }
            body.spacing(spacing::XS).into()
        }
        _ => page(shown, view).map(wrap),
    };
    let content = scrollable(
        container(body)
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
        Element::from(nav(&listed, shown, view)).map(wrap),
        theme::vertical_rule(palette.border_inactive),
        content,
    ]
    .height(Length::Fill)
    .into()
}

/// What the status bar shows while the tab is open.
#[derive(Debug)]
pub struct StatusBar<'a> {
    pub changed: usize,
    /// A failed write, which takes the place of the hint.
    pub failure: Option<&'a str>,
    pub notice: Option<&'a str>,
    pub lang: Language,
    pub palette: &'a Palette,
}

pub fn status_bar<'a>(bar: &StatusBar<'a>) -> Element<'a, Message> {
    let lang = bar.lang;
    let changed = (bar.changed > 0).then(|| {
        fill_template(
            lang.text(Msg::SettingsStatusChanged),
            &[("{count}", &bar.changed.to_string())],
        )
    });
    let line = match (bar.failure, bar.notice) {
        (Some(failure), _) => PageStatusLine::Failure(failure),
        (None, Some(notice)) => PageStatusLine::Notice(notice),
        (None, None) => PageStatusLine::Hint(lang.text(Msg::SettingsStatusHint)),
    };
    page_status_bar(
        PageStatusBar {
            mode: lang.text(Msg::SettingsMode),
            line,
            right: changed.as_deref(),
        },
        bar.palette,
    )
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests;
