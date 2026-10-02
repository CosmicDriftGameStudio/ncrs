//! The drive menu for Alt+F1 and Alt+F2: one row per place, the highlighted one
//! is where Enter goes.

use iced::widget::{button, column, container, text, Column, Row};
use iced::{Element, Length};

use super::layout::DIALOG_WIDTH;
use super::theme::{self, colors, font_size, spacing};
use crate::fs::Volume;
use crate::i18n::{Language, Msg};
use crate::messages::{Message, PanelSide};

/// Rows shown at once. A list longer than this scrolls with the highlight, so
/// the dialog stays inside the window with many mounts.
const VISIBLE_ROWS: usize = 12;

/// Index of the first row shown when `selected` is highlighted: the window only
/// moves once the highlight would leave it.
pub fn first_visible(selected: usize) -> usize {
    (selected + 1).saturating_sub(VISIBLE_ROWS)
}

pub fn view<'a>(
    side: PanelSide,
    volumes: &'a [Volume],
    selected: usize,
    lang: Language,
) -> Element<'a, Message> {
    let title = match side {
        PanelSide::Left => Msg::VolumesTitleLeft,
        PanelSide::Right => Msg::VolumesTitleRight,
    };
    let first = first_visible(selected);
    let rows = volumes
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_ROWS)
        .map(|(index, volume)| {
            let is_selected = index == selected;
            let name_color = if is_selected {
                colors::SELECTED_TEXT
            } else {
                colors::DIR_COLOR
            };
            let path_color = if is_selected {
                colors::SELECTED_TEXT
            } else {
                colors::DIM_TEXT
            };
            let content = Row::with_children([
                container(
                    text(volume.name.as_str())
                        .size(font_size::ROW)
                        .color(name_color)
                        .wrapping(text::Wrapping::None),
                )
                .width(Length::FillPortion(2))
                .clip(true)
                .into(),
                container(
                    text(volume.path.to_string_lossy())
                        .size(font_size::STATUS)
                        .color(path_color)
                        .wrapping(text::Wrapping::None),
                )
                .width(Length::FillPortion(3))
                .clip(true)
                .into(),
            ])
            .spacing(spacing::CELL_PADDING_X);
            button(content)
                .on_press(Message::VolumeMenuClick(index))
                .width(Length::Fill)
                .style(theme::row(is_selected, true))
                .into()
        });

    let body = column![
        text(lang.text(title))
            .size(font_size::TITLE)
            .color(colors::ACCENT),
        Column::with_children(rows)
    ]
    .spacing(spacing::SECTION_GAP);

    container(body)
        .padding(spacing::OUTER_PADDING * 2.0)
        .width(DIALOG_WIDTH)
        .style(theme::dialog)
        .into()
}
