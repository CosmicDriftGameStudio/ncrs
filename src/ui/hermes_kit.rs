//! The building blocks of the filing pages. Every size of the pages lives
//! here; the pages put the blocks together and never pick a size themselves.
//! The look follows the boards: a navigation, a page header of 72 px, the
//! content, an action bar of 48 px and a status bar of 22 px.

use iced::font::Weight;
use iced::widget::text::LineHeight;
use iced::widget::text::Wrapping;
use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, text, text_input, Column,
    Row, Space,
};
use iced::{alignment, Background, Border, Color, Element, Font, Length, Padding, Pixels, Theme};

use super::format;
use super::kit::{self, Check};
use super::theme::{self, DISABLED_OPACITY};
use crate::i18n::{Language, Msg};
use crate::messages::Message;
use crate::palette::Palette;

pub const HEADER_HEIGHT: f32 = 72.0;
pub const ACTION_BAR_HEIGHT: f32 = 48.0;
pub const STATUS_BAR_HEIGHT: f32 = 22.0;
pub const NAV_WIDTH: f32 = 200.0;
pub const ROW_ONE_LINE: f32 = 28.0;
pub const LABEL_WIDTH: f32 = 160.0;
pub const CONTROL_HEIGHT: f32 = 28.0;
pub const CHIP_HEIGHT: f32 = 20.0;
pub const PIECE_HEIGHT: f32 = 24.0;
pub const TEXT_SMALL: f32 = 12.0;
pub const TEXT_BODY: f32 = 14.0;
pub const TEXT_TITLE: f32 = 16.0;
pub const GAP_XS: f32 = 4.0;
pub const GAP_S: f32 = 8.0;
pub const GAP_M: f32 = 16.0;
pub const GAP_L: f32 = 24.0;
pub const EXPLAIN_WIDTH: f32 = 640.0;
pub const FORM_WIDTH: f32 = 760.0;
pub const SELECTION_BAR: f32 = 2.0;

const RADIUS: f32 = 4.0;
const SMALL_LINE: f32 = 16.0;
const BODY_LINE: f32 = 18.0;
/// A control of `CONTROL_HEIGHT` is a line of 18 px with 5 px above and below.
const CONTROL_PAD_Y: f32 = 5.0;
const PIECE_PAD_Y: f32 = 3.0;
const EMPTY_TEXT_WIDTH: f32 = 360.0;
const EXPLAIN_LINES: f32 = 2.0;
const ICON_PAD_X: f32 = 9.0;
const BUTTON_PAD_X: f32 = 12.0;
const MENU_PAD_X: f32 = 8.0;
const KEY_HINT_ALPHA: f32 = 0.7;
const ELLIPSIS: &str = "…";
const REMOVE: &str = "×";
const SEPARATOR_HEIGHT: f32 = 1.0;

fn semibold() -> Font {
    Font {
        weight: Weight::Semibold,
        ..Font::DEFAULT
    }
}

fn small<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    color: Color,
) -> Element<'a, M> {
    text(content)
        .size(TEXT_SMALL)
        .line_height(LineHeight::Absolute(Pixels(SMALL_LINE)))
        .color(color)
        .wrapping(Wrapping::None)
        .into()
}

fn body<'a, M: 'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    color: Color,
) -> Element<'a, M> {
    text(content)
        .size(TEXT_BODY)
        .line_height(LineHeight::Absolute(Pixels(BODY_LINE)))
        .color(color)
        .wrapping(Wrapping::None)
        .into()
}

fn fill_style(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color)),
        ..container::Style::default()
    }
}

fn bar<'a, M: 'a>(color: Color) -> Element<'a, M> {
    container(Space::new())
        .width(SELECTION_BAR)
        .height(Length::Fill)
        .style(fill_style(color))
        .into()
}

// ---------------------------------------------------------------------
// Page frame
// ---------------------------------------------------------------------

