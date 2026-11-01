//! The building blocks of the filing pages. Every size of the pages lives
//! here; the pages put the blocks together and never pick a size themselves.
//! The look follows the boards: a navigation, a page header of 72 px, the
//! content, an action bar of 48 px and a status bar of 22 px.

use iced::font::Weight;
use iced::mouse::ScrollDelta;
use iced::widget::text::LineHeight;
use iced::widget::text::Wrapping;
use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, stack, text, text_input,
    Column, Row, Space,
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
pub const ROW_TWO_LINE: f32 = 48.0;
pub const LIST_WIDTH: f32 = 360.0;
pub const THUMBNAIL_WIDTH: f32 = 48.0;
pub const THUMBNAIL_HEIGHT: f32 = 64.0;
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
const FILTER_ARROW_ROOM: f32 = 20.0;
const FILTER_BORDER: f32 = 1.0;
const FILTER_CLEAR_ROOM: f32 = 16.0;
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
        Some(sentence) => small(sentence, palette.dim_text),
        None => Space::new().into(),
    };
    action_bar_with(note_line, buttons, palette)
}

/// [`action_bar`] with anything at the left, like a sentence and a link.
pub fn action_bar_with<'a>(
    left: Element<'a, Message>,
    buttons: Vec<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
    let content = row![
        container(left).width(Length::Fill).clip(true),
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
    /// A button of the page header that shows its caption and opens the list.
    Button,
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
        MenuLook::Button => (widest + 2.0 * MENU_PAD_X, MENU_PAD_X, CONTROL_PAD_Y),
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

/// [`text_field`] in the monospace font.
pub fn mono_text_field<'a>(
    placeholder: &str,
    value: &str,
    id: &'static str,
    on_input: Option<impl Fn(String) -> Message + 'a>,
    on_submit: Message,
    palette: &Palette,
) -> Element<'a, Message> {
    field_in(placeholder, value, id, on_input, on_submit, true, palette)
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

fn filter_caption(label: &str, chosen: Option<&str>) -> String {
    match chosen {
        Some(value) => format!("{label}: {value}"),
        None => label.to_owned(),
    }
}

/// The widest of the caption and the entries of the menu.
fn filter_content_width(label: &str, chosen: Option<&str>, items: &[MenuItem]) -> f32 {
    items
        .iter()
        .map(|item| format::estimated_width(&menu_row_text(item), TEXT_BODY))
        .chain([format::estimated_width(
            &filter_caption(label, chosen),
            TEXT_BODY,
        )])
        .fold(0.0, f32::max)
}

/// How wide `filter` is, for fitting filters into a line.
pub fn filter_width(label: &str, chosen: Option<&str>, items: &[MenuItem]) -> f32 {
    let room = if chosen.is_some() {
        FILTER_CLEAR_ROOM + 2.0 * FILTER_BORDER
    } else {
        FILTER_ARROW_ROOM
    };
    filter_content_width(label, chosen, items) + 2.0 * MENU_PAD_X + room
}

/// A filter of 28 px: its name with an arrow while nothing is chosen, dimmed
/// and without a menu while there is nothing to choose from. With a value
/// chosen it is framed in the success colour on a quiet fill: the name
/// dimmed, the value, and inside the frame a cross that clears it. `items`
/// are the values to choose from.
pub fn filter<'a>(
    label: &str,
    chosen: Option<&str>,
    items: &[MenuItem],
    on_clear: Message,
    palette: &Palette,
) -> Element<'a, Message> {
    let choices = entries(items);
    let caption = filter_caption(label, chosen);
    let widest = filter_content_width(label, chosen, items);
    let colors = *palette;
    let Some(value) = chosen else {
        if choices.is_empty() {
            return filter_without_values(label, widest, palette);
        }
        let base = theme::dropdown_field(palette);
        return pick_list(choices, None::<Entry>, |choice| choice.message)
            .placeholder(caption)
            .width(Length::Fixed(widest + 2.0 * MENU_PAD_X + FILTER_ARROW_ROOM))
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
    let width = widest + 2.0 * MENU_PAD_X;
    let shown = container(
        Row::new()
            .push(body(format!("{label}:"), colors.dim_text))
            .push(body(value.to_owned(), colors.text))
            .spacing(GAP_XS),
    )
    .padding([inner_pad_y, MENU_PAD_X])
    .width(width);
    let opener = pick_list(choices, None::<Entry>, |choice| choice.message)
        .handle(pick_list::Handle::None)
        .width(Length::Fixed(width))
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
            .push(stack![shown, opener])
            .push(remove_button(on_clear, colors.dim_text, palette))
            .into(),
        palette,
    )
}

