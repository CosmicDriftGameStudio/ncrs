//! One picture per Git building block with all its states, in Dracula. A
//! block that changes its look changes its picture, and the diff is the
//! review. The pixels are compared on macOS only.

mod lists;
mod panes;
mod repos;
mod space;

use crate::snapshot_support::ReferenceSnapshot as _;
use crate::theme;
use iced::widget::container;
use iced::{Element, Size};
use iced_test::Simulator;

use crate::palette::ThemePreset;

/// Renders `content` on the panel ground in `size` and compares it with
/// `tests/snapshots/git-kit-{name}-dracula-tiny-skia.png`.
pub fn matches_picture(name: &str, size: Size, content: Element<'_, ()>) {
    let palette = ThemePreset::Dracula.palette();
    let file = format!("git-kit-{name}-dracula");
    let matches = Simulator::with_size(
        iced::Settings {
            default_font: crate::APP_FONT,
            ..iced::Settings::default()
        },
        size,
        container(content)
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .style(theme::panel(&palette)),
    )
    .snapshot(&iced::Theme::Dark)
    .unwrap()
    .matches_reference_image(format!("tests/snapshots/{file}.png"))
    .unwrap();
    assert!(matches, "{file} differs from its image");
}
