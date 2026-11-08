//! The header, the bubbles and the chips of the chat.
use iced::border::Radius;
use iced::widget::text::{LineHeight, Span, Wrapping};
use iced::widget::{button, container, rich_text, span, text, Column, Row, Space};
use iced::{alignment, Background, Border, Color, Element, Font, Length, Pixels, Theme};

use crate::palette::Palette;
use crate::theme::{self, DISABLED_OPACITY};

use super::*;

const AVATAR_SIZE: f32 = 30.0;
const BUBBLE_RADIUS: f32 = 16.0;
const BUBBLE_TIP: f32 = 4.0;
const BUBBLE_PAD_Y: f32 = 10.0;
const BUBBLE_PAD_X: f32 = 14.0;
const BUBBLE_MAX_WIDTH: f32 = 620.0;
const CHAT_TEXT: f32 = 14.0;
const CHAT_LINE: f32 = 20.0;
const CHIP_PAD_Y: f32 = 6.0;
const CHIP_PAD_X: f32 = 14.0;
const SMALL_CHIP_PAD_Y: f32 = 2.0;
const SMALL_CHIP_PAD_X: f32 = 10.0;
/// Puts the avatar level with the first line of a bubble.
const AVATAR_TOP_NUDGE: f32 = BUBBLE_PAD_Y + (CHAT_LINE - AVATAR_SIZE) / 2.0;
const BOLD_MARK: &str = "**";
const PATH_OPACITY: f32 = 0.8;

/// The text with its `**bold**` stretches as spans.
fn spans_of<'a>(content: &str, color: Color) -> Vec<Span<'a, (), Font>> {
    content
        .split(BOLD_MARK)
        .enumerate()
        .filter(|(_, piece)| !piece.is_empty())
        .map(|(index, piece)| {
            let plain = span(piece.to_owned()).color(color);
            if index % 2 == 1 {
                plain.font(semibold())
            } else {
                plain
            }
        })
        .collect()
}

/// A paragraph of the chat; `**bold**` is set in bold.
pub fn chat_text<'a, M: 'a>(content: &str, color: Color) -> Element<'a, M> {
    rich_text(spans_of(content, color))
        .size(CHAT_TEXT)
        .line_height(LineHeight::Absolute(Pixels(CHAT_LINE)))
        .wrapping(Wrapping::WordOrGlyph)
        .into()
}

/// The round mark of Hermes: a letter on the filled button colour.
pub fn avatar<'a, M: 'a>(letter: &str, palette: &Palette) -> Element<'a, M> {
    let (fill, ink) = palette.primary_button();
    container(
        text(letter.to_owned())
            .size(CHAT_TEXT)
            .font(semibold())
            .color(ink)
            .wrapping(Wrapping::None),
    )
    .width(AVATAR_SIZE)
    .height(AVATAR_SIZE)
    .center_x(AVATAR_SIZE)
    .center_y(AVATAR_SIZE)
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(fill)),
        border: Border {
            radius: theme::radius::CORNER.into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

/// The top of the chat: the avatar, the name with what Hermes does, and at the
/// right the button that starts a new chat and the key that closes the chat.
pub fn chat_header<'a, M: Clone + 'a>(
    letter: &str,
    name: &str,
    tagline: &str,
    new_chat: (&str, M),
    close_hint: &str,
    palette: &Palette,
) -> Element<'a, M> {
    Row::new()
        .push(avatar(letter, palette))
        .push(
            Column::new()
                .push(
                    text(name.to_owned())
                        .size(CHAT_TEXT)
                        .font(semibold())
                        .color(palette.text)
                        .wrapping(Wrapping::None),
                )
                .push(small(tagline.to_owned(), palette.dim_text)),
        )
        .push(Space::new().width(Length::Fill))
        .push(chat_chip_small(new_chat.0, false, new_chat.1, palette))
        .push(small(close_hint.to_owned(), palette.dim_text))
        .spacing(GAP_S + GAP_XS)
        .padding([0.0, THREAD_PADDING])
        .align_y(alignment::Vertical::Center)
        .into()
}

/// What Hermes says: on the left, next to the avatar. `parts` are paragraphs
/// ([`chat_text`]) and cards.
pub fn hermes_bubble<'a, M: 'a>(parts: Vec<Element<'a, M>>, palette: &Palette) -> Element<'a, M> {
    let colors = *palette;
    let body = container(Column::with_children(parts).spacing(GAP_S))
        .padding([BUBBLE_PAD_Y, BUBBLE_PAD_X])
        .max_width(BUBBLE_MAX_WIDTH)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(colors.dialog)),
            border: Border {
                color: colors.border_inactive,
                width: 1.0,
                radius: Radius {
                    top_left: BUBBLE_RADIUS,
                    top_right: BUBBLE_RADIUS,
                    bottom_right: BUBBLE_RADIUS,
                    bottom_left: BUBBLE_TIP,
                },
            },
            ..container::Style::default()
        });
    Row::new()
        .push(container(avatar("H", palette)).padding([AVATAR_TOP_NUDGE, 0.0]))
        .push(body)
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Top)
        .into()
}

