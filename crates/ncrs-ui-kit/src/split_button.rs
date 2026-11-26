//! A toolbar button with a small arrow beside it: the button does the usual,
//! the arrow opens a menu of the other ways.

use iced::widget::text::Wrapping;
use iced::widget::{container, row, text, Column};
use iced::{alignment, Element, Length};

use crate::kit::{toolbar_button, Tone};
use crate::layout::MENU_PADDING;
use crate::list_below::list_below_own_width;
use crate::palette::Palette;
use crate::popup_menu::{menu_entry_colors, menu_item, menu_separator, MenuRow, PopupEntry};
use crate::theme::{self, font_size, spacing};

/// The label of the arrow half.
const ARROW: &str = "\u{25be}";

/// Room for a name and, beside it, why it cannot be chosen.
const MENU_WIDTH: f32 = 360.0;

/// The rows of the menu under the arrow, as a framed list.
fn menu_list<'a, M: Clone + 'a>(rows: Vec<MenuRow<M>>, palette: &Palette) -> Element<'a, M> {
    let items = rows.into_iter().map(|line| match line {
        MenuRow::Separator => menu_separator(palette),
        MenuRow::Entry(entry) | MenuRow::Danger(entry) | MenuRow::Submenu(entry, _) => {
            entry_row(entry, palette)
        }
    });
    container(Column::with_children(items))
        .padding(MENU_PADDING)
        .width(MENU_WIDTH)
        .style(theme::menu(palette))
        .into()
}

fn entry_row<'a, M: Clone + 'a>(entry: PopupEntry<M>, palette: &Palette) -> Element<'a, M> {
    let (label_color, key_color) =
        menu_entry_colors(palette, entry.on_press.is_some(), entry.is_selected);
    let content = row![
        text(entry.label)
            .size(font_size::ROW)
            .color(label_color)
            .width(Length::Fill)
            .wrapping(Wrapping::None),
        text(entry.key.unwrap_or_default())
            .size(font_size::STATUS)
            .color(key_color)
            .wrapping(Wrapping::None),
    ]
    .align_y(alignment::Vertical::Center);
    menu_item(
        content,
        entry.is_selected,
        entry.on_press,
        entry.on_hover,
        palette,
    )
}

/// The button `main` and the arrow beside it, with `tip` and `tone`.
/// `menu` is the rows to show under them while the menu is open; `None` keeps
/// it closed. A click outside the list is for the caller to catch: the list
/// itself takes no outside clicks.
pub fn split_button<'a, M: Clone + 'a>(
    main: Element<'a, M>,
    tip: &str,
    on_arrow: Option<M>,
    menu: Option<Vec<MenuRow<M>>>,
    tone: Tone,
    palette: &Palette,
) -> Element<'a, M> {
    let halves = row![
        main,
        toolbar_button(ARROW, None, tip, on_arrow, tone, palette),
    ]
    .spacing(spacing::BORDER_WIDTH);
    match menu {
        Some(rows) => list_below_own_width(halves.into(), menu_list(rows, palette)),
        None => halves.into(),
    }
}
