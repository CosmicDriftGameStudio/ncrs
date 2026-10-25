//! The drive menu for Alt+F1 and Alt+F2: one row per place, the highlighted one
//! is where Enter goes.

use iced::widget::container;
use iced::widget::{button, column, text, Column, Row};
use iced::{Element, Length, Size};

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::theme::{self, font_size, spacing};
use crate::fs::Volume;
use crate::i18n::{Language, Msg};
use crate::messages::{Message, PanelSide};
use crate::palette::Palette;

/// Medium rather than large: the list is short, and a large frame would be
/// mostly empty.
const SIZE: DialogSize = DialogSize::Medium;

/// Rows shown at once. A list longer than this scrolls with the highlight, so
/// the dialog stays inside the window with many mounts.
const VISIBLE_ROWS: usize = 12;

/// The name column takes two parts of five, the path three.
const NAME_SHARE: f32 = 0.4;
const PATH_SHARE: f32 = 0.6;

/// Index of the first row shown when `selected` is highlighted: the window only
/// moves once the highlight would leave it.
pub fn first_visible(selected: usize) -> usize {
    (selected + 1).saturating_sub(VISIBLE_ROWS)
}

pub fn view<'a>(
    side: PanelSide,
    volumes: &'a [Volume],
    selected: usize,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let subtitle = match side {
        PanelSide::Left => Msg::VolumesSubtitleLeft,
        PanelSide::Right => Msg::VolumesSubtitleRight,
    };
    let first = first_visible(selected);
    let row_width = frame::content_width(SIZE, window) - 2.0 * spacing::CELL_PADDING_X;
    let name_chars = frame::chars_that_fit(row_width * NAME_SHARE);
    let path_chars = frame::chars_that_fit(row_width * PATH_SHARE);
    let rows = volumes
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_ROWS)
        .map(|(index, volume)| {
            let is_selected = index == selected;
            let name_color = if is_selected {
                palette.cursor_text
            } else {
                palette.directory
            };
            let path_color = if is_selected {
                palette.cursor_text
            } else {
                palette.dim_text
            };
            let content = Row::with_children([
                container(
                    text(format::ellipsize_end(volume.name.as_str(), name_chars))
                        .size(font_size::ROW)
                        .color(name_color)
                        .wrapping(text::Wrapping::None),
                )
                .width(Length::FillPortion(2))
                .clip(true)
                .into(),
                container(
                    text(format::ellipsize_middle(
                        &volume.path.to_string_lossy(),
                        path_chars,
                    ))
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
                .style(theme::row(palette, false, is_selected, true))
                .into()
        });

    Frame::new(
        lang.text(Msg::VolumesTitle),
        column![Column::with_children(rows)],
        SIZE,
        window,
        lang,
        palette,
    )
    .subtitle(lang.text(subtitle))
    .on_close(Message::VolumeMenuClose)
    // Enter goes to the highlighted row, which the content defines.
    .hints(KeyHints::default())
    .into_element()
}