/// The page: navigation left, right of it the header, the content and the
/// action bar, separated by lines.
pub fn tab_page<'a>(
    nav: Element<'a, Message>,
    header: Element<'a, Message>,
    content: Element<'a, Message>,
    action_bar: Option<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut main = Column::new()
        .push(header)
        .push(container(content).width(Length::Fill).height(Length::Fill));
    if let Some(bar) = action_bar {
        main = main.push(bar);
    }
    row![
        container(nav)
            .width(NAV_WIDTH)
            .height(Length::Fill)
            .style(theme::sidebar(palette)),
        theme::vertical_rule(palette.border_inactive),
        main.width(Length::Fill).height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// One entry of the navigation.
pub struct NavEntry {
    pub label: String,
    pub count: Option<String>,
    pub active: bool,
    pub on_press: Message,
}

/// The navigation: the name of the group in capitals with the beta mark, then
/// the entries. `focused` colours the bar of the active entry in the accent.
pub fn nav<'a>(
    heading: &str,
    beta: &str,
    entries: Vec<NavEntry>,
    focused: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    let head = row![
        small(heading.to_uppercase(), palette.dim_text),
        chip(beta, ChipKind::Automatic, None, palette),
    ]
    .spacing(GAP_S)
    .align_y(alignment::Vertical::Center)
    .padding(Padding {
        top: GAP_M,
        right: GAP_M,
        bottom: GAP_S,
        left: GAP_M,
    });
    let bar_color = if focused {
        palette.accent
    } else {
        palette.success
    };
    let items = entries.into_iter().map(|entry| {
        let marker = if entry.active {
            bar_color
        } else {
            Color::TRANSPARENT
        };
        let mut line = Row::new()
            .push(body(entry.label, palette.text))
            .push(Space::new().width(Length::Fill))
            .align_y(alignment::Vertical::Center);
        if let Some(count) = entry.count {
            line = line.push(small(count, palette.dim_text));
        }
        let content = row![
            bar(marker),
            container(line)
                .padding([0.0, GAP_M - SELECTION_BAR])
                .width(Length::Fill)
                .center_y(Length::Fill),
        ];
        button(content)
            .on_press(entry.on_press)
            .width(Length::Fill)
            .height(ROW_ONE_LINE)
            .padding(0.0)
            .style(row_button(palette, entry.active))
            .into()
    });
    Column::new().push(head).extend(items).into()
}

fn row_button(
    palette: &Palette,
    selected: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let colors = *palette;
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: (selected || hovered).then_some(Background::Color(colors.title)),
            text_color: colors.text,
            border: Border::default(),
            ..button::Style::default()
        }
    }
}

/// The header of a page: title, an explanation of up to two lines and the
/// actions on the right.
pub fn page_header<'a>(
    title: &str,
    theme_name: Option<&str>,
    explain: Option<String>,
    title_extra: Option<Element<'a, Message>>,
    actions: Vec<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut title_row = Row::new()
        .push(
            text(title.to_owned())
                .size(TEXT_TITLE)
                .font(semibold())
                .color(palette.text)
                .wrapping(Wrapping::None),
        )
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center);
    if let Some(name) = theme_name {
        title_row = title_row.push(small(name.to_owned(), palette.dim_text));
    }
    if let Some(extra) = title_extra {
        title_row = title_row.push(extra);
    }
    let mut texts = Column::new().push(title_row).spacing(GAP_XS);
    if let Some(sentence) = explain {
        texts = texts.push(
            container(
                text(sentence)
                    .size(TEXT_SMALL)
                    .line_height(LineHeight::Absolute(Pixels(SMALL_LINE)))
                    .color(palette.dim_text)
                    .wrapping(Wrapping::Word),
            )
            .max_width(EXPLAIN_WIDTH)
            .max_height(EXPLAIN_LINES * SMALL_LINE)
            .clip(true),
        );
    }
    let content = row![
        container(texts).width(Length::Fill),
        Row::with_children(actions)
            .spacing(GAP_S)
            .align_y(alignment::Vertical::Center),
    ]
    .spacing(GAP_M)
    .align_y(alignment::Vertical::Center);
    column![
        container(content)
            .padding([0.0, GAP_L])
            .width(Length::Fill)
            .center_y(Length::Fill),
        theme::horizontal_rule(palette.border_inactive),
    ]
    .height(HEADER_HEIGHT)
    .into()
}

/// The bar under the content: a note left, the buttons right.
pub fn action_bar<'a>(
    note: Option<String>,
    buttons: Vec<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
    let note_line: Element<'a, Message> = match note {
        Some(sentence) => container(small(sentence, palette.dim_text))
            .width(Length::Fill)
            .clip(true)
            .into(),
        None => Space::new().width(Length::Fill).into(),
    };
    let content = row![
        note_line,
        Row::with_children(buttons)
            .spacing(GAP_S)
            .align_y(alignment::Vertical::Center),
    ]
    .spacing(GAP_M)
    .align_y(alignment::Vertical::Center);
    column![
        theme::horizontal_rule(palette.border_inactive),
        container(content)
            .padding([0.0, GAP_L])
            .width(Length::Fill)
            .center_y(Length::Fill),
    ]
    .height(ACTION_BAR_HEIGHT)
    .into()
}