/// A chosen value in its frame: the success colour on a quiet fill.
fn chosen_frame<'a>(content: Element<'a, Message>, palette: &Palette) -> Element<'a, Message> {
    let colors = *palette;
    container(content)
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
pub fn criterion<'a>(label: &str, value: &str, palette: &Palette) -> Element<'a, Message> {
    chosen_frame(
        container(
            Row::new()
                .push(body(format!("{label}:"), palette.dim_text))
                .push(body(value.to_owned(), palette.text))
                .spacing(GAP_XS),
        )
        .padding([CONTROL_PAD_Y - FILTER_BORDER, MENU_PAD_X])
        .into(),
        palette,
    )
}

/// The filter of a facet the documents have no value for: the name and the
/// arrow of an open one, dimmed, and nothing to open.
fn filter_without_values<'a>(label: &str, widest: f32, palette: &Palette) -> Element<'a, Message> {
    let colors = *palette;
    container(
        Row::new()
            .push(body(label.to_owned(), colors.dim_text))
            .push(Space::new().width(Length::Fill))
            .push(body("▾".to_owned(), colors.dim_text)),
    )
    .padding([CONTROL_PAD_Y - FILTER_BORDER, MENU_PAD_X])
    .width(widest + 2.0 * MENU_PAD_X + FILTER_ARROW_ROOM)
    .height(CONTROL_HEIGHT)
    .style(move |_| container::Style {
        border: Border {
            color: colors.border.scale_alpha(DISABLED_OPACITY),
            width: FILTER_BORDER,
            radius: theme::radius::CONTROL.into(),
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

/// What a row of two lines shows: the name, a dimmed second line, a figure at
/// the right and, where the row can be checked, the box before it.
pub struct RowTwo {
    pub primary: String,
    pub secondary: String,
    pub right: Option<String>,
    pub lead: Option<(Check, Message)>,
    /// The row is switched off: its name is dimmed too.
    pub muted: bool,
}

/// A row of 48 px.
pub fn list_row_two<'a>(
    row_content: RowTwo,
    selected: bool,
    on_press: Option<Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    let marker = if selected {
        palette.success
    } else {
        Color::TRANSPARENT
    };
    let name_color = if row_content.muted {
        palette.dim_text
    } else {
        palette.text
    };
    let lines = Column::new()
        .push(body(row_content.primary, name_color))
        .push(small(row_content.secondary, palette.dim_text));
    let mut line = Row::new()
        .push(container(lines).width(Length::Fill).clip(true))
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center);
    if let Some(figure) = row_content.right {
        line = line.push(small(figure, palette.dim_text));
    }
    let text_cell = button(
        container(line)
            .padding(Padding {
                left: if row_content.lead.is_some() {
                    0.0
                } else {
                    GAP_M - SELECTION_BAR
                },
                right: GAP_M,
                ..Padding::ZERO
            })
            .width(Length::Fill)
            .center_y(Length::Fill),
    )
    .on_press_maybe(on_press)
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(0.0)
    .style(row_button(palette, false));
    let mut cells = Row::new().push(bar(marker));
    if let Some((state, toggle)) = row_content.lead {
        cells = cells.push(
            mouse_area(
                container(kit::checkbox(state, None, palette))
                    .padding(Padding {
                        left: GAP_M - SELECTION_BAR,
                        right: GAP_S,
                        ..Padding::ZERO
                    })
                    .height(Length::Fill)
                    .center_y(Length::Fill),
            )
            .on_press(toggle),
        );
    }
    let fill = if selected {
        palette.title
    } else {
        Color::TRANSPARENT
    };
    column![
        container(cells.push(text_cell))
            .height(ROW_TWO_LINE - SEPARATOR_HEIGHT)
            .style(fill_style(fill)),
        theme::horizontal_rule(palette.border_inactive),
    ]
    .into()
}

/// The list left of a detail: the rows, 360 px wide, a line at the right. The
/// wheel over it sends `on_scroll`.
pub fn list_pane<'a>(
    rows: Vec<Element<'a, Message>>,
    on_scroll: impl Fn(ScrollDelta) -> Message + 'a,
    palette: &Palette,
) -> Element<'a, Message> {
    row![
        mouse_area(
            container(Column::with_children(rows))
                .width(LIST_WIDTH)
                .height(Length::Fill)
                .clip(true),
        )
        .on_scroll(on_scroll),
        theme::vertical_rule(palette.border_inactive),
    ]
    .into()
}

