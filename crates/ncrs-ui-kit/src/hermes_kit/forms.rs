//! Form rows, fields and toggles.
use iced::widget::text::LineHeight;
use iced::widget::text::Wrapping;
use iced::widget::{
    button, column, container, mouse_area, row, text, text_input, Column, Row, Space,
};
use iced::{alignment, Background, Border, Color, Element, Font, Length, Pixels, Theme};

use crate::kit::{self, Check};
use crate::palette::Palette;
use crate::theme;

use super::*;

// ---------------------------------------------------------------------
// Forms
// ---------------------------------------------------------------------

/// A label of 160 px, then the control and under it a hint.
pub fn form_row<'a, M: Clone + 'a>(
    label: &str,
    control: Element<'a, M>,
    hint: Option<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut right = Column::new()
        .push(
            row![Space::new().height(CONTROL_HEIGHT), control].align_y(alignment::Vertical::Center),
        )
        .spacing(GAP_XS)
        .width(Length::Fill);
    if let Some(extra) = hint {
        right = right.push(extra);
    }
    row![
        container(body(label.to_owned(), palette.dim_text))
            .width(LABEL_WIDTH)
            .height(CONTROL_HEIGHT)
            .align_y(alignment::Vertical::Center),
        right,
    ]
    .spacing(GAP_M)
    .into()
}

/// The title of a group of rows, in capitals over a line.
pub fn section<'a, M: 'a>(title: &str, palette: &Palette) -> Element<'a, M> {
    column![
        Space::new().height(GAP_S),
        container(
            column![
                container(
                    text(title.to_uppercase())
                        .size(TEXT_SMALL)
                        .font(semibold())
                        .color(palette.dim_text)
                        .wrapping(Wrapping::None),
                )
                .height(Length::Fill)
                .align_y(alignment::Vertical::Bottom),
                Space::new().height(GAP_XS),
                theme::horizontal_rule(palette.border_inactive),
            ]
            .width(Length::Fill),
        )
        .height(ROW_ONE_LINE),
    ]
    .into()
}

/// A value of a row: text of 14 px in the text colour.
pub fn value<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    body(content, palette.text)
}

/// [`value`] that wraps where the line is full.
pub fn sentence<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    text(content)
        .size(TEXT_BODY)
        .line_height(LineHeight::Absolute(Pixels(BODY_LINE)))
        .color(palette.text)
        .wrapping(Wrapping::Word)
        .into()
}

/// [`value`] in the dimmed colour.
pub fn value_dim<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    body(content, palette.dim_text)
}

/// [`value`] in a colour of its own, for states.
pub fn value_in<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    color: Color,
) -> Element<'a, M> {
    body(content, color)
}

/// The items of one control cell side by side.
pub fn cell<'a, M: Clone + 'a>(items: Vec<Element<'a, M>>) -> Element<'a, M> {
    Row::with_children(items)
        .spacing(GAP_M)
        .align_y(alignment::Vertical::Center)
        .into()
}

/// A note of 12 px, dimmed.
pub fn hint<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    text(content)
        .size(TEXT_SMALL)
        .line_height(LineHeight::Absolute(Pixels(SMALL_LINE)))
        .color(palette.dim_text)
        .wrapping(Wrapping::Word)
        .into()
}

/// A problem: text in the error colour behind a bar of 2 px.
pub fn error_line<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, M> {
    row![
        bar(palette.error),
        text(content)
            .size(TEXT_SMALL)
            .line_height(LineHeight::Absolute(Pixels(SMALL_LINE)))
            .color(palette.error)
            .wrapping(Wrapping::Word),
    ]
    .spacing(GAP_S)
    // The bar fills its height, which would otherwise make the row take all free room.
    .height(Length::Shrink)
    .into()
}

/// Text in the monospace font.
pub fn mono<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    size: f32,
    color: Color,
) -> Element<'a, M> {
    text(content)
        .size(size)
        .font(Font::MONOSPACE)
        .color(color)
        .wrapping(Wrapping::None)
        .into()
}

fn field_style(palette: &Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    let base = theme::field_input(palette);
    let colors = *palette;
    move |theme, status| {
        let mut style = base(theme, status);
        style.background = Background::Color(colors.background);
        style
    }
}

fn field_in<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> M + 'a>,
    on_submit: M,
    monospace: bool,
    palette: &Palette,
) -> Element<'a, M> {
    let field = text_input(placeholder, value).id(id);
    let typed_in = if monospace {
        field.font(Font::MONOSPACE)
    } else {
        field
    };
    let wired = match on_input {
        Some(typed) => typed_in.on_input(typed).on_submit(on_submit),
        None => typed_in,
    };
    wired
        .size(TEXT_BODY)
        .line_height(LineHeight::Absolute(Pixels(BODY_LINE)))
        .padding([CONTROL_PAD_Y, MENU_PAD_X])
        .style(field_style(palette))
        .width(Length::Fill)
        .into()
}

/// A text field of 28 px. Without `on_input` it takes no typing.
pub fn text_field<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> M + 'a>,
    on_submit: M,
    palette: &Palette,
) -> Element<'a, M> {
    field_in(placeholder, value, id, on_input, on_submit, false, palette)
}

/// [`text_field`] in the monospace font.
pub fn mono_text_field<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> M + 'a>,
    on_submit: M,
    palette: &Palette,
) -> Element<'a, M> {
    field_in(placeholder, value, id, on_input, on_submit, true, palette)
}

/// A checkbox with its label; a click on either toggles.
pub fn checkbox<'a, M: Clone + 'a>(
    state: Check,
    on_toggle: Option<M>,
    label: Option<&str>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut line = Row::new()
        .push(kit::checkbox(state, None, palette))
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center);
    if let Some(caption) = label {
        line = line.push(body(caption.to_owned(), palette.text));
    }
    let area = mouse_area(
        container(line)
            .height(CONTROL_HEIGHT)
            .align_y(alignment::Vertical::Center),
    );
    match on_toggle {
        Some(message) => area.on_press(message).into(),
        None => area.into(),
    }
}

/// Buttons side by side of which one is on.
pub fn segmented<'a, M: Clone + 'a>(
    options: &[(&str, M)],
    selected: usize,
    palette: &Palette,
) -> Element<'a, M> {
    let colors = *palette;
    let buttons = options.iter().enumerate().map(|(index, (label, message))| {
        let chosen = index == selected;
        button(container(label_text(label)).center_y(Length::Fill))
            .padding([0.0, BUTTON_PAD_X])
            .height(CONTROL_HEIGHT)
            .on_press(message.clone())
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: (chosen || hovered).then_some(Background::Color(colors.title)),
                    text_color: if chosen { colors.text } else { colors.dim_text },
                    border: Border {
                        color: if chosen {
                            colors.success
                        } else {
                            colors.border
                        },
                        width: 1.0,
                        radius: RADIUS.into(),
                    },
                    ..button::Style::default()
                }
            })
            .into()
    });
    Row::with_children(buttons).spacing(GAP_XS).into()
}