/// The status bar of an open page: the badge, the keys and the Hermes state.
pub fn status_bar<'a>(
    badge: &str,
    hints: &[(&str, Msg)],
    right: String,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let (fill, ink) = palette.primary_button();
    let badge_cell = container(
        text(badge.to_uppercase())
            .size(TEXT_SMALL)
            .font(semibold())
            .color(ink)
            .wrapping(Wrapping::None),
    )
    .padding([0.0, GAP_S])
    .center_y(Length::Fill)
    .style(fill_style(fill));
    let mut line = Row::new()
        .push(badge_cell)
        .spacing(GAP_M)
        .align_y(alignment::Vertical::Center);
    for (key, verb) in hints {
        line = line.push(
            row![
                kit::key_cap(key, palette),
                small(lang.text(*verb), palette.dim_text)
            ]
            .spacing(GAP_XS)
            .align_y(alignment::Vertical::Center),
        );
    }
    container(
        row![
            line,
            Space::new().width(Length::Fill),
            container(small(right, palette.dim_text)).padding([0.0, GAP_M]),
        ]
        .align_y(alignment::Vertical::Center),
    )
    .width(Length::Fill)
    .height(STATUS_BAR_HEIGHT)
    .style(theme::statusbar(palette))
    .into()
}

// ---------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum ButtonLook {
    Primary,
    Secondary,
    Link,
}

fn button_style(
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

fn sized_button<'a>(
    content: Element<'a, Message>,
    on_press: Option<Message>,
    look: ButtonLook,
    palette: &Palette,
) -> Element<'a, Message> {
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

fn label_text<'a>(label: &str) -> Element<'a, Message> {
    text(label.to_owned())
        .size(TEXT_BODY)
        .wrapping(Wrapping::None)
        .into()
}

/// The one button of a page that is filled; `key` is the key that presses it.
pub fn primary<'a>(
    label: &str,
    key: Option<&str>,
    on_press: Option<Message>,
    palette: &Palette,
) -> Element<'a, Message> {
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

pub fn secondary<'a>(
    label: &str,
    on_press: Option<Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    sized_button(label_text(label), on_press, ButtonLook::Secondary, palette)
}

/// Text in the accent colour, without frame.
pub fn link<'a>(label: &str, on_press: Option<Message>, palette: &Palette) -> Element<'a, Message> {
    sized_button(label_text(label), on_press, ButtonLook::Link, palette)
}

// ---------------------------------------------------------------------
// Menus and dropdowns, one implementation on a pick list
// ---------------------------------------------------------------------

/// One entry of a menu.
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub label: String,
    pub key_hint: Option<String>,
    pub message: Message,
}

impl MenuItem {
    pub fn new(label: impl Into<String>, message: Message) -> Self {
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
}

#[derive(Debug, Clone)]
struct Entry {
    label: String,
    position: usize,
    message: Message,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.position == other.position
    }
}

impl std::fmt::Display for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// The text of a menu row: the label, and the key behind it.
fn menu_row_text(item: &MenuItem) -> String {
    match &item.key_hint {
        Some(key) => format!("{}    {key}", item.label),
        None => item.label.clone(),
    }
}

fn entries(items: &[MenuItem]) -> Vec<Entry> {
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

fn pick_style(
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
pub fn menu_button<'a>(
    caption: &str,
    items: &[MenuItem],
    look: MenuLook,
    palette: &Palette,
) -> Element<'a, Message> {
    let choices = entries(items);
    let widest = choices
        .iter()
        .map(|choice| format::estimated_width(&choice.label, TEXT_BODY))
        .chain([format::estimated_width(caption, TEXT_BODY)])
        .fold(0.0, f32::max);
    let (width, pad_x, pad_y) = match look {
        MenuLook::Icon => (CONTROL_HEIGHT, ICON_PAD_X, CONTROL_PAD_Y),
        MenuLook::PieceAdd => (widest + 2.0 * MENU_PAD_X, MENU_PAD_X, PIECE_PAD_Y + 2.0),
    };
    let shown = if look == MenuLook::Icon {
        ELLIPSIS
    } else {
        caption
    };
    pick_list(choices, None::<Entry>, |choice| choice.message)
        .placeholder(shown.to_owned())
        .handle(pick_list::Handle::None)
        .width(match look {
            MenuLook::Icon => Length::Fixed(width),
            _ => Length::Fixed(width.max(CONTROL_HEIGHT)),
        })
        .padding([pad_y, pad_x])
        .text_size(TEXT_BODY)
        .text_line_height(LineHeight::Absolute(Pixels(BODY_LINE)))
        .style(pick_style(palette, look))
        .menu_style(theme::dropdown_menu(palette))
        .into()
}

// ---------------------------------------------------------------------
// Forms
// ---------------------------------------------------------------------

/// A label of 160 px, then the control and under it a hint.
pub fn form_row<'a>(
    label: &str,
    control: Element<'a, Message>,
    hint: Option<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
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
pub fn cell<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
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

