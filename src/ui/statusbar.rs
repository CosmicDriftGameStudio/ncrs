//! Bottom bar: active path, selected entry info, or the last error.

use iced::widget::{container, row, text};
use iced::{alignment, Element, Length};

use super::format;
use super::layout::{DATE_COLUMN_WIDTH, SIZE_COLUMN_WIDTH, STATUSBAR_HEIGHT};
use super::panel::PanelState;
use super::theme::{self, colors, font_size, spacing};

pub fn view<'a, M: 'a>(panel: &'a PanelState) -> Element<'a, M> {
    let path = text(panel.path.display().to_string())
        .size(font_size::STATUS)
        .color(colors::ACCENT)
        .wrapping(text::Wrapping::None);

    let details: Element<'a, M> = if let Some(error) = &panel.error {
        text(error.as_str())
            .size(font_size::STATUS)
            .color(colors::ERROR)
            .wrapping(text::Wrapping::None)
            .into()
    } else if let Some(entry) = panel.selected_entry() {
        row![
            container(
                text(entry.name.as_str())
                    .size(font_size::STATUS)
                    .wrapping(text::Wrapping::None)
            )
            .width(Length::Fill)
            .clip(true),
            text(format::entry_size(entry))
                .size(font_size::STATUS)
                .width(SIZE_COLUMN_WIDTH)
                .align_x(alignment::Horizontal::Right),
            text(format::time(entry.modified))
                .size(font_size::STATUS)
                .width(DATE_COLUMN_WIDTH)
                .align_x(alignment::Horizontal::Right),
        ]
        .into()
    } else {
        text(if panel.loading { "Loading…" } else { "" })
            .size(font_size::STATUS)
            .color(colors::DIM_TEXT)
            .into()
    };

    let separator = text("│").size(font_size::STATUS).color(colors::DIM_TEXT);

    container(
        row![
            container(path).width(Length::FillPortion(2)).clip(true),
            separator,
            container(details).width(Length::FillPortion(3)).clip(true),
        ]
        .spacing(spacing::CELL_PADDING_X)
        .align_y(alignment::Vertical::Center),
    )
    .padding([0.0, spacing::CELL_PADDING_X * 1.5])
    .height(STATUSBAR_HEIGHT)
    .width(Length::Fill)
    .align_y(alignment::Vertical::Center)
    .style(theme::statusbar)
    .into()
}
