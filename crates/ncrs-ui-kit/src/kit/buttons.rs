//! Buttons, key caps, chips and the segmented group.

use iced::widget::button as widget_button;
use iced::widget::text::Wrapping;
use iced::widget::{container, row, text, Row};
use iced::{alignment, Element, Length, Theme};

use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing, DISABLED_OPACITY};

/// Horizontal room inside a button, either side of the label.
pub(in crate::kit) const BUTTON_PADDING_X: f32 = 10.0;

/// How strongly the outline of a key cap shows on a filled button.
const CAP_OUTLINE_ALPHA: f32 = 0.35;

/// What a button stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// The action the dialog is for: filled.
    Primary,
    /// Everything else: outlined.
    Secondary,
    /// An action that deletes: filled in the error color.
    Danger,
}

impl Tone {
    /// Filled for the button that is the focus of the dialog, outlined for the
    /// others.
    pub const fn filled_if(filled: bool) -> Self {
        if filled {
            Self::Primary
        } else {
            Self::Secondary
        }
    }
}

/// A chip the user can take away again: the label and a cross. The chip
/// is outlined in the "on" role.
pub fn removable_chip<'a, M: Clone + 'a>(
    label: &str,
    on_remove: M,
    palette: &Palette,
) -> Element<'a, M> {
    let cross = widget_button(text("×").size(font_size::SMALL))
        .padding([0.0, spacing::XS])
        .on_press(on_remove)
        .style(theme::close_button(palette));
    container(
        row![
            text(label.to_owned())
                .size(font_size::SMALL)
                .wrapping(Wrapping::None),
            cross
        ]
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center),
    )
    .padding([1.0, spacing::XS])
    .style(theme::own_chip(palette))
    .into()
}

/// A key as a small outlined cap.
pub fn key_cap<'a, M: Clone + 'a>(label: &str, palette: &Palette) -> Element<'a, M> {
    cap(label, false, palette)
}

/// A key cap in a menu row; on the highlighted (filled) row it is drawn in
/// the highlight's text color so it stays readable.
pub fn key_cap_in_row<'a, M: Clone + 'a>(
    label: &str,
    highlighted: bool,
    palette: &Palette,
) -> Element<'a, M> {
    cap(label, highlighted, palette)
}

/// The one key cap of a highlighted row that an edit acts on: filled in the
/// highlight's text color, so it stands out from the outlined caps beside it.
pub fn key_cap_focused<'a, M: Clone + 'a>(label: &str, palette: &Palette) -> Element<'a, M> {
    let base = theme::key_cap(palette, true);
    let (fill, ink) = (palette.cursor_text, palette.cursor);
    container(
        text(label.to_owned())
            .size(font_size::KEY_CAP)
            .wrapping(Wrapping::None),
    )
    .padding([0.0, spacing::XS - 1.0])
    .style(move |theme: &Theme| {
        let mut style = base(theme);
        style.background = Some(fill.into());
        style.text_color = Some(ink);
        style
    })
    .into()
}

pub(in crate::kit) fn cap<'a, M: Clone + 'a>(
    label: &str,
    on_fill: bool,
    palette: &Palette,
) -> Element<'a, M> {
    let base = theme::key_cap(palette, on_fill);
    container(
        text(label.to_owned())
            .size(font_size::KEY_CAP)
            .wrapping(Wrapping::None),
    )
    .padding([0.0, spacing::XS - 1.0])
    .style(move |theme: &Theme| {
        let mut style = base(theme);
        if on_fill {
            style.border.color = style.border.color.scale_alpha(CAP_OUTLINE_ALPHA);
        }
        style
    })
    .into()
}

fn tone_style(
    palette: &Palette,
    tone: Tone,
) -> impl Fn(&Theme, widget_button::Status) -> widget_button::Style {
    let primary = theme::dialog_button(palette, true);
    let secondary = theme::dialog_button(palette, false);
    let danger = theme::danger_button(palette);
    move |theme, status| {
        let disabled = matches!(status, widget_button::Status::Disabled);
        // A disabled button keeps its fill and outline, only fainter: without
        // them it reads as plain text.
        let shown = if disabled {
            widget_button::Status::Active
        } else {
            status
        };
        let mut style = match tone {
            Tone::Primary => primary(theme, shown),
            Tone::Secondary => secondary(theme, shown),
            Tone::Danger => danger(theme, shown),
        };
        if disabled {
            style.background = style
                .background
                .map(|background| background.scale_alpha(DISABLED_OPACITY));
            style.text_color = style.text_color.scale_alpha(DISABLED_OPACITY);
            style.border.color = style.border.color.scale_alpha(DISABLED_OPACITY);
        }
        style
    }
}

pub(in crate::kit) fn label_text<'a, M: Clone + 'a>(label: &str) -> Element<'a, M> {
    text(label.to_owned())
        .size(font_size::BODY)
        .wrapping(Wrapping::None)
        .into()
}

fn sized_button<'a, M: Clone + 'a>(
    content: Element<'a, M>,
    on_press: Option<M>,
    tone: Tone,
    palette: &Palette,
) -> Element<'a, M> {
    widget_button(container(content).center_y(Length::Fill))
        .padding([0.0, BUTTON_PADDING_X])
        .height(size::BUTTON)
        .on_press_maybe(on_press)
        .style(tone_style(palette, tone))
        .into()
}

/// A button of 24 px. `on_press` of `None` disables it.
pub fn button<'a, M: Clone + 'a>(
    label: &str,
    on_press: Option<M>,
    tone: Tone,
    palette: &Palette,
) -> Element<'a, M> {
    sized_button(label_text(label), on_press, tone, palette)
}

/// Like [`button`], with a small Enter cap: the button Enter presses.
pub fn enter_button<'a, M: Clone + 'a>(
    label: &str,
    on_press: Option<M>,
    tone: Tone,
    palette: &Palette,
) -> Element<'a, M> {
    let content = row![
        label_text(label),
        cap(ENTER_KEY, on_press.is_some(), palette)
    ]
    .spacing(spacing::XS)
    .align_y(alignment::Vertical::Center);
    sized_button(content.into(), on_press, tone, palette)
}

/// Like [`button`], with a small cap that names the key that presses it.
pub fn key_button<'a, M: Clone + 'a>(
    label: &str,
    key: &str,
    on_press: Option<M>,
    tone: Tone,
    palette: &Palette,
) -> Element<'a, M> {
    let content = row![label_text(label), cap(key, on_press.is_some(), palette)]
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center);
    sized_button(content.into(), on_press, tone, palette)
}

/// The label of the Enter key, on its cap and in the hints.
pub const ENTER_KEY: &str = "↵";

/// A group of buttons of which one is on, like the load setting of Hermes.
pub fn segmented<'a, M: Clone + 'a>(
    options: &[(&str, M)],
    selected: usize,
    enabled: bool,
    palette: &Palette,
) -> Element<'a, M> {
    let buttons = options.iter().enumerate().map(|(index, (label, message))| {
        widget_button(container(label_text(label)).center_y(Length::Fill))
            .padding([0.0, BUTTON_PADDING_X])
            .height(size::ROW)
            .on_press_maybe(enabled.then(|| message.clone()))
            .style(theme::segment_button(palette, index == selected))
            .into()
    });
    container(Row::with_children(buttons))
        .style(theme::segment_group(palette))
        .into()
}
