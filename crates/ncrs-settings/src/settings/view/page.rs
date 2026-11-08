use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, text, Column};
use iced::{alignment, Element, Length, Padding};

use super::*;
use crate::message::SettingsMessage;
use crate::settings::*;
use ncrs_config::config::ConfigError;
use ncrs_config::SettingsFeature;
use ncrs_i18n::fill_template;
use ncrs_i18n::Msg;
use ncrs_ui_kit::kit;
use ncrs_ui_kit::layout::SETTINGS_NAV_WIDTH;
use ncrs_ui_kit::nav_list::{nav_list, NavGroup, NavItem};
use ncrs_ui_kit::theme::{self, font_size, spacing};

/// In the list the Hermes page shows only its second name: the group head
/// above it already says Hermes.
fn nav_label(feature: SettingsFeature) -> Msg {
    match feature {
        SettingsFeature::Hermes => Msg::SettingsFeatureHermesSorting,
        other => feature_label(other),
    }
}

fn group_mark(group: SettingsGroup) -> Option<Msg> {
    match group {
        SettingsGroup::Hermes => Some(Msg::HermesBeta),
        SettingsGroup::General | SettingsGroup::Files | SettingsGroup::Documents => None,
    }
}

pub fn nav<'a>(
    listed: &[SettingsFeature],
    shown: Option<SettingsFeature>,
    view: &SettingsView<'a>,
) -> Element<'a, Message> {
    let palette = view.palette;
    let lang = view.lang;
    let message = |query: String| SettingsMessage::SearchChanged(query);
    let search = kit::text_field(
        lang.text(Msg::SettingsSearchPlaceholder),
        &view.state.query,
        SEARCH_FIELD_ID,
        Some(message),
        message(view.state.query.clone()),
        palette,
    );
    let mut groups: Vec<NavGroup<SettingsMessage>> = Vec::new();
    let mut last_group = None;
    for &feature in listed {
        let group = feature.group();
        let item = NavItem {
            label: lang.text(nav_label(feature)).to_owned(),
            suffix: None,
            chosen: shown == Some(feature),
            dimmed: view.state.focus.area == crate::focus::Area::Rows,
            on_press: SettingsMessage::Select(feature),
        };
        match groups.last_mut() {
            Some(last) if last_group == Some(group) => last.items.push(item),
            _ => {
                groups.push(NavGroup {
                    head: lang.text(group_label(group)).to_owned(),
                    mark: group_mark(group).map(|mark| lang.text(mark).to_owned()),
                    items: vec![item],
                });
                last_group = Some(group);
            }
        }
    }
    let entries = nav_list(groups, palette);
    let from_config_file = shown.and_then(feature_config_place).is_some();
    let footer: Element<'a, Message> = if from_config_file {
        text(lang.text(Msg::SettingsNavFooter))
            .size(font_size::SMALL)
            .color(palette.dim_text)
            .wrapping(Wrapping::Word)
            .into()
    } else {
        container("").into()
    };
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

pub fn heading<'a>(feature: SettingsFeature, view: &SettingsView<'a>) -> Element<'a, Message> {
    let palette = view.palette;
    let lang = view.lang;
    let mut title = row![text(lang.text(feature_label(feature)).to_owned())
        .size(font_size::FIGURE)
        .font(ncrs_ui_kit::APP_FONT_BOLD)]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    if let Some(suffix) = feature_suffix(feature) {
        title = title.push(
            text(lang.text(suffix).to_owned())
                .size(font_size::TITLE)
                .color(palette.dim_text),
        );
    }
    if let Some(beta_mark) = feature_beta_mark(feature) {
        title = title.push(
            text(lang.text(beta_mark).to_owned())
                .size(font_size::TITLE)
                .color(palette.dim_text),
        );
    }
    title = title.push(container("").width(Length::Fill));
    if let Some(place) = feature_config_place(feature) {
        title = title.push(mono(place.to_owned(), palette.dim_text));
    }
    column![
        title,
        kit::dim(lang.text(feature_intro(feature)).to_owned(), palette)
    ]
    .spacing(spacing::XS)
    .into()
}

pub fn page<'a>(shown: Option<SettingsFeature>, view: &SettingsView<'a>) -> Element<'a, Message> {
    let palette = view.palette;
    let lang = view.lang;
    let Some(feature) = shown else {
        return kit::dim(lang.text(Msg::SettingsNothingFound).to_owned(), palette);
    };
    let query = view.state.query.trim().to_lowercase();
    let whole_feature = query.is_empty() || feature_name_matches(feature, &query, lang);
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
    let focus = view.state.focus;
    let sections = ncrs_ui_kit::setting_rows::sections_with_focus(
        groups_of(feature, view),
        |setting| whole_feature || row_matches(setting, &query),
        view.state.marked_row.as_deref(),
        focus.row_focus(),
        |row, inner| Some(SettingsMessage::FocusThen(row, inner.map(Box::new))),
        palette,
    );
    sections
        .into_iter()
        .fold(body, |column, section| column.push(section))
        .spacing(spacing::XS)
        .into()
}

/// The pages in the list, the one shown and its rows that have a key: what the
/// keyboard moves through, from the same groups and filter the page draws.
pub fn focus_facts(
    view: &SettingsView<'_>,
) -> Option<(
    Vec<SettingsFeature>,
    SettingsFeature,
    Vec<ncrs_ui_kit::setting_rows::RowKey<Message>>,
)> {
    let shown = shown_feature(view)?;
    let query = view.state.query.trim().to_lowercase();
    let whole_feature = query.is_empty() || feature_name_matches(shown, &query, view.lang);
    let rows = ncrs_ui_kit::setting_rows::keyed_rows(groups_of(shown, view), |setting| {
        whole_feature || row_matches(setting, &query)
    });
    Some((listed_features(view), shown, rows))
}

pub fn config_broken_text(broken: &ConfigError, view: &SettingsView<'_>) -> String {
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
