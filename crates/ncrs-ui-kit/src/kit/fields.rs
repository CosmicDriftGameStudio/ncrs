//! Text fields and the checkbox.

use iced::widget::checkbox as widget_checkbox;
use iced::widget::text::{LineHeight, Shaping};
use iced::widget::text_input;
use iced::{Element, Length, Pixels};

use crate::palette::Palette;
use crate::theme::{self, font_size, spacing};

/// The height of the text inside a field: with the padding it makes
/// [`size::ROW_TALL`].
pub const FIELD_LINE_HEIGHT: f32 = 16.0;

/// A single-line text field of 24 px. Without `on_input` it takes no typing:
/// it is shown disabled.
pub fn text_field<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> M + 'a>,
    on_submit: M,
    palette: &Palette,
) -> Element<'a, M> {
    field_in(
        placeholder,
        value,
        id,
        on_input,
        on_submit,
        Look::Plain,
        palette,
    )
}

/// A text field that shows dots instead of what is typed, for a password.
pub fn secret_field<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> M + 'a>,
    on_submit: M,
    palette: &Palette,
) -> Element<'a, M> {
    field_in(
        placeholder,
        value,
        id,
        on_input,
        on_submit,
        Look::Secret,
        palette,
    )
}

/// A text field for a path or a name, set in the monospace font.
pub fn mono_text_field<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> M + 'a>,
    on_submit: M,
    palette: &Palette,
) -> Element<'a, M> {
    field_in(
        placeholder,
        value,
        id,
        on_input,
        on_submit,
        Look::Mono,
        palette,
    )
}

/// How a text field shows what is typed.
#[derive(Clone, Copy)]
enum Look {
    Plain,
    /// The monospace font, for paths and names.
    Mono,
    /// Dots, for a password.
    Secret,
}

fn field_in<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> M + 'a>,
    on_submit: M,
    look: Look,
    palette: &Palette,
) -> Element<'a, M> {
    let plain = text_input(placeholder, value)
        .id(id)
        .secure(matches!(look, Look::Secret));
    let fonted = match look {
        Look::Mono => plain.font(crate::APP_FONT),
        Look::Plain | Look::Secret => plain,
    };
    let field = match on_input {
        Some(typed) => fonted.on_input(typed).on_submit(on_submit),
        None => fonted,
    };
    field
        .size(font_size::BODY)
        .line_height(LineHeight::Absolute(Pixels(FIELD_LINE_HEIGHT)))
        .padding([spacing::XS, spacing::M])
        .style(theme::field_input(palette))
        .width(Length::Fill)
        .into()
}

/// The state of a checkbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Off,
    On,
    /// Some of what the box stands for is on, like a group of which only part
    /// of the entries is chosen.
    Mixed,
}

/// The side of a checkbox.
const CHECKBOX_SIDE: f32 = 14.0;

/// A checkbox without a label. `on_toggle` of `None` leaves the clicking to
/// the widget around it, like a list row.
pub fn checkbox<'a, M: Clone + 'a>(
    state: Check,
    on_toggle: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut boxed = widget_checkbox(state != Check::Off)
        .size(CHECKBOX_SIDE)
        .style(theme::dialog_checkbox(palette));
    if state == Check::Mixed {
        boxed = boxed.icon(widget_checkbox::Icon {
            font: crate::APP_FONT,
            code_point: '-',
            size: None,
            line_height: LineHeight::default(),
            shaping: Shaping::Basic,
        });
    }
    if let Some(message) = on_toggle {
        boxed = boxed.on_toggle(move |_| message.clone());
    }
    boxed.into()
}
