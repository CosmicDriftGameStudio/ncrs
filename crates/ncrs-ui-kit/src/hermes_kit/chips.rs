//! Chips, pieces and filters.
use iced::widget::text::LineHeight;
use iced::widget::{button, container, opaque, pick_list, stack, Row, Space};
use iced::{alignment, Background, Border, Color, Element, Length, Pixels};

use crate::menu_button::Entry;
use crate::palette::Palette;
use crate::theme::{self, DISABLED_OPACITY};

use super::*;

// ---------------------------------------------------------------------
// Chips, pieces and filters
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipKind {
    /// Set by Hermes: a grey fill.
    Automatic,
    /// Set by the user: an accent outline.
    Own,
}

fn remove_button<'a, M: Clone + 'a>(
    on_remove: M,
    color: Color,
    palette: &Palette,
) -> Element<'a, M> {
    button(small(REMOVE, color))
        .on_press(on_remove)
        .padding([0.0, GAP_XS])
        .style(theme::close_button(palette))
        .into()
}

/// A short label of 20 px; an own one may carry a cross.
pub fn chip<'a, M: Clone + 'a>(
    label: &str,
    kind: ChipKind,
    on_remove: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
    let colors = *palette;
    let ink = match kind {
        ChipKind::Automatic => colors.dim_text,
        ChipKind::Own => colors.accent,
    };
    let mut line = Row::new()
        .push(small(label.to_owned(), ink))
        .spacing(GAP_XS)
        .align_y(alignment::Vertical::Center);
    if let Some(message) = on_remove {
        line = line.push(remove_button(message, ink, palette));
    }
    container(line)
        .padding([0.0, GAP_S])
        .height(CHIP_HEIGHT)
        .align_y(alignment::Vertical::Center)
        .style(move |_| container::Style {
            background: (kind == ChipKind::Automatic)
                .then_some(Background::Color(colors.border_inactive)),
            border: Border {
                color: if kind == ChipKind::Own {
                    colors.accent
                } else {
                    Color::TRANSPARENT
                },
                width: 1.0,
                radius: (CHIP_HEIGHT / 2.0).into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn filter_caption(label: &str, chosen: Option<&str>) -> String {
    match chosen {
        Some(value) => format!("{label}: {value}"),
        None => label.to_owned(),
    }
}

/// A filter of 28 px: its name with an arrow while nothing is chosen, dimmed
/// and without a menu while there is nothing to choose from. With a value
/// chosen it is framed in the success colour on a quiet fill: the name
/// dimmed, the value, and inside the frame a cross that clears it. `items`
/// are the values to choose from; `width` is the same for every filter of a
/// row.
pub fn filter<'a, M: Clone + 'a>(
    label: &str,
    chosen: Option<&str>,
    items: &[MenuItem<M>],
    width: Length,
    on_clear: M,
    palette: &Palette,
) -> Element<'a, M> {
    let choices = entries(items);
    let caption = filter_caption(label, chosen);
    let colors = *palette;
    let Some(value) = chosen else {
        if choices.is_empty() {
            return filter_without_values(&caption, width, palette);
        }
        let base = theme::dropdown_field(palette);
        return pick_list(choices, None::<Entry<M>>, |choice| choice.message)
            .placeholder(caption)
            .width(width)
            .padding([CONTROL_PAD_Y, MENU_PAD_X])
            .text_size(TEXT_BODY)
            .text_line_height(LineHeight::Absolute(Pixels(BODY_LINE)))
            .style(move |theme, status| {
                let mut style = base(theme, status);
                style.background = Background::Color(colors.background);
                style
            })
            .menu_style(theme::dropdown_menu(palette))
            .into();
    };
    let inner_pad_y = CONTROL_PAD_Y - FILTER_BORDER;
    let shown = container(
        Row::new()
            .push(body(format!("{label}:"), colors.dim_text))
            .push(body(value.to_owned(), colors.text))
            .spacing(GAP_XS),
    )
    .padding([inner_pad_y, MENU_PAD_X])
    .width(Length::Fill);
    let opener = pick_list(choices, None::<Entry<M>>, |choice| choice.message)
        .handle(pick_list::Handle::None)
        .width(Length::Fill)
        .padding([inner_pad_y, MENU_PAD_X])
        .text_size(TEXT_BODY)
        .text_line_height(LineHeight::Absolute(Pixels(BODY_LINE)))
        .style(move |_, _| pick_list::Style {
            text_color: Color::TRANSPARENT,
            placeholder_color: Color::TRANSPARENT,
            handle_color: Color::TRANSPARENT,
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
        })
        .menu_style(theme::dropdown_menu(palette));
    chosen_frame(
        Row::new()
            .push(stack![shown, opener].width(Length::Fill))
            .push(remove_button(on_clear, colors.dim_text, palette))
            .width(Length::Fill)
            .into(),
        width,
        palette,
    )
}

/// A chosen value in its frame: the success colour on a quiet fill.
fn chosen_frame<'a, M: Clone + 'a>(
    content: Element<'a, M>,
    width: Length,
    palette: &Palette,
) -> Element<'a, M> {
    let colors = *palette;
    container(content)
        .width(width)
        .height(CONTROL_HEIGHT)
        .align_y(alignment::Vertical::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(colors.title)),
            border: Border {
                color: colors.success,
                width: FILTER_BORDER,
                radius: theme::radius::CONTROL.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// A filter that is only shown: the name dimmed, the value, no menu and no
/// cross.
pub fn criterion<'a, M: Clone + 'a>(label: &str, value: &str, palette: &Palette) -> Element<'a, M> {
    chosen_frame(
        container(
            Row::new()
                .push(body(format!("{label}:"), palette.dim_text))
                .push(body(value.to_owned(), palette.text))
                .spacing(GAP_XS),
        )
        .padding([CONTROL_PAD_Y - FILTER_BORDER, MENU_PAD_X])
        .into(),
        Length::Shrink,
        palette,
    )
}

/// The filter of a facet the documents have no value for: built like an open
/// one, dimmed, and nothing to open.
fn filter_without_values<'a, M: Clone + 'a>(
    caption: &str,
    width: Length,
    palette: &Palette,
) -> Element<'a, M> {
    let colors = *palette;
    let base = theme::dropdown_field(palette);
    let dimmed = pick_list(Vec::<Entry<M>>::new(), None::<Entry<M>>, |choice| {
        choice.message
    })
    .placeholder(caption.to_owned())
    .width(width)
    .padding([CONTROL_PAD_Y, MENU_PAD_X])
    .text_size(TEXT_BODY)
    .text_line_height(LineHeight::Absolute(Pixels(BODY_LINE)))
    .style(move |theme, _| {
        let mut style = base(theme, pick_list::Status::Active);
        style.background = Background::Color(colors.background);
        style.placeholder_color = colors.dim_text;
        style.handle_color = colors.dim_text;
        style.border.color = colors.border.scale_alpha(DISABLED_OPACITY);
        style
    });
    stack![
        dimmed,
        opaque(Space::new().width(width).height(CONTROL_HEIGHT))
    ]
    .into()
}

/// A piece of a path or a file name, 24 px, with a cross where it can go.
pub fn piece<'a, M: Clone + 'a>(
    label: &str,
    on_remove: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
    let colors = *palette;
    let mut line = Row::new()
        .push(small(label.to_owned(), colors.text))
        .spacing(GAP_XS)
        .align_y(alignment::Vertical::Center);
    if let Some(message) = on_remove {
        line = line.push(remove_button(message, colors.dim_text, palette));
    }
    container(line)
        .padding([0.0, GAP_S])
        .height(PIECE_HEIGHT)
        .align_y(alignment::Vertical::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(colors.title)),
            border: Border {
                color: colors.border_inactive,
                width: 1.0,
                radius: RADIUS.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// The pieces in a row with `separator` between them and, last, the button
/// that adds one.
pub fn pieces<'a, M: Clone + 'a>(
    items: Vec<Element<'a, M>>,
    separator: &str,
    add: Option<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut line = Row::new()
        .spacing(GAP_XS)
        .align_y(alignment::Vertical::Center);
    let mut first = true;
    for item in items {
        if !first {
            line = line.push(small(separator.to_owned(), palette.dim_text));
        }
        first = false;
        line = line.push(item);
    }
    if let Some(extra) = add {
        line = line.push(extra);
    }
    line.wrap().vertical_spacing(GAP_XS).into()
}
