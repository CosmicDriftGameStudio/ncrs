//! The frame of a filing page: header, action and status bar.
use iced::widget::text::LineHeight;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, text, Column, Row, Space};
use iced::{alignment, Background, Border, Element, Length, Pixels, Theme};

use crate::kit;
use crate::palette::Palette;
use crate::theme;
use ncrs_i18n::{Language, Msg};

use super::*;

// ---------------------------------------------------------------------
// Page frame
// ---------------------------------------------------------------------

/// The page: the header, the content and the action bar, separated by lines.
pub fn tab_page<'a, M: Clone + 'a>(
    header: Element<'a, M>,
    content: Element<'a, M>,
    action_bar: Option<Element<'a, M>>,
    _palette: &Palette,
) -> Element<'a, M> {
    let mut main = Column::new()
        .push(header)
        .push(container(content).width(Length::Fill).height(Length::Fill));
    if let Some(bar) = action_bar {
        main = main.push(bar);
    }
    main.width(Length::Fill).height(Length::Fill).into()
}

pub(in crate::hermes_kit) fn row_button(
    palette: &Palette,
    selected: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: (selected || hovered).then_some(Background::Color(colors.title)),
            text_color: colors.text,
            border: Border::default(),
            ..button::Style::default()
        }
    }
}

/// The header of a page: title, an explanation of up to two lines and the
/// actions on the right.
pub fn page_header<'a, M: Clone + 'a>(
    title: &str,
    theme_name: Option<&str>,
    explain: Option<String>,
    title_extra: Option<Element<'a, M>>,
    actions: Vec<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut title_row = Row::new()
        .push(
            text(title.to_owned())
                .size(TEXT_TITLE)
                .font(semibold())
                .color(palette.text)
                .wrapping(Wrapping::None),
        )
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center);
    if let Some(name) = theme_name {
        title_row = title_row.push(small(name.to_owned(), palette.dim_text));
    }
    if let Some(extra) = title_extra {
        title_row = title_row.push(extra);
    }
    let mut texts = Column::new().push(title_row).spacing(GAP_XS);
    if let Some(sentence) = explain {
        texts = texts.push(
            container(
                text(sentence)
                    .size(TEXT_SMALL)
                    .line_height(LineHeight::Absolute(Pixels(SMALL_LINE)))
                    .color(palette.dim_text)
                    .wrapping(Wrapping::Word),
            )
            .max_width(EXPLAIN_WIDTH)
            .max_height(EXPLAIN_LINES * SMALL_LINE)
            .clip(true),
        );
    }
    let content = row![
        container(texts).width(Length::Fill),
        Row::with_children(actions)
            .spacing(GAP_S)
            .align_y(alignment::Vertical::Center),
    ]
    .spacing(GAP_M)
    .align_y(alignment::Vertical::Center);
    column![
        container(content)
            .padding([0.0, GAP_L])
            .width(Length::Fill)
            .center_y(Length::Fill),
        theme::horizontal_rule(palette.border_inactive),
    ]
    .height(HEADER_HEIGHT)
    .into()
}

/// The bar under the content: a note left, the buttons right.
pub fn action_bar<'a, M: Clone + 'a>(
    note: Option<String>,
    buttons: Vec<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let note_line: Element<'a, M> = match note {
        Some(sentence) => small(sentence, palette.dim_text),
        None => Space::new().into(),
    };
    action_bar_with(note_line, buttons, palette)
}

/// [`action_bar`] with anything at the left, like a sentence and a link.
pub fn action_bar_with<'a, M: Clone + 'a>(
    left: Element<'a, M>,
    buttons: Vec<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let content = row![
        container(left).width(Length::Fill).clip(true),
        Row::with_children(buttons)
            .spacing(GAP_S)
            .align_y(alignment::Vertical::Center),
    ]
    .spacing(GAP_M)
    .align_y(alignment::Vertical::Center);
    column![
        theme::horizontal_rule(palette.border_inactive),
        container(content)
            .padding([0.0, GAP_L])
            .width(Length::Fill)
            .center_y(Length::Fill),
    ]
    .height(ACTION_BAR_HEIGHT)
    .into()
}

/// The status bar of an open page: the badge, the keys and the Hermes state.
pub fn status_bar<'a, M: Clone + 'a>(
    badge: &str,
    hints: &[(&str, Msg)],
    right: String,
    lang: Language,
    palette: &Palette,
) -> Element<'a, M> {
    let (fill, ink) = palette.primary_button();
    let badge_cell = container(
        text(badge.to_uppercase())
            .size(TEXT_SMALL)
            .font(semibold())
            .color(ink)
            .wrapping(Wrapping::None),
    )
    .padding([0.0, GAP_S])
    .center_y(Length::Fill)
    .style(fill_style(fill));
    let mut line = Row::new()
        .push(badge_cell)
        .spacing(GAP_M)
        .align_y(alignment::Vertical::Center);
    for (key, verb) in hints {
        line = line.push(
            row![
                kit::key_cap(key, palette),
                small(lang.text(*verb), palette.dim_text)
            ]
            .spacing(GAP_XS)
            .align_y(alignment::Vertical::Center),
        );
    }
    container(
        row![
            line,
            Space::new().width(Length::Fill),
            container(small(right, palette.dim_text)).padding([0.0, GAP_M]),
        ]
        .align_y(alignment::Vertical::Center),
    )
    .width(Length::Fill)
    .height(STATUS_BAR_HEIGHT)
    .style(theme::statusbar(palette))
    .into()
}
