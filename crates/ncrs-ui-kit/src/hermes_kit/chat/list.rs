//! The list of chats beside the conversation: the button for a new chat, the
//! search field, the chats in groups and, under the rows, the footer for what
//! is marked.

use iced::widget::{container, mouse_area, scrollable, text_input, Column, Row, Space};
use iced::{alignment, Element, Length};

use crate::palette::Palette;
use crate::popup_menu::{menu_entry_colors, menu_item, PopupEntry};
use crate::theme::{self, font_size, spacing};

use super::*;

pub const CHAT_LIST_WIDTH: f32 = 300.0;
const MARK: &str = "\u{25b8} ";

/// One chat of the list.
#[derive(Debug)]
pub struct ChatListRow<M> {
    pub title: String,
    /// The second line: the status and when the chat was last used.
    pub detail: String,
    pub active: bool,
    pub marked: bool,
    pub on_press: M,
    pub on_right_press: M,
    /// The title is being edited: the row is an input instead.
    pub rename: Option<ChatListRename<M>>,
    /// The entries of the menu opened on this row, shown under it.
    pub menu: Vec<PopupEntry<M>>,
}

#[derive(Debug)]
pub struct ChatListRename<M> {
    pub draft: String,
    pub id: &'static str,
    pub on_input: fn(String) -> M,
    pub on_submit: M,
}

#[derive(Debug)]
pub struct ChatListGroup<M> {
    pub heading: String,
    pub rows: Vec<ChatListRow<M>>,
}

/// What stands under the rows while something is marked or waits for a yes.
#[derive(Debug)]
pub enum ChatListFooter<'a, M> {
    Marked {
        text: String,
        delete_label: &'a str,
        on_delete: M,
    },
    Ask {
        text: String,
        yes_label: &'a str,
        on_yes: M,
        no_label: &'a str,
        on_no: M,
    },
}

#[derive(Debug)]
pub struct ChatListProps<'a, M> {
    pub new_label: &'a str,
    pub new_key: String,
    pub on_new: M,
    pub search_placeholder: &'a str,
    pub search: String,
    pub search_id: &'static str,
    pub on_search: fn(String) -> M,
    /// Pressing Enter in the search field.
    pub on_search_submit: M,
    pub groups: Vec<ChatListGroup<M>>,
    /// Shown instead of the groups when there is no chat to list.
    pub empty_note: &'a str,
    pub footer: Option<ChatListFooter<'a, M>>,
}

fn row_of<'a, M: Clone + 'a>(row: ChatListRow<M>, palette: &Palette) -> Element<'a, M> {
    let ChatListRow {
        title,
        detail,
        active,
        marked,
        on_press,
        on_right_press,
        rename,
        menu,
    } = row;
    let shown: Element<'a, M> = match rename {
        Some(edit) => {
            let input = text_input("", &edit.draft)
                .id(edit.id)
                .size(font_size::ROW)
                .padding([spacing::XS, spacing::XS])
                .width(Length::Fill)
                .style(theme::search_input(palette))
                .on_input(edit.on_input)
                .on_submit(edit.on_submit);
            container(input)
                .padding([spacing::M, GAP_M])
                .height(ROW_TWO_LINE)
                .center_y(ROW_TWO_LINE)
                .into()
        }
        None => {
            let primary = if marked {
                format!("{MARK}{title}")
            } else {
                title
            };
            mouse_area(list_row_two(
                RowTwo {
                    primary,
                    secondary: detail,
                    right: None,
                    lead: None,
                    muted: false,
                },
                active,
                Some(on_press),
                palette,
            ))
            .on_right_press(on_right_press)
            .into()
        }
    };
    if menu.is_empty() {
        return shown;
    }
    let entries = menu.into_iter().map(|entry| {
        let (label_color, _) = menu_entry_colors(palette, entry.on_press.is_some(), false);
        menu_item(
            body(entry.label, label_color),
            false,
            entry.on_press,
            None,
            palette,
        )
    });
    let menu_box = container(Column::with_children(entries))
        .padding(spacing::XS)
        .width(Length::Fill)
        .style(theme::menu(palette));
    Column::new()
        .push(shown)
        .push(container(menu_box).padding([spacing::XS, GAP_S]))
        .into()
}

fn footer_of<'a, M: Clone + 'a>(
    footer: ChatListFooter<'a, M>,
    palette: &Palette,
) -> Element<'a, M> {
    let line = match footer {
        ChatListFooter::Marked {
            text,
            delete_label,
            on_delete,
        } => Row::new()
            .push(small(text, palette.dim_text))
            .push(Space::new().width(Length::Fill))
            .push(secondary(delete_label, Some(on_delete), palette)),
        ChatListFooter::Ask {
            text,
            yes_label,
            on_yes,
            no_label,
            on_no,
        } => Row::new()
            .push(small(text, palette.dim_text))
            .push(Space::new().width(Length::Fill))
            .push(secondary(no_label, Some(on_no), palette))
            .push(primary(yes_label, None, Some(on_yes), palette))
            .spacing(GAP_S),
    };
    column_with_rule(
        container(line.align_y(alignment::Vertical::Center))
            .padding(GAP_S)
            .width(Length::Fill)
            .into(),
        palette,
    )
}

fn column_with_rule<'a, M: 'a>(content: Element<'a, M>, palette: &Palette) -> Element<'a, M> {
    Column::new()
        .push(theme::horizontal_rule(palette.border_inactive))
        .push(content)
        .into()
}

/// The list at the left of the chat, `CHAT_LIST_WIDTH` wide.
pub fn chat_list<'a, M: Clone + 'a>(
    props: ChatListProps<'a, M>,
    palette: &Palette,
) -> Element<'a, M> {
    let top = container(
        Column::new()
            .push(primary(
                props.new_label,
                Some(&props.new_key),
                Some(props.on_new),
                palette,
            ))
            .push(text_field(
                props.search_placeholder,
                &props.search,
                props.search_id,
                Some(props.on_search),
                props.on_search_submit,
                palette,
            ))
            .spacing(GAP_S),
    )
    .padding(GAP_S + GAP_XS)
    .width(Length::Fill);
    let mut rows = Column::new().width(Length::Fill);
    if props.groups.is_empty() {
        rows = rows
            .push(container(small(props.empty_note.to_owned(), palette.dim_text)).padding(GAP_M));
    }
    for group in props.groups {
        rows = rows.push(
            container(small(group.heading.to_owned(), palette.dim_text)).padding([GAP_XS, GAP_M]),
        );
        for row in group.rows {
            rows = rows.push(row_of(row, palette));
        }
    }
    let mut page = Column::new()
        .push(top)
        .push(theme::horizontal_rule(palette.border_inactive))
        .push(scrollable(rows).width(Length::Fill).height(Length::Fill));
    if let Some(footer) = props.footer {
        page = page.push(footer_of(footer, palette));
    }
    Row::new()
        .push(
            container(page)
                .width(CHAT_LIST_WIDTH)
                .height(Length::Fill)
                .style(theme::panel(palette)),
        )
        .push(theme::vertical_rule(palette.border_inactive))
        .into()
}
