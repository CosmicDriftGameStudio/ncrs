//! The one-line bar under the path tabs with the first favourites as numbered
//! slots. It looks like the function key bar: a key cap with the number, then
//! the folder's name.
//!
//! The slots are clipped, not wrapped, when the window is narrow; the hint at
//! the right end goes first.

use iced::widget::{button, column, container, mouse_area, row, text, tooltip, Row, Space};
use iced::{alignment, Element, Length};

use super::fkeys::{numbered_cap, CAP_GAP, HINT_GAP, ROW_PADDING_X};
use super::theme::{self, font_size, size, spacing};
use crate::palette::Palette;

pub const FAVOURITES_BAR_HEIGHT: f32 = size::FAVOURITES_BAR;

/// How many favourites get a slot, and a number.
pub const FAVOURITES_BAR_SLOTS: usize = 5;

#[derive(Debug)]
pub struct Slot<M> {
    /// The number on the cap, from 1.
    pub number: usize,
    pub label: String,
    /// The folder of the active panel.
    pub is_current: bool,
    /// Shown while the pointer rests on the slot.
    pub tooltip: String,
    pub on_press: M,
    pub on_right_press: M,
}

/// The bar. `hint` is the dim text at the right end, `None` shows none.
pub fn view<'a, M: Clone + 'a>(
    slots: Vec<Slot<M>>,
    hint: Option<String>,
    palette: &Palette,
) -> Element<'a, M> {
    let slot_views = slots.into_iter().map(|slot| {
        let content = row![
            numbered_cap(slot.number.to_string(), slot.is_current, false, palette),
            text(slot.label)
                .size(font_size::SMALL)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(CAP_GAP)
        .align_y(alignment::Vertical::Center);
        let pressable = button(content)
            .on_press(slot.on_press)
            .padding([0.0, spacing::XS])
            .style(theme::favourite_slot(palette, slot.is_current));
        let with_menu = mouse_area(pressable).on_right_press(slot.on_right_press);
        let hover = container(text(slot.tooltip).size(font_size::SMALL))
            .padding([spacing::XS, spacing::M])
            .style(theme::menu(palette));
        let slot_view: Element<'a, M> = tooltip(with_menu, hover, tooltip::Position::Bottom).into();
        slot_view
    });
    let mut line = Row::new()
        .push(Row::with_children(slot_views).spacing(HINT_GAP))
        .push(Space::new().width(Length::Fill))
        .align_y(alignment::Vertical::Center);
    if let Some(shown) = hint {
        line = line.push(
            text(shown)
                .size(font_size::SMALL)
                .color(palette.dim_text)
                .wrapping(text::Wrapping::None),
        );
    }
    container(column![
        theme::horizontal_rule(palette.border_inactive),
        container(line)
            .padding([0.0, ROW_PADDING_X])
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .align_y(alignment::Vertical::Center),
        theme::horizontal_rule(palette.border_inactive),
    ])
    .height(FAVOURITES_BAR_HEIGHT)
    .width(Length::Fill)
    .style(theme::statusbar(palette))
    .into()
}

/// The number cap of a favourite, as the settings list draws it.
pub fn number_cap<'a, M: 'a>(number: usize, palette: &Palette) -> Element<'a, M> {
    numbered_cap(number.to_string(), false, false, palette)
}

/// Blank space as wide as [`number_cap`]'s, for the rows without a slot.
pub fn number_cap_gap<'a, M: 'a>() -> Element<'a, M> {
    iced::widget::container(
        text("0")
            .size(font_size::KEY_CAP)
            .wrapping(text::Wrapping::None)
            .color(iced::Color::TRANSPARENT),
    )
    .padding([0.0, spacing::XS - 1.0])
    .style(move |_: &iced::Theme| iced::widget::container::Style {
        border: iced::Border {
            width: spacing::BORDER_WIDTH,
            ..iced::Border::default()
        },
        ..iced::widget::container::Style::default()
    })
    .into()
}
