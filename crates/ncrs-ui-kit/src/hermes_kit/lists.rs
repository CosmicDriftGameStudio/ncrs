//! Lists, rows and content areas.
use iced::mouse::ScrollDelta;
use iced::widget::text::LineHeight;
use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, Column, Row, Space,
};
use iced::{alignment, Background, Border, Color, Element, Length, Padding, Pixels};

use crate::kit::{self, Check};
use crate::palette::Palette;
use crate::theme;

use super::*;

// ---------------------------------------------------------------------
// Lists and content
// ---------------------------------------------------------------------

fn list_row_shell<'a, M: Clone + 'a>(
    content: Element<'a, M>,
    height: f32,
    selected: bool,
    on_press: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
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
pub fn list_row_one<'a, M: Clone + 'a>(
    content: Element<'a, M>,
    right: Option<String>,
    indent: bool,
    selected: bool,
    on_press: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
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
#[derive(Debug)]
pub struct RowTwo<M> {
    pub primary: String,
    pub secondary: String,
    pub right: Option<String>,
    pub lead: Option<(Check, M)>,
    /// The row is switched off: its name is dimmed too.
    pub muted: bool,
}

/// A row of 48 px.
pub fn list_row_two<'a, M: Clone + 'a>(
    row_content: RowTwo<M>,
    selected: bool,
    on_press: Option<M>,
    palette: &Palette,
) -> Element<'a, M> {
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
pub fn list_pane<'a, M: Clone + 'a>(
    rows: Vec<Element<'a, M>>,
    on_scroll: impl Fn(ScrollDelta) -> M + 'a,
    palette: &Palette,
) -> Element<'a, M> {
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
pub fn detail<'a, M: Clone + 'a>(children: Vec<Element<'a, M>>) -> Element<'a, M> {
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
pub fn list_and_detail<'a, M: Clone + 'a>(
    list: Element<'a, M>,
    detail_part: Element<'a, M>,
) -> Element<'a, M> {
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
pub fn thumbnail<'a, M: Clone + 'a>(on_press: Option<M>, palette: &Palette) -> Element<'a, M> {
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
pub fn icon_button<'a, M: Clone + 'a>(
    label: &str,
    on_press: M,
    palette: &Palette,
) -> Element<'a, M> {
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
pub fn dropdown<'a, M: Clone + 'a>(
    width: f32,
    items: &[MenuItem<M>],
    selected: usize,
    palette: &Palette,
) -> Element<'a, M> {
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
pub fn form_column<'a, M: Clone + 'a>(children: Vec<Element<'a, M>>) -> Element<'a, M> {
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
pub fn centered_note<'a, M: Clone + 'a>(content: Element<'a, M>) -> Element<'a, M> {
    container(container(content).max_width(FORM_WIDTH))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

/// What a page says when it has nothing to show.
pub fn empty_state<'a, M: Clone + 'a>(
    title: &str,
    sentence: &str,
    action: Option<Element<'a, M>>,
    palette: &Palette,
) -> Element<'a, M> {
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
