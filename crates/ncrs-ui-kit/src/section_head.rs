//! The dark bar that heads a group of rows and folds it open or shut.

use iced::mouse::Interaction;
use iced::widget::text::Wrapping;
use iced::widget::{column, container, mouse_area, row, text, Space};
use iced::{alignment, Element, Length};

use crate::palette::Palette;
use crate::theme::{self, font_size, git, size, spacing};
use crate::APP_FONT_BOLD;

#[derive(Debug, Clone)]
pub struct SectionHead<'a, M> {
    pub title: &'a str,
    /// The rows in the group. A group without rows is shut and dimmed, and
    /// there is nothing to fold.
    pub count: usize,
    pub open: bool,
    /// The keys are on the head.
    pub has_focus: bool,
    /// A click, and Enter while the head has focus, send this.
    pub on_toggle: M,
}

pub fn section_head<'a, M: Clone + 'a>(
    head: SectionHead<'_, M>,
    palette: &Palette,
) -> Element<'a, M> {
    section_head_with(head, Vec::new(), palette)
}

/// A head with buttons at its right end. The count then stands after the
/// title, and the buttons keep their own clicks.
pub fn section_head_with<'a, M: Clone + 'a>(
    head: SectionHead<'_, M>,
    actions: Vec<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let colors = *palette;
    let has_focus = head.has_focus;
    let empty = head.count == 0;
    let shown_open = head.open && !empty;
    let arrow = if shown_open { "\u{25be}" } else { "\u{25b8}" };
    let ink = if empty {
        palette.dim_text
    } else {
        palette.text
    };
    let title = text(head.title.to_uppercase())
        .size(font_size::SMALL)
        .font(APP_FONT_BOLD)
        .wrapping(Wrapping::None);
    let count = text(head.count.to_string())
        .size(font_size::SMALL)
        .color(palette.dim_text)
        .wrapping(Wrapping::None);
    let mut line = row![
        text(arrow).size(font_size::SMALL).wrapping(Wrapping::None),
        title,
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    if actions.is_empty() {
        line = line.push(Space::new().width(Length::Fill)).push(count);
    } else {
        line = line.push(count).push(Space::new().width(Length::Fill));
        line = line.push(
            iced::widget::Row::with_children(actions)
                .spacing(spacing::XS)
                .align_y(alignment::Vertical::Center),
        );
    }
    let bar = container(line)
        .padding([0.0, spacing::M])
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(alignment::Vertical::Center)
        .style(move |theme: &iced::Theme| {
            let mut style = if has_focus {
                git::section_head_focused(&colors)(theme)
            } else {
                git::section_head(&colors, empty)(theme)
            };
            style.text_color = Some(ink);
            style
        });
    let framed = column![
        theme::horizontal_rule(palette.border_inactive),
        bar,
        theme::horizontal_rule(palette.border_inactive),
    ];
    let area = mouse_area(
        container(framed)
            .height(size::SECTION_HEAD)
            .width(Length::Fill),
    )
    .interaction(Interaction::Pointer);
    if empty {
        area.into()
    } else {
        area.on_press(head.on_toggle).into()
    }
}