/// What the user said: on the right, on the filled button colour.
pub fn user_bubble<'a, M: 'a>(content: &str, palette: &Palette) -> Element<'a, M> {
    user_bubble_around(chat_text(content, palette.primary_button().1), palette)
}

/// A folder the user picked or typed: its name, with its path in small type
/// below it, cut in the middle.
pub fn user_folder_bubble<'a, M: 'a>(name: &str, path: &str, palette: &Palette) -> Element<'a, M> {
    let ink = palette.primary_button().1;
    user_bubble_around(
        Column::new()
            .push(chat_text(name, ink))
            .push(path_hint(
                "",
                path,
                TEXT_SMALL,
                ink.scale_alpha(PATH_OPACITY),
                palette,
            ))
            .into(),
        palette,
    )
}

fn user_bubble_around<'a, M: 'a>(content: Element<'a, M>, palette: &Palette) -> Element<'a, M> {
    let (fill, _) = palette.primary_button();
    let body = container(content)
        .padding([BUBBLE_PAD_Y, BUBBLE_PAD_X])
        .max_width(BUBBLE_MAX_WIDTH)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(fill)),
            border: Border {
                radius: Radius {
                    top_left: BUBBLE_RADIUS,
                    top_right: BUBBLE_RADIUS,
                    bottom_right: BUBBLE_TIP,
                    bottom_left: BUBBLE_RADIUS,
                },
                ..Border::default()
            },
            ..container::Style::default()
        });
    Row::new()
        .push(Space::new().width(Length::Fill))
        .push(body)
        .into()
}

/// Hermes at work: a dimmed line in the place of the answer.
pub fn working_bubble<'a, M: 'a>(content: &str, palette: &Palette) -> Element<'a, M> {
    hermes_bubble(vec![chat_text(content, palette.dim_text)], palette)
}

fn chip_style(
    palette: &Palette,
    primary: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    let (fill, ink) = colors.primary_button();
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let mut style = if primary {
            button::Style {
                background: Some(Background::Color(fill)),
                text_color: ink,
                border: Border {
                    color: fill,
                    width: 1.0,
                    radius: theme::radius::CORNER.into(),
                },
                ..button::Style::default()
            }
        } else {
            button::Style {
                background: hovered.then_some(Background::Color(colors.title)),
                text_color: colors.text,
                border: Border {
                    color: colors.border,
                    width: 1.0,
                    radius: theme::radius::CORNER.into(),
                },
                ..button::Style::default()
            }
        };
        if matches!(status, button::Status::Disabled) {
            style.text_color = style.text_color.scale_alpha(DISABLED_OPACITY);
            style.border.color = style.border.color.scale_alpha(DISABLED_OPACITY);
        }
        style
    }
}

/// An answer to pick, as a pill. The primary one is filled.
pub fn chat_chip<'a, M: Clone + 'a>(
    label: &str,
    primary: bool,
    on_press: M,
    palette: &Palette,
) -> Element<'a, M> {
    button(
        text(label.to_owned())
            .size(CHAT_TEXT)
            .wrapping(Wrapping::None),
    )
    .padding([CHIP_PAD_Y, CHIP_PAD_X])
    .on_press(on_press)
    .style(chip_style(palette, primary))
    .into()
}

/// [`chat_chip`] for a choice inside a card; `selected` is the one chosen.
pub fn chat_chip_small<'a, M: Clone + 'a>(
    label: &str,
    selected: bool,
    on_press: M,
    palette: &Palette,
) -> Element<'a, M> {
    button(
        text(label.to_owned())
            .size(theme::font_size::SMALL)
            .wrapping(Wrapping::None),
    )
    .padding([SMALL_CHIP_PAD_Y, SMALL_CHIP_PAD_X])
    .on_press(on_press)
    .style(chip_style(palette, selected))
    .into()
}

/// Chips side by side that move to the next line where the row is full.
pub fn chip_row<'a, M: 'a>(chips: Vec<Element<'a, M>>) -> Element<'a, M> {
    Row::with_children(chips)
        .spacing(GAP_S)
        .wrap()
        .vertical_spacing(GAP_S)
        .into()
}
