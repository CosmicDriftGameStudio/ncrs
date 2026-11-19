//! The button style of the rows of a filing list.
use iced::widget::button;
use iced::{Background, Theme};

use crate::palette::Palette;
use crate::theme;

/// A selected row looks as the cursor row of an active panel does; the
/// pointer over another row only tints it, so the two stay apart.
pub(in crate::hermes_kit) fn row_button(
    palette: &Palette,
    selected: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    let cursor_row = theme::row(palette, false, true, true);
    move |theme, status| {
        if selected {
            return cursor_row(theme, status);
        }
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then_some(Background::Color(colors.title)),
            text_color: colors.text,
            ..button::Style::default()
        }
    }
}