fn field_in<'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> Message + 'a>,
    on_submit: Message,
    monospace: bool,
    palette: &Palette,
) -> Element<'a, Message> {
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
pub fn text_field<'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> Message + 'a>,
    on_submit: Message,
    palette: &Palette,
) -> Element<'a, Message> {
    field_in(placeholder, value, id, on_input, on_submit, false, palette)
}

/// A checkbox with its label; a click on either toggles.
pub fn checkbox<'a>(
    state: Check,
    on_toggle: Option<Message>,
    label: Option<&str>,
    palette: &Palette,
) -> Element<'a, Message> {
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
pub fn segmented<'a>(
    options: &[(&str, Message)],
    selected: usize,
    palette: &Palette,
) -> Element<'a, Message> {
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

fn remove_button<'a>(on_remove: Message, color: Color, palette: &Palette) -> Element<'a, Message> {
    button(small(REMOVE, color))
        .on_press(on_remove)
        .padding([0.0, GAP_XS])
        .style(theme::close_button(palette))
        .into()
}

/// A short label of 20 px; an own one may carry a cross.
pub fn chip<'a>(
    label: &str,
    kind: ChipKind,
    on_remove: Option<Message>,
    palette: &Palette,
) -> Element<'a, Message> {
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

/// A piece of a path or a file name, 24 px, with a cross where it can go.
pub fn piece<'a>(
    label: &str,
    on_remove: Option<Message>,
    palette: &Palette,
) -> Element<'a, Message> {
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
pub fn pieces<'a>(
    items: Vec<Element<'a, Message>>,
    separator: &str,
    add: Option<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
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

// ---------------------------------------------------------------------
// Lists and content
// ---------------------------------------------------------------------

fn list_row_shell<'a>(
    content: Element<'a, Message>,
    height: f32,
    selected: bool,
    on_press: Option<Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    let marker = if selected {
        palette.success
    } else {
        Color::TRANSPARENT
    };
    column![
        button(row![bar(marker), content])
            .on_press_maybe(on_press)
            .width(Length::Fill)
            .height(height - SEPARATOR_HEIGHT)
            .padding(0.0)
            .style(row_button(palette, selected)),
        theme::horizontal_rule(palette.border_inactive),
    ]
    .into()
}

/// A row of 28 px.
pub fn list_row_one<'a>(
    content: Element<'a, Message>,
    right: Option<String>,
    indent: bool,
    selected: bool,
    on_press: Option<Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut line = Row::new()
        .push(container(content).width(Length::Fill))
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center);
    if let Some(figure) = right {
        line = line.push(small(figure, palette.dim_text));
    }
    let left = if indent { GAP_L + GAP_M } else { GAP_M } - SELECTION_BAR;
    let padded = container(line)
        .padding(Padding {
            left,
            right: GAP_M,
            ..Padding::ZERO
        })
        .width(Length::Fill)
        .center_y(Length::Fill)
        .into();
    list_row_shell(padded, ROW_ONE_LINE, selected, on_press, palette)
}

/// [`detail`] for forms: no wider than `FORM_WIDTH`.
pub fn form_column<'a>(children: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    scrollable(
        container(
            Column::with_children(children)
                .spacing(GAP_S)
                .width(Length::Fill),
        )
        .max_width(FORM_WIDTH)
        .padding([GAP_M, GAP_L]),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// What a page says when it has nothing to show.
pub fn empty_state<'a>(
    title: &str,
    sentence: &str,
    action: Option<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut content = Column::new()
        .push(body(title.to_owned(), palette.text))
        .push(container(hint(sentence.to_owned(), palette)).max_width(EMPTY_TEXT_WIDTH))
        .spacing(GAP_S)
        .align_x(alignment::Horizontal::Center);
    if let Some(button) = action {
        content = content.push(button);
    }
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::HermesPage;

    #[test]
    fn a_menu_row_shows_the_key_behind_the_label() {
        let item = MenuItem {
            label: "Pause".to_owned(),
            key_hint: Some("P".to_owned()),
            message: Message::HermesPage(HermesPage::Overview),
        };
        assert_eq!(menu_row_text(&item), "Pause    P");
        assert_eq!(
            menu_row_text(&MenuItem::new(
                "Open",
                Message::HermesPage(HermesPage::Review)
            )),
            "Open"
        );
    }

    #[test]
    fn entries_with_the_same_label_stay_apart() {
        let same = |page| MenuItem::new("Same", Message::HermesPage(page));
        let found = entries(&[same(HermesPage::Overview), same(HermesPage::Review)]);
        assert_ne!(found.first(), found.get(1));
    }
}
