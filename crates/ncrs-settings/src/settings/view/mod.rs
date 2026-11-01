mod groups;
mod page;

use groups::*;
use page::*;

use crate::settings::*;

use std::path::Path;

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, scrollable, text};
use iced::{alignment, Color, Element, Font, Length, Padding};

use crate::message::{SettingChange, SettingField, SettingsMessage};
use ncrs_config::config::{AiEndpointProblem, Filing, ProgramLineProblem, ServerEntry};
use ncrs_config::SettingsFeature;
use ncrs_i18n::fill_template;
use ncrs_i18n::{FuturamaName, Language, Msg};
use ncrs_ui_kit::format::shown_path;
use ncrs_ui_kit::frame::{SCROLLBAR_MARGIN, SCROLLBAR_WIDTH};
use ncrs_ui_kit::kit::{self, Check, Tone};
use ncrs_ui_kit::layout::SETTINGS_PAGE_MAX_WIDTH;
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

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
    pub home: &'a Path,
    /// How many actions have keys other than their built-in ones.
    pub keys_changed: usize,
    pub lang: Language,
    pub palette: &'a Palette,
}

struct SettingRow<'a> {
    label: String,
    /// The Futurama name, from a `Msg`; the only name the command list takes.
    theme_name: Option<String>,
    /// The key in the config or theme file, shown where the theme name is but
    /// never offered to the command list.
    config_key: Option<String>,
    description: Option<String>,
    /// What is wrong with the value, under the description.
    problem: Option<String>,
    control: Element<'a, Message>,
    /// Rows that show a state or a user's own data are not commands.
    in_command_list: bool,
    control_width: Option<f32>,
}

struct Group<'a> {
    /// A sentence under the title that says what the group is for.
    intro: Option<String>,
    /// Folded away behind its title until the user opens it or a search finds
    /// a row in it.
    advanced: bool,
    title: String,
    rows: Vec<SettingRow<'a>>,
}

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
        FieldProblem::Threads => Msg::SettingsProblemThreads,
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
        SettingsFeature::Hermes => Msg::SettingsFeatureHermes,
    }
}

fn feature_theme_name(feature: SettingsFeature) -> Option<FuturamaName> {
    match feature {
        SettingsFeature::Hermes => Some(FuturamaName::GroupHermes),
        SettingsFeature::Look
        | SettingsFeature::Language
        | SettingsFeature::Programs
        | SettingsFeature::Keys
        | SettingsFeature::Servers => None,
    }
}

fn feature_beta_mark(feature: SettingsFeature) -> Option<Msg> {
    match feature {
        SettingsFeature::Hermes => Some(Msg::HermesBeta),
        SettingsFeature::Look
        | SettingsFeature::Language
        | SettingsFeature::Programs
        | SettingsFeature::Keys
        | SettingsFeature::Servers => None,
    }
}

fn feature_intro(feature: SettingsFeature) -> Msg {
    match feature {
        SettingsFeature::Look => Msg::SettingsFeatureLookIntro,
        SettingsFeature::Language => Msg::SettingsFeatureLanguageIntro,
        SettingsFeature::Programs => Msg::SettingsFeatureProgramsIntro,
        SettingsFeature::Keys => Msg::SettingsFeatureKeysIntro,
        SettingsFeature::Servers => Msg::SettingsFeatureServersIntro,
        SettingsFeature::Hermes => Msg::SettingsFeatureHermesIntro,
    }
}

fn feature_config_place(feature: SettingsFeature) -> &'static str {
    match feature {
        SettingsFeature::Look => THEME_SECTION,
        SettingsFeature::Language => LANGUAGE_KEY,
        SettingsFeature::Programs => OPEN_SECTION,
        SettingsFeature::Keys => KEYS_SECTION,
        SettingsFeature::Servers => SERVERS_SECTION,
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
        config_key: None,
        description: note.map(|msg| lang.text(msg).to_owned()),
        problem: None,
        control,
        control_width: None,
        in_command_list: true,
    }
}

fn groups_of<'a>(feature: SettingsFeature, view: &SettingsView<'a>) -> Vec<Group<'a>> {
    match feature {
        SettingsFeature::Look => look_groups(view),
        SettingsFeature::Language => language_groups(view),
        SettingsFeature::Programs => programs_groups(view),
        SettingsFeature::Keys => keys_groups(view),
        SettingsFeature::Servers => servers_groups(view),
        SettingsFeature::Hermes => hermes_groups(view),
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
    for feature in SettingsFeature::ALL {
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

/// The features the search leaves, in their order.
fn listed_features(view: &SettingsView<'_>) -> Vec<SettingsFeature> {
    let query = view.state.query.trim().to_lowercase();
    SettingsFeature::ALL
        .into_iter()
        .filter(|&feature| feature_matches(feature, &query, view))
        .collect()
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
    .height(size::STATUS_BAR)
    .width(Length::Fill)
    .style(theme::statusbar(palette))
    .into()
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests;
