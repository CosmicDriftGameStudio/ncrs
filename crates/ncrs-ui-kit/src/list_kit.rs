//! Controls for the settings pages that the plain kit has no equivalent of.

use iced::widget::text::Wrapping;
use iced::widget::{container, text, Row};
use iced::{Element, Length};

use crate::palette::Palette;
use crate::theme::{self, font_size, size};

/// Horizontal room inside a segment, either side of the label.
const SEGMENT_PADDING_X: f32 = 10.0;

/// A group of options of which one is selected; an option without a message
/// is not available and shows dimmed.
pub fn segmented_some_unavailable<'a, M: Clone + 'a>(
    options: &[(&str, Option<M>)],
    selected: usize,
    palette: &Palette,
) -> Element<'a, M> {
    let segments = options.iter().enumerate().map(|(index, (label, message))| {
        iced::widget::button(
            container(
                text((*label).to_owned())
                    .size(font_size::BODY)
                    .wrapping(Wrapping::None),
            )
            .center_y(Length::Fill),
        )
        .padding([0.0, SEGMENT_PADDING_X])
        .height(size::ROW)
        .on_press_maybe(message.clone())
        .style(theme::segment_button(palette, index == selected))
        .into()
    });
    container(Row::with_children(segments))
        .style(theme::segment_group(palette))
        .into()
}
