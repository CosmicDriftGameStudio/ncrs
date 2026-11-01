//! Buttons, menus and dropdowns.
use iced::widget::text::Wrapping;
use iced::widget::{button, container, pick_list, text, Row};
use iced::{alignment, Background, Border, Element, Length, Padding, Size, Theme};

use crate::format;
use crate::menu_button::{Entry, MenuButton};
use crate::palette::Palette;
use crate::theme::{self, DISABLED_OPACITY};

use super::*;

// ---------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::hermes_kit) enum ButtonLook {
    Primary,
    Secondary,
    Link,
}

pub(in crate::hermes_kit) fn button_style(
    palette: &Palette,
    look: ButtonLook,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    let (fill, ink) = colors.primary_button();
    move |_, status| {
        let disabled = matches!(status, button::Status::Disabled);
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let mut style = match look {
            ButtonLook::Primary => button::Style {
                background: Some(Background::Color(fill)),
                text_color: ink,
                border: Border {
                    color: if hovered { colors.accent } else { fill },
                    width: 1.0,
                    radius: RADIUS.into(),
                },
                ..button::Style::default()
            },
            ButtonLook::Secondary => button::Style {
                background: hovered.then_some(Background::Color(colors.title)),
                text_color: colors.text,
                border: Border {
                    color: if hovered {
                        colors.accent
                    } else {
                        colors.border
                    },
                    width: 1.0,
                    radius: RADIUS.into(),
                },
                ..button::Style::default()
            },
            ButtonLook::Link => button::Style {
                background: None,
                text_color: if hovered { colors.text } else { colors.accent },
                border: Border::default(),
                ..button::Style::default()
            },
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

fn sized_button<'a, M: Clone + 'a>(
    content: Element<'a, M>,
    on_press: Option<M>,
    look: ButtonLook,
    palette: &Palette,
) -> Element<'a, M> {
    let padding = if look == ButtonLook::Link {
        0.0
    } else {
        BUTTON_PAD_X
    };
    button(container(content).center_y(Length::Fill))
        .padding([0.0, padding])
        .height(CONTROL_HEIGHT)
        .on_press_maybe(on_press)
        .style(button_style(palette, look))
        .into()
}

pub(in crate::hermes_kit) fn label_text<'a, M: Clone + 'a>(label: &str) -> Element<'a, M> {
    text(label.to_owned())
        .size(TEXT_BODY)
        .wrapping(Wrapping::None)
        .into()
}

/// The one button of a page that is filled; `key` is the key that presses it.
pub fn primary<'a, M: Clone + 'a>(
    label: &str,
    key: Option<&str>,
    on_press: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut content = Row::new()
        .push(label_text(label))
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center);
    if let Some(shortcut) = key {
        let (_, ink) = palette.primary_button();
        content = content.push(small(shortcut.to_owned(), ink.scale_alpha(KEY_HINT_ALPHA)));
    }
    sized_button(content.into(), on_press, ButtonLook::Primary, palette)
}

pub fn secondary<'a, M: Clone + 'a>(
    label: &str,
    on_press: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
    sized_button(label_text(label), on_press, ButtonLook::Secondary, palette)
}

/// Text in the accent colour, without frame.
pub fn link<'a, M: Clone + 'a>(
    label: &str,
    on_press: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
    sized_button(label_text(label), on_press, ButtonLook::Link, palette)
}

// ---------------------------------------------------------------------
// Menus and dropdowns, one implementation on a pick list
// ---------------------------------------------------------------------

/// One entry of a menu.
#[derive(Debug, Clone)]
pub struct MenuItem<M> {
    pub label: String,
    pub key_hint: Option<String>,
    pub message: M,
}

