//! Text fields and the checkbox.

use iced::widget::checkbox as widget_checkbox;
use iced::widget::text::{LineHeight, Shaping};
use iced::widget::{text_editor, text_input};
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
    id: impl Into<iced::widget::Id>,
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
    id: impl Into<iced::widget::Id>,
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
    /// Plain, with room at the right end for a counter.
    Counted,
    /// The monospace font, for paths and names.
    Mono,
    /// Dots, for a password.
    Secret,
}

/// A borderless single-line field in the app font at the size of a file row,
/// for a line that reads like text (the command line, the path in the status
/// bar). `on_submit` is optional: a path that can only be edited has none.
pub fn line_field<'a, M: Clone + 'a>(
    value: &str,
    id: impl Into<iced::widget::Id>,
    on_input: impl Fn(String) -> M + 'a,
    on_submit: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
    let field = text_input("", value)
        .id(id)
        .on_input(on_input)
        .size(font_size::FILE)
        .font(crate::APP_FONT)
        .padding(0.0)
        .style(theme::path_input(palette))
        .width(Length::Fill);
    match on_submit {
        Some(message) => field.on_submit(message).into(),
        None => field.into(),
    }
}

fn field_in<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
    id: impl Into<iced::widget::Id>,
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
        Look::Plain | Look::Counted | Look::Secret => plain,
    };
    let right_room = if matches!(look, Look::Counted) {
        COUNTER_ROOM
    } else {
        spacing::M
    };
    let field = match on_input {
        Some(typed) => fonted.on_input(typed).on_submit(on_submit),
        None => fonted,
    };
    field
        .size(font_size::BODY)
        .line_height(LineHeight::Absolute(Pixels(FIELD_LINE_HEIGHT)))
        .padding(iced::Padding {
            top: spacing::XS,
            bottom: spacing::XS,
            left: spacing::M,
            right: right_room,
        })
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

/// One choice of a group of which exactly one is on: a round button with its
/// label; a click on either chooses it.
pub fn radio_choice<'a, M: Clone + 'a>(
    label: &str,
    selected: bool,
    on_choose: M,
    palette: &Palette,
) -> Element<'a, M> {
    iced::widget::container(
        iced::widget::radio(
            label.to_owned(),
            true,
            selected.then_some(true),
            move |_| on_choose.clone(),
        )
        .size(CHECKBOX_SIDE)
        .text_size(font_size::BODY)
        .spacing(spacing::M)
        .style(theme::dialog_radio(palette)),
    )
    .height(theme::size::ROW_TALL)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

/// A checkbox with its label; a click on either toggles.
pub fn labelled_checkbox<'a, M: Clone + 'a>(
    state: Check,
    on_toggle: Option<M>,
    label: Option<&str>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut line = iced::widget::row![checkbox(state, None, palette)]
        .spacing(spacing::M)
        .align_y(iced::alignment::Vertical::Center);
    if let Some(caption) = label {
        line = line.push(
            iced::widget::text(caption.to_owned())
                .size(font_size::BODY)
                .color(palette.text)
                .wrapping(iced::widget::text::Wrapping::None),
        );
    }
    let area = iced::widget::mouse_area(
        iced::widget::container(line)
            .height(theme::size::ROW_TALL)
            .align_y(iced::alignment::Vertical::Center),
    );
    match on_toggle {
        Some(message) => area.on_press(message).into(),
        None => area.into(),
    }
}

/// Room at the right end of a counted field, so what is typed stops before the counter.
const COUNTER_ROOM: f32 = 56.0;

/// A single-line field with `counter` ("12/72") written at its right end, in
/// the warning color while `over` is set. The counter only tells; it takes no
/// input and blocks nothing.
pub fn counted_text_field<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
    (counter, over): (String, bool),
    id: impl Into<iced::widget::Id>,
    on_input: Option<impl Fn(String) -> M + 'a>,
    on_submit: M,
    palette: &Palette,
) -> Element<'a, M> {
    let ink = if over {
        palette.error
    } else {
        palette.dim_text
    };
    let field = field_in(
        placeholder,
        value,
        id,
        on_input,
        on_submit,
        Look::Counted,
        palette,
    );
    iced::widget::stack![
        field,
        iced::widget::container(
            iced::widget::text(counter)
                .size(font_size::SMALL)
                .color(ink)
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([0.0, spacing::L])
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Center),
    ]
    .into()
}

/// A field for a longer text that wraps and takes lines, set in the monospace
/// font, as a note is edited. It fills the room it is given.
pub fn text_area<'a, M: Clone + 'a>(
    content: &'a text_editor::Content,
    id: &'static str,
    on_action: impl Fn(text_editor::Action) -> M + 'a,
    palette: &Palette,
) -> Element<'a, M> {
    text_area_hinted(content, id, "", on_action, palette)
}

/// [`text_area`] with a placeholder that shows while the field is empty.
pub fn text_area_hinted<'a, M: Clone + 'a>(
    content: &'a text_editor::Content,
    id: &'static str,
    placeholder: &str,
    on_action: impl Fn(text_editor::Action) -> M + 'a,
    palette: &Palette,
) -> Element<'a, M> {
    let base = theme::field_input(palette);
    text_editor(content)
        .id(id)
        .placeholder(placeholder.to_owned())
        .on_action(on_action)
        .font(crate::APP_FONT)
        .size(font_size::BODY)
        .padding(spacing::L)
        .height(Length::Fill)
        .style(move |theme, status| {
            let input_status = match status {
                text_editor::Status::Active => text_input::Status::Active,
                text_editor::Status::Hovered => text_input::Status::Hovered,
                text_editor::Status::Focused { is_hovered } => {
                    text_input::Status::Focused { is_hovered }
                }
                text_editor::Status::Disabled => text_input::Status::Disabled,
            };
            let style = base(theme, input_status);
            text_editor::Style {
                background: style.background,
                border: style.border,
                placeholder: style.placeholder,
                value: style.value,
                selection: style.selection,
            }
        })
        .into()
}
