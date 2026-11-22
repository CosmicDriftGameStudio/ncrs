//! Rows and bars that carry buttons of their own: a file with its action, a
//! titled list with an action for all of it, a notice banner, a hunk bar.

use iced::widget::text::Wrapping;
use iced::widget::{button, container, mouse_area, row, text, Row, Space};
use iced::{alignment, Color, Element, Length, Padding};

use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing};

/// A list entry that is pressed to choose it and has buttons beside it, like a
/// file that can be staged or a conflict that can be resolved.
pub struct ActionRow<'a, M> {
    /// A short column before the label, such as the status letter.
    pub lead: Option<(String, Color)>,
    pub label: String,
    /// Small texts after the label, such as line counts.
    pub trail: Vec<(String, Color)>,
    pub actions: Vec<Element<'a, M>>,
    pub chosen: bool,
    pub on_press: M,
}

impl<M> std::fmt::Debug for ActionRow<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionRow")
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

/// Room of the lead letter.
const LEAD_WIDTH: f32 = 14.0;

pub fn action_row<'a, M: Clone + 'a>(entry: ActionRow<'a, M>, palette: &Palette) -> Element<'a, M> {
    let ink = if entry.chosen {
        palette.cursor_text
    } else {
        palette.text
    };
    let mut line = Row::new()
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    if let Some((letter, color)) = entry.lead {
        line = line.push(
            container(text(letter).size(font_size::BODY).color(if entry.chosen {
                palette.cursor_text
            } else {
                color
            }))
            .width(LEAD_WIDTH),
        );
    }
    line = line.push(
        container(
            text(entry.label)
                .size(font_size::BODY)
                .color(ink)
                .wrapping(Wrapping::None),
        )
        .width(Length::Fill)
        .clip(true),
    );
    for (content, color) in entry.trail {
        line = line.push(
            text(content)
                .size(font_size::SMALL)
                .color(if entry.chosen {
                    palette.cursor_text
                } else {
                    color
                })
                .wrapping(Wrapping::None),
        );
    }
    let pressable = button(
        container(line)
            .padding([0.0, spacing::CELL_PADDING_X])
            .center_y(Length::Fill),
    )
    .on_press(entry.on_press)
    .width(Length::Fill)
    .height(size::ROW_TALL)
    .padding(0.0)
    .style(theme::menu_item(palette, entry.chosen));
    let mut whole = Row::new()
        .push(pressable)
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    for action in entry.actions {
        whole = whole.push(action);
    }
    container(whole)
        .padding(Padding {
            right: spacing::M,
            ..Padding::ZERO
        })
        .into()
}

/// The title of a list with how many it holds and what can be done with all.
pub fn section_bar<'a, M: 'a>(
    title: String,
    count: usize,
    actions: Vec<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut line = row![
        text(title)
            .size(font_size::BODY)
            .color(palette.text)
            .wrapping(Wrapping::None),
        text(format!("({count})"))
            .size(font_size::BODY)
            .color(palette.dim_text),
        Space::new().width(Length::Fill),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    for action in actions {
        line = line.push(action);
    }
    container(line)
        .padding([0.0, spacing::M])
        .height(size::ROW_TALL + spacing::XS)
        .center_y(size::ROW_TALL + spacing::XS)
        .into()
}

/// The line that opens a hunk, with buttons at its right end.
pub fn hunk_bar<'a, M: 'a>(
    content: String,
    actions: Vec<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut line = row![
        text(content)
            .size(font_size::SMALL)
            .color(palette.dim_text)
            .wrapping(Wrapping::None),
        Space::new().width(Length::Fill),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    for action in actions {
        line = line.push(action);
    }
    container(line)
        .padding([0.0, spacing::CELL_PADDING_X])
        .height(size::ROW_TALL)
        .center_y(size::ROW_TALL)
        .width(Length::Fill)
        .style(theme::menu(palette))
        .into()
}

/// What a banner is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerTone {
    /// Something is under way and wants a decision.
    Attention,
    /// Something failed.
    Failure,
}

/// How high a banner without details is, so what lies below it can be placed by
/// its row, like a menu that hangs at a row.
pub const BANNER_HEIGHT: f32 = 66.0;

/// A strip across the page with a text and the buttons that answer it.
pub fn banner<'a, M: 'a>(
    tone: BannerTone,
    headline: String,
    explanation: Option<String>,
    details: Option<String>,
    actions: Vec<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let color = match tone {
        BannerTone::Attention => palette.accent,
        BannerTone::Failure => palette.error,
    };
    let fixed = details.is_none();
    let mut words = iced::widget::Column::new().spacing(spacing::XS / 2.0);
    words = words.push(
        text(headline)
            .size(font_size::BODY)
            .color(palette.text)
            .wrapping(Wrapping::Word),
    );
    if let Some(more) = explanation {
        words = words.push(
            text(more)
                .size(font_size::SMALL)
                .color(palette.dim_text)
                .wrapping(Wrapping::Word),
        );
    }
    if let Some(output) = details {
        words = words.push(
            container(
                text(output)
                    .font(crate::APP_FONT)
                    .size(font_size::FILE)
                    .color(palette.text)
                    .wrapping(Wrapping::Word),
            )
            .padding(spacing::M)
            .width(Length::Fill)
            .style(theme::menu(palette)),
        );
    }
    let mut line = Row::new()
        .push(container(words).width(Length::Fill))
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    for action in actions {
        line = line.push(action);
    }

    let strip = container(
        container(line)
            .padding([spacing::M, spacing::L])
            .width(Length::Fill)
            .style(theme::outlined_in(color)),
    )
    .padding([spacing::M, spacing::XXL]);
    if fixed {
        strip.height(BANNER_HEIGHT).into()
    } else {
        strip.into()
    }
}

/// `content`, and a right press on it sends `on_right_press`.
pub fn context_area<'a, M: Clone + 'a>(
    content: Element<'a, M>,
    on_right_press: M,
) -> Element<'a, M> {
    mouse_area(content).on_right_press(on_right_press).into()
}
