//! The styles of buttons that read as links.

use iced::widget::button;
use iced::{Border, Theme};

use super::hover_fill;
use crate::palette::Palette;

/// A flat button in the text color, for an action inside the content.
pub fn link_button(palette: &Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then(|| hover_fill(&colors)),
            text_color: colors.text,
            border: Border::default(),
            ..button::Style::default()
        }
    }
}

/// A flat button in the accent, for a name that opens what it names: it reads
/// as a link and not as plain text.
pub fn named_link_button(palette: &Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then(|| hover_fill(&colors)),
            text_color: colors.accent,
            border: Border::default(),
            ..button::Style::default()
        }
    }
}
