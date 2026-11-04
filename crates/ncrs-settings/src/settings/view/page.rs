use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, text, Column};
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
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

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
    let mut entries = Column::new().spacing(NAV_ENTRY_GAP);
    let mut current_group = None;
    for &feature in listed {
        if current_group != Some(feature.group()) {
            current_group = Some(feature.group());
            entries = entries.push(
                container(
                    text(lang.text(group_label(feature.group())).to_owned())
                        .size(font_size::SMALL)
                        .color(palette.dim_text)
                        .wrapping(Wrapping::None),
                )
                .padding(Padding {
                    top: spacing::S,
                    left: spacing::CELL_PADDING_X,
                    ..Padding::ZERO
                }),
            );
        }
        let mut label = row![text(lang.text(feature_label(feature)).to_owned())
            .size(font_size::BODY)
            .wrapping(Wrapping::None)]
        .spacing(spacing::S)
        .align_y(alignment::Vertical::Center);
        if let Some(suffix) = feature_suffix(feature) {
            label = label.push(
                text(lang.text(suffix).to_owned())
                    .size(font_size::SMALL)
                    .color(palette.dim_text)
                    .wrapping(Wrapping::None),
            );
        }
        if let Some(beta_mark) = feature_beta_mark(feature) {
            label = label.push(
                text(lang.text(beta_mark).to_owned())
                    .size(font_size::SMALL)
                    .color(palette.dim_text)
                    .wrapping(Wrapping::None),
            );
        }
        entries = entries.push(
            button(container(label).center_y(Length::Fill))
                .on_press(SettingsMessage::Select(feature))
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

pub fn heading<'a>(feature: SettingsFeature, view: &SettingsView<'a>) -> Element<'a, Message> {
    let palette = view.palette;
    let lang = view.lang;
    let mut title = row![text(lang.text(feature_label(feature)).to_owned())
        .size(font_size::FIGURE)
        .font(ncrs_ui_kit::APP_FONT)]
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
    let mut first = true;
    for group in groups_of(feature, view) {
        let rows: Vec<Element<'a, Message>> = group
            .rows
            .into_iter()
            .filter(|setting| whole_feature || row_matches(setting, &query))
            .map(|setting| {
                let marked = view.state.marked_row.as_deref() == Some(setting.label.as_str());
                let lines_below: Vec<Element<'a, Message>> = setting
                    .problem
                    .iter()
                    .map(|wrong| kit::error_line(wrong.clone(), palette))
                    .chain(
                        setting
                            .warning
                            .iter()
                            .map(|caution| kit::warning_line(caution.clone(), palette)),
                    )
                    .collect();
                let form_row = kit::form_row_with_problem(
                    &setting.label,
                    setting
                        .theme_name
                        .as_deref()
                        .or(setting.config_key.as_deref()),
                    setting.description.as_deref(),
                    lines_below,
                    setting.control,
                    setting.control_width.unwrap_or(size::CONTROL_WIDTH),
                    palette,
                );
                if marked {
                    // The outline is drawn over the content, so the row keeps
                    // its text and controls clear of it.
                    container(form_row)
                        .padding([0.0, spacing::XS])
                        .style(theme::marked_setting(palette))
                        .into()
                } else {
                    form_row
                }
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        let mut content = Column::with_children(rows);
        if let Some(intro) = group.intro {
            content = Column::new()
                .push(kit::dim(intro, palette))
                .push(content)
                .spacing(spacing::XS);
        }
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
