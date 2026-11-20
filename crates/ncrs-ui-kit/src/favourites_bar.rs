//! The one-line bar under the path tabs with the favourites as slots. It looks
//! like the function key bar: a key cap with the number, then the folder's
//! name. The first nine have a number (their keys), the others only a name.
//!
//! The bar shows as many slots as fit and puts a button at the end that opens
//! the menu of the rest; the hint at the right end goes first.

use iced::widget::{button, column, container, mouse_area, row, text, Row, Space};
use iced::{alignment, Element, Length};

use super::fkeys::{numbered_cap, CAP_GAP, HINT_GAP, ROW_PADDING_X};
use super::theme::{self, font_size, size, spacing};
use crate::palette::Palette;

pub const FAVOURITES_BAR_HEIGHT: f32 = size::FAVOURITES_BAR;

/// How many favourites have a key and a number on their cap.
pub const FAVOURITE_KEYS: usize = 9;

/// What the overflow button takes, with its padding.
const OVERFLOW_WIDTH: f32 = 28.0;
/// A letter of the slot label is about this many times the font size wide;
/// a little too wide is better than a slot that is cut off.
const LETTER_WIDTH: f32 = 0.6;
const CAP_WIDTH: f32 = 20.0;

/// The width a slot takes in the bar, estimated: the text is laid out by the
/// renderer, the bar needs the number before that.
pub fn slot_width(label: &str, numbered: bool) -> f32 {
    let cap = if numbered { CAP_WIDTH + CAP_GAP } else { 0.0 };
    let letters: f32 = label.chars().map(|_| font_size::SMALL * LETTER_WIDTH).sum();
    2.0 * spacing::XS + cap + letters
}

/// How many of the slots with these `labels` fit in a bar of `width`: all of
/// them, or else as many as leave room for the overflow button.
pub fn slots_that_fit(labels: &[String], width: f32) -> usize {
    let available = width - 2.0 * ROW_PADDING_X;
    let widths: Vec<f32> = labels
        .iter()
        .enumerate()
        .map(|(index, label)| slot_width(label, index < FAVOURITE_KEYS))
        .collect();
    let gaps: f32 = labels.iter().skip(1).map(|_| HINT_GAP).sum();
    if widths.iter().sum::<f32>() + gaps <= available {
        return labels.len();
    }
    let room = available - OVERFLOW_WIDTH - HINT_GAP;
    let mut used = 0.0;
    let mut count = 0;
    for slot in widths {
        let next = used + slot + if count > 0 { HINT_GAP } else { 0.0 };
        if next > room {
            break;
        }
        used = next;
        count += 1;
    }
    count
}

/// The bottom-left corner of the overflow button in the window, for a bar that
/// shows the first `fitting` of the slots with these `labels`, estimated like
/// `slots_that_fit`. The bar sits right under the title bar.
pub fn overflow_anchor(labels: &[String], fitting: usize) -> iced::Point {
    let slots: f32 = labels
        .iter()
        .take(fitting)
        .enumerate()
        .map(|(index, label)| slot_width(label, index < FAVOURITE_KEYS) + HINT_GAP)
        .sum();
    iced::Point::new(
        ROW_PADDING_X + slots,
        size::TITLE_BAR + FAVOURITES_BAR_HEIGHT,
    )
}

#[derive(Debug)]
pub struct Slot<M> {
    /// The number on the cap, from 1; none for a favourite without a key.
    pub number: Option<usize>,
    pub label: String,
    /// The folder of the active panel.
    pub is_current: bool,
    /// Shown while the pointer rests on the slot.
    pub tooltip: String,
    pub on_press: M,
    pub on_right_press: M,
}

/// The button at the end of the bar for the favourites that did not fit.
#[derive(Debug)]
pub struct Overflow<M> {
    pub tooltip: String,
    pub on_press: M,
}

/// The bar. `hint` is the dim text at the right end, `None` shows none.
pub fn view<'a, M: Clone + 'a>(
    slots: Vec<Slot<M>>,
    overflow: Option<Overflow<M>>,
    hint: Option<String>,
    palette: &Palette,
) -> Element<'a, M> {
    let slot_views = slots.into_iter().map(|slot| {
        let label = text(slot.label)
            .size(font_size::SMALL)
            .wrapping(text::Wrapping::None);
        let content = match slot.number {
            Some(number) => row![
                numbered_cap(number.to_string(), slot.is_current, false, palette),
                label
            ],
            None => row![label],
        }
        .spacing(CAP_GAP)
        .align_y(alignment::Vertical::Center);
        let pressable = button(content)
            .on_press(slot.on_press)
            .padding([0.0, spacing::XS])
            .style(theme::favourite_slot(palette, slot.is_current));
        let with_menu = mouse_area(pressable).on_right_press(slot.on_right_press);
        let slot_view: Element<'a, M> = crate::kit::tooltip(
            with_menu,
            slot.tooltip,
            iced::widget::tooltip::Position::Bottom,
            palette,
        );
        slot_view
    });
    let mut line = Row::new()
        .push(Row::with_children(slot_views).spacing(HINT_GAP))
        .align_y(alignment::Vertical::Center);
    if let Some(rest) = overflow {
        let more = button(text("\u{bb}").size(font_size::SMALL))
            .on_press(rest.on_press)
            .padding([0.0, spacing::XS])
            .style(theme::favourite_slot(palette, false));
        line = line
            .push(Space::new().width(HINT_GAP))
            .push(crate::kit::tooltip(
                more,
                rest.tooltip,
                iced::widget::tooltip::Position::Bottom,
                palette,
            ));
    }
    line = line.push(Space::new().width(Length::Fill));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn names(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("folder{index}")).collect()
    }

    #[test]
    fn everything_shows_when_it_fits() {
        assert_eq!(slots_that_fit(&names(3), 1200.0), 3);
        assert_eq!(slots_that_fit(&[], 100.0), 0);
    }

    #[test]
    fn what_does_not_fit_leaves_room_for_the_overflow_button() {
        let labels = names(20);
        let shown = slots_that_fit(&labels, 600.0);
        assert!(shown > 0 && shown < labels.len());
        let used: f32 = labels
            .iter()
            .take(shown)
            .enumerate()
            .map(|(index, label)| slot_width(label, index < FAVOURITE_KEYS) + HINT_GAP)
            .sum();
        assert!(used + OVERFLOW_WIDTH <= 600.0 - 2.0 * ROW_PADDING_X);
    }

    #[test]
    fn a_narrower_window_shows_fewer() {
        let labels = names(20);
        assert!(slots_that_fit(&labels, 400.0) < slots_that_fit(&labels, 800.0));
    }

    #[test]
    fn a_slot_without_a_key_is_narrower_than_one_with() {
        assert!(slot_width("docs", false) < slot_width("docs", true));
    }
}
