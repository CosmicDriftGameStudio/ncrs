//! A small outlined label in any color: a group of the Platz tab, a category.

use iced::widget::{button, container, text};
use iced::{Color, Element};

use crate::theme::git;
use crate::theme::{font_size, spacing};

/// The chip. A click sends `on_press`; without one it is a label.
pub fn color_chip<'a, M: Clone + 'a>(
    label: &str,
    ink: Color,
    on_press: Option<M>,
) -> Element<'a, M> {
    let content = text(label.to_owned())
        .size(font_size::SMALL)
        .wrapping(iced::widget::text::Wrapping::None);
    match on_press {
        Some(message) => button(content)
            .padding([0.0, spacing::XS])
            .style(git::state_chip(ink))
            .on_press(message)
            .into(),
        None => container(content)
            .padding([0.0, spacing::XS])
            .style(git::state_label(ink))
            .into(),
    }
}
