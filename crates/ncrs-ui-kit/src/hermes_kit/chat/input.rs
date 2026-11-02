//! The input of the chat: a round field with a round send button.
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{button, container, text, text_input, tooltip, Row};
use iced::{alignment, Background, Border, Element, Length, Pixels, Theme};

use crate::palette::Palette;
use crate::theme;

use super::*;

const INPUT_PAD_Y: f32 = 10.0;
const INPUT_PAD_X: f32 = 18.0;
const SEND_SIZE: f32 = 38.0;
const PILL: f32 = 999.0;
const SEND_ARROW: &str = "↑";

fn field_style(palette: &Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    let base = theme::field_input(palette);
    let background = palette.dialog;
    move |theme, status| {
        let mut style = base(theme, status);
        style.background = Background::Color(background);
        style.border.radius = PILL.into();
        style
    }
}

fn send_style(palette: &Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    let (fill, ink) = colors.primary_button();
    move |_, status| {
        let disabled = matches!(status, button::Status::Disabled);
        button::Style {
            background: Some(Background::Color(if disabled {
                fill.scale_alpha(theme::DISABLED_OPACITY)
            } else {
                fill
            })),
            text_color: ink,
            border: Border {
                radius: PILL.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// What the input shows and answers.
#[derive(Debug)]
pub struct ChatInput<'s, M> {
    pub value: &'s str,
    pub placeholder: &'s str,
    pub id: &'static str,
    pub on_input: fn(String) -> M,
    pub on_submit: M,
    /// Without it the field takes no typing and the button does nothing, for
    /// as long as Hermes works.
    pub enabled: bool,
    pub send_label: &'s str,
}

/// The field and the send button.
pub fn chat_input<'a, M: Clone + 'a>(props: ChatInput<'_, M>, palette: &Palette) -> Element<'a, M> {
    let ChatInput {
        value,
        placeholder,
        id,
        on_input,
        on_submit,
        enabled,
        send_label,
    } = props;
    let base = text_input(placeholder, value)
        .id(id)
        .size(TEXT_BODY)
        .line_height(LineHeight::Absolute(Pixels(BODY_LINE)))
        .padding([INPUT_PAD_Y, INPUT_PAD_X])
        .style(field_style(palette))
        .width(Length::Fill);
    let field = if enabled {
        base.on_input(on_input).on_submit(on_submit.clone())
    } else {
        base
    };
    let can_send = enabled && !value.trim().is_empty();
    let send = button(
        container(text(SEND_ARROW).size(TEXT_TITLE).wrapping(Wrapping::None))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(SEND_SIZE)
    .height(SEND_SIZE)
    .padding(0.0)
    .on_press_maybe(can_send.then_some(on_submit))
    .style(send_style(palette));
    let tip = container(small(send_label.to_owned(), palette.text))
        .padding(GAP_XS)
        .style(theme::menu(palette));
    Row::new()
        .push(field)
        .push(tooltip(send, tip, tooltip::Position::Top))
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center)
        .into()
}
