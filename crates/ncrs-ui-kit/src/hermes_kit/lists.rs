//! Lists, rows and content areas.
use iced::widget::text::LineHeight;
use iced::widget::{
    button, column, container, mouse_area, pick_list, scrollable, Column, Row,
};
use iced::{alignment, Color, Element, Length, Padding, Pixels};

use crate::format;
use crate::kit::{self, Check};
use crate::palette::Palette;
use crate::theme;

use super::*;

// ---------------------------------------------------------------------
// Lists and content
// ---------------------------------------------------------------------

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

/// A target in a list where some are ticked.
#[derive(Debug)]
pub struct TargetCheck<'a, M> {
    pub state: Check,
    /// Toggles the target; a click anywhere on the row does.
    pub on_toggle: Option<M>,
    /// The colour of the chip; see [`target_chip`].
    pub index: usize,
    /// "T1": the number the target got when it was made.
    pub number: &'a str,
    pub name: &'a str,
    pub path: &'a str,
    /// The keyboard is on this row.
    pub marked: bool,
}

/// A row of 28 px: the box, the chip of the target, its name and its path
/// dimmed. A marked row is outlined like a row of the settings.
pub fn target_check_row<'a, M: Clone + 'a>(
    target: TargetCheck<'_, M>,
    palette: &Palette,
) -> Element<'a, M> {
    let (name_color, path_color) = if target.marked {
        (palette.cursor_text, palette.cursor_text)
    } else {
        (palette.text, palette.dim_text)
    };
    let line = Row::new()
        .push(kit::checkbox(target.state, None, palette))
        .push(target_chip(target.index, target.number, palette))
        .push(body(target.name.to_owned(), name_color))
        .push(small(target.path.to_owned(), path_color))
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center);
    let row = container(line)
        .width(Length::Fill)
        .height(ROW_ONE_LINE)
        .padding([0.0, GAP_XS])
        .align_y(alignment::Vertical::Center);
    let row = if target.marked {
        row.style(theme::focused_setting(palette))
    } else {
        row
    };
    let area = mouse_area(row);
    match target.on_toggle {
        Some(message) => area.on_press(message).into(),
        None => area.into(),
    }
}

/// A place in a target a file could go to.
#[derive(Debug)]
pub struct TargetPlace<'a, M> {
    /// The box before it and what toggles it, where several can be ticked.
    pub check: Option<(Check, M)>,
    /// The colour of the chip; see [`target_chip`].
    pub index: usize,
    /// "T1 Documents".
    pub label: &'a str,
    /// The rule and the path, cut in the middle to the room.
    pub line: &'a str,
    /// The keyboard is on this row.
    pub marked: bool,
}

/// A row of 28 px and `width`: the box, the chip of the target and where the
/// file would go. A marked row is outlined like a row of the settings.
pub fn target_place_row<'a, M: Clone + 'a>(
    place: TargetPlace<'_, M>,
    width: f32,
    palette: &Palette,
) -> Element<'a, M> {
    let ink = if place.marked {
        palette.cursor_text
    } else {
        palette.text
    };
    let chip_room = format::estimated_width(place.label, TEXT_SMALL) + 2.0 * GAP_S;
    let mut room = width - 2.0 * GAP_XS - chip_room - GAP_S;
    let mut line = Row::new()
        .spacing(GAP_S)
        .align_y(alignment::Vertical::Center);
    let mut on_toggle = None;
    if let Some((state, toggle)) = place.check {
        room -= CHECK_ROOM;
        line = line.push(kit::checkbox(state, None, palette));
        on_toggle = Some(toggle);
    }
    let line = line
        .push(target_chip(place.index, place.label, palette))
        .push(small(
            format::fit_middle(place.line, room.max(0.0), TEXT_SMALL),
            ink,
        ));
    let row = container(line)
        .width(Length::Fill)
        .height(ROW_ONE_LINE)
        .padding([0.0, GAP_XS])
        .align_y(alignment::Vertical::Center)
        .clip(true);
    let row = if place.marked {
        row.style(theme::focused_setting(palette))
    } else {
        row
    };
    let area = mouse_area(row);
    match on_toggle {
        Some(message) => area.on_press(message).into(),
        None => area.into(),
    }
}

/// The box of a [`target_place_row`] and the gap after it.
const CHECK_ROOM: f32 = 24.0;

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

/// A path in monospace; the parts that stand for a field are in the on color.
pub fn path<'a, M: 'a>(parts: Vec<(String, bool)>, palette: &Palette) -> Element<'a, M> {
    let texts = parts.into_iter().map(|(part, is_field)| {
        mono(
            part,
            TEXT_SMALL,
            if is_field {
                palette.primary_button().0
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
    sized_dropdown(
        width,
        items,
        selected,
        palette,
        [CONTROL_PAD_Y, MENU_PAD_X],
        (TEXT_BODY, BODY_LINE),
    )
}

/// [`dropdown`] the size of a settings text field, for a settings row.
pub fn field_dropdown<'a, M: Clone + 'a>(
    width: f32,
    items: &[MenuItem<M>],
    selected: usize,
    palette: &Palette,
) -> Element<'a, M> {
    sized_dropdown(
        width,
        items,
        selected,
        palette,
        [theme::spacing::XS, theme::spacing::M],
        (theme::font_size::BODY, kit::FIELD_LINE_HEIGHT),
    )
}

fn sized_dropdown<'a, M: Clone + 'a>(
    width: f32,
    items: &[MenuItem<M>],
    selected: usize,
    palette: &Palette,
    padding: [f32; 2],
    (text_size, line_height): (f32, f32),
) -> Element<'a, M> {
    let choices = entries(items);
    let chosen = choices.get(selected).cloned();
    pick_list(choices, chosen, |choice| choice.message)
        .width(width)
        .padding(padding)
        .text_size(text_size)
        .text_line_height(LineHeight::Absolute(Pixels(line_height)))
        .style(pick_style(palette, MenuLook::Icon))
        .menu_style(theme::dropdown_menu(palette))
        .into()
}

/// A line across the detail.
pub fn rule<'a, M: 'a>(palette: &Palette) -> Element<'a, M> {
    theme::horizontal_rule(palette.border_inactive)
}
