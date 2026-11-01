//! The scrim behind the dialogs; the prompt itself is in `ncrs-ops`.

use iced::widget::container;
use iced::{alignment, Background, Element, Length};

use super::theme;
use crate::messages::Message;

pub use ncrs_ops::prompt_view::view;

/// A scrim behind the dialog, so clicks outside it do not reach the panels.
pub fn scrim<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .align_y(alignment::Vertical::Center)
        .style(|_theme| container::Style {
            background: Some(Background::Color(theme::SCRIM)),
            ..container::Style::default()
        })
        .into()
}
