//! A sidebar list of pages in headed groups: the group head in small dim
//! capitals, the pages under it, a divider between the groups.

use iced::widget::text::Wrapping;
use iced::widget::{button, container, row, text, Column};
use iced::{alignment, Element, Length, Padding};

use crate::kit;
use crate::palette::Palette;
use crate::theme::{self, font_size, size, spacing};

const ITEM_GAP: f32 = 2.0;

#[derive(Debug)]
pub struct NavItem<M> {
    pub label: String,
    /// A dim word after the label.
    pub suffix: Option<String>,
    pub chosen: bool,
    /// The chosen page while the keyboard is on its rows.
    pub dimmed: bool,
    pub on_press: M,
}

#[derive(Debug)]
pub struct NavGroup<M> {
    pub head: String,
    /// A dim word after the head, in the case it is given in.
    pub mark: Option<String>,
    pub items: Vec<NavItem<M>>,
}

/// The groups in order; a group without items is left out, and so is the
/// divider that would have stood above it.
pub fn nav_list<'a, M: Clone + 'a>(groups: Vec<NavGroup<M>>, palette: &Palette) -> Element<'a, M> {
    let mut list = Column::new();
    let mut first = true;
    for group in groups.into_iter().filter(|group| !group.items.is_empty()) {
        if !first {
            list = list.push(container("").height(spacing::L));
            list = list.push(kit::divider(palette));
            list = list.push(container("").height(spacing::L));
        }
        first = false;
        list = list.push(head(&group, palette));
        let mut items = Column::new().spacing(ITEM_GAP);
        for item in group.items {
            items = items.push(item_button(item, palette));
        }
        list = list.push(container(items).padding(Padding {
            top: ITEM_GAP,
            ..Padding::ZERO
        }));
    }
    list.into()
}

fn head<'a, M: 'a>(group: &NavGroup<M>, palette: &Palette) -> Element<'a, M> {
    let dim = |content: String| {
        text(content)
            .size(font_size::SMALL)
            .color(palette.dim_text)
            .wrapping(Wrapping::None)
    };
    let mut line = row![dim(group.head.to_uppercase())].spacing(spacing::XS);
    if let Some(mark) = &group.mark {
        line = line.push(dim(mark.clone()));
    }
    container(line)
        .padding(Padding {
            left: spacing::CELL_PADDING_X,
            bottom: spacing::XS,
            ..Padding::ZERO
        })
        .into()
}

fn item_button<'a, M: Clone + 'a>(item: NavItem<M>, palette: &Palette) -> Element<'a, M> {
    // On the cursor fill the label takes the cursor's ink.
    let ink = if item.chosen && !item.dimmed {
        palette.cursor_text
    } else {
        palette.text
    };
    let mut label_row = row![text(item.label)
        .size(font_size::BODY)
        .color(ink)
        .wrapping(Wrapping::None)]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    if let Some(suffix) = item.suffix {
        label_row = label_row.push(
            text(suffix)
                .size(font_size::SMALL)
                .color(if item.chosen && !item.dimmed {
                    palette.cursor_text
                } else {
                    palette.dim_text
                })
                .wrapping(Wrapping::None),
        );
    }
    let content = container(label_row)
        .padding(Padding {
            left: spacing::CELL_PADDING_X,
            ..Padding::ZERO
        })
        .center_y(Length::Fill);
    button(content)
        .on_press(item.on_press)
        .width(Length::Fill)
        .height(size::SIDEBAR_ITEM)
        .padding(0.0)
        .style(theme::feature_item(palette, item.chosen, item.dimmed))
        .into()
}