impl<M> MenuItem<M> {
    pub fn new(label: impl Into<String>, message: M) -> Self {
        Self {
            label: label.into(),
            key_hint: None,
            message,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuLook {
    /// The `…` button of 28 x 28.
    Icon,
    /// The "+ add …" button at the end of a row of pieces.
    PieceAdd,
    /// A button of the page header that shows its caption and opens the list.
    Button,
}

/// The text of a menu row: the label, and the key behind it.
fn menu_row_text<M: Clone>(item: &MenuItem<M>) -> String {
    match &item.key_hint {
        Some(key) => format!("{}    {key}", item.label),
        None => item.label.clone(),
    }
}

pub(in crate::hermes_kit) fn entries<M: Clone>(items: &[MenuItem<M>]) -> Vec<Entry<M>> {
    items
        .iter()
        .enumerate()
        .map(|(position, item)| Entry {
            label: menu_row_text(item),
            position,
            message: item.message.clone(),
        })
        .collect()
}

pub(in crate::hermes_kit) fn pick_style(
    palette: &Palette,
    look: MenuLook,
) -> impl Fn(&Theme, pick_list::Status) -> pick_list::Style {
    let base = theme::dropdown_field(palette);
    let colors = *palette;
    move |theme, status| {
        let mut style = base(theme, status);
        style.background = Background::Color(colors.background);
        match look {
            MenuLook::Icon => style.text_color = colors.text,
            MenuLook::Button => {
                style.text_color = colors.text;
                style.placeholder_color = colors.text;
            }
            MenuLook::PieceAdd => {
                style.placeholder_color = colors.accent;
                style.border.color = if matches!(status, pick_list::Status::Active) {
                    colors.border_inactive
                } else {
                    colors.accent
                };
            }
        }
        style
    }
}

/// A button that opens a list of entries below it. Picking one sends its
/// message; the button itself keeps showing `caption`.
pub fn menu_button<'a, M: Clone + 'a>(
    caption: &str,
    items: &[MenuItem<M>],
    look: MenuLook,
    palette: &Palette,
) -> Element<'a, M> {
    let choices = entries(items);
    let widest = choices
        .iter()
        .map(|choice| format::estimated_width(&choice.label, TEXT_BODY))
        .chain([format::estimated_width(caption, TEXT_BODY)])
        .fold(0.0, f32::max);
    let caption_width = format::estimated_width(caption, TEXT_BODY);
    let (look_width, pad_x, pad_y) = match look {
        MenuLook::Icon => (CONTROL_HEIGHT, ICON_PAD_X, CONTROL_PAD_Y),
        MenuLook::PieceAdd => (widest + 2.0 * MENU_PAD_X, MENU_PAD_X, PIECE_PAD_Y + 2.0),
        MenuLook::Button => (caption_width + 2.0 * MENU_PAD_X, MENU_PAD_X, CONTROL_PAD_Y),
    };
    let shown = if look == MenuLook::Icon {
        ELLIPSIS
    } else {
        caption
    };
    let height = BODY_LINE + 2.0 * pad_y;
    let width = match look {
        MenuLook::Icon => look_width,
        _ => look_width.max(CONTROL_HEIGHT),
    };
    MenuButton::new(
        choices,
        shown.to_owned(),
        Size::new(width, height),
        Padding::from([pad_y, pad_x]),
        TEXT_BODY,
        pick_style(palette, look),
        theme::dropdown_menu(palette),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Page {
        Overview,
        Review,
    }

    #[test]
    fn a_menu_row_shows_the_key_behind_the_label() {
        let item = MenuItem {
            label: "Pause".to_owned(),
            key_hint: Some("P".to_owned()),
            message: Page::Overview,
        };
        assert_eq!(menu_row_text(&item), "Pause    P");
        assert_eq!(menu_row_text(&MenuItem::new("Open", Page::Review)), "Open");
    }

    #[test]
    fn entries_with_the_same_label_stay_apart() {
        let same = |page| MenuItem::new("Same", page);
        let found = entries(&[same(Page::Overview), same(Page::Review)]);
        assert_ne!(found.first(), found.get(1));
    }
}