/// The part right of a list: padding of 16 and 24, scrolling when it is long.
pub fn detail<'a>(children: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    scrollable(
        container(
            Column::with_children(children)
                .spacing(GAP_S)
                .width(Length::Fill),
        )
        .padding([GAP_M, GAP_L]),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// A list and the detail of the selected entry side by side.
pub fn list_and_detail<'a>(
    list: Element<'a, Message>,
    detail_part: Element<'a, Message>,
) -> Element<'a, Message> {
    row![list, detail_part]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// A path in monospace; the parts that stand for a field are in the accent.
pub fn path<'a, M: 'a>(parts: Vec<(String, bool)>, palette: &Palette) -> Element<'a, M> {
    let texts = parts.into_iter().map(|(part, is_field)| {
        mono(
            part,
            TEXT_SMALL,
            if is_field {
                palette.accent
            } else {
                palette.text
            },
        )
    });
    container(Row::with_children(texts).align_y(alignment::Vertical::Center))
        .height(CONTROL_HEIGHT)
        .align_y(alignment::Vertical::Center)
        .clip(true)
        .into()
}

/// The page of a document as a stand-in of 48 x 64 px; a press shows the
/// preview.
pub fn thumbnail<'a>(on_press: Option<Message>, palette: &Palette) -> Element<'a, Message> {
    let colors = *palette;
    let lines = Column::with_children(
        std::iter::repeat_with(|| {
            container(Space::new())
                .width(Length::Fill)
                .height(3.0)
                .style(fill_style(colors.border_inactive))
                .into()
        })
        .take(6),
    )
    .spacing(GAP_XS);
    button(lines)
        .on_press_maybe(on_press)
        .width(THUMBNAIL_WIDTH)
        .height(THUMBNAIL_HEIGHT)
        .padding([GAP_S, GAP_S - GAP_XS / 2.0])
        .style(move |_, _| button::Style {
            background: Some(Background::Color(colors.title)),
            border: Border {
                color: colors.border_inactive,
                width: 1.0,
                radius: RADIUS.into(),
            },
            ..button::Style::default()
        })
        .into()
}

/// A square button of 28 px with a one-character label.
pub fn icon_button<'a>(label: &str, on_press: Message, palette: &Palette) -> Element<'a, Message> {
    button(
        container(label_text(label))
            .width(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .on_press(on_press)
    .width(CONTROL_HEIGHT)
    .height(CONTROL_HEIGHT)
    .padding(0.0)
    .style(button_style(palette, ButtonLook::Secondary))
    .into()
}

/// A dropdown of 28 px that shows the chosen entry. `items` hold the labels
/// and what choosing one sends.
pub fn dropdown<'a>(
    width: f32,
    items: &[MenuItem],
    selected: usize,
    palette: &Palette,
) -> Element<'a, Message> {
    let choices = entries(items);
    let chosen = choices.get(selected).cloned();
    pick_list(choices, chosen, |choice| choice.message)
        .width(width)
        .padding([CONTROL_PAD_Y, MENU_PAD_X])
        .text_size(TEXT_BODY)
        .text_line_height(LineHeight::Absolute(Pixels(BODY_LINE)))
        .style(pick_style(palette, MenuLook::Icon))
        .menu_style(theme::dropdown_menu(palette))
        .into()
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

/// A line across the detail.
pub fn rule<'a, M: 'a>(palette: &Palette) -> Element<'a, M> {
    theme::horizontal_rule(palette.border_inactive)
}

/// A note in the middle of the content, for a page with nothing to list.
pub fn centered_note<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(container(content).max_width(FORM_WIDTH))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
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
