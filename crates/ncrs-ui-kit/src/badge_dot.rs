//! A small filled dot that marks something as new, beside a label.

use iced::widget::{container, Space};
use iced::{Border, Color, Element, Length};

/// A filled circle of `diameter` pixels.
pub fn badge_dot<'a, M: 'a>(color: Color, diameter: f32) -> Element<'a, M> {
    container(Space::new())
        .width(Length::Fixed(diameter))
        .height(Length::Fixed(diameter))
        .style(move |_theme| container::Style {
            background: Some(color.into()),
            border: Border {
                radius: (diameter / 2.0).into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}
