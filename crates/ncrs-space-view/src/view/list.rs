//! The groups and their rows, the empty note, and the detail pane beside them.

use iced::widget::{column, container, mouse_area, row};
use iced::{Color, Element, Length, Padding};
use ncrs_i18n::Msg;
use ncrs_space::Group;
use ncrs_ui_kit::format::{shown_path, size_shown};
use ncrs_ui_kit::kit::Check;
use ncrs_ui_kit::page_scroll::page_scroll_tracked_padded;
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::row_scroll::FOCUSED_ROW_ID;
use ncrs_ui_kit::section_head::{section_head_with, SectionHead};
use ncrs_ui_kit::size_row::{size_row, SizeRow};
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::rows::{matches_filter, DisplayRow, GROUP_ORDER};
use crate::state::{CursorKey, State};
use crate::texts::{group_title, row_name, row_reason};
use crate::update::LIST_ID;

pub fn group_color(group: Group, palette: &Palette) -> Color {
    match group {
        Group::Safe => theme::space::safe(palette),
        Group::Worktrees => theme::space::worktrees(),
        Group::Decide => theme::space::decide(),
        Group::InUse => theme::space::in_use(palette),
    }
}

fn one_row<'a>(
    row: &'a DisplayRow,
    biggest: u64,
    state: &'a State,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let is_cursor = state.cursor == Some(CursorKey::Row(row.path.clone()));
    let check = row.is_deletable().then(|| {
        if state.is_ticked(row) {
            Check::On
        } else {
            Check::Off
        }
    });
    let built = size_row(
        SizeRow {
            check,
            name: row_name(row, ctx.lang),
            path: shown_path(&row.path, ctx.home),
            size_text: size_shown(row.bytes, ctx.lang),
            fraction: row.bytes as f32 / biggest.max(1) as f32,
            color: group_color(row.group, ctx.palette),
            reason: row_reason(row, ctx.lang),
            outcome: None,
            cursor: is_cursor,
            on_press: Some(Message::RowPressed(row.path.clone())),
            on_toggle: Some(Message::RowToggled(row.path.clone())),
        },
        ctx.palette,
    );
    let framed = if is_cursor {
        container(built).id(FOCUSED_ROW_ID).into()
    } else {
        built
    };
    mouse_area(framed)
        .on_right_press(Message::OpenMenuAt(row.path.clone()))
        .on_double_click(Message::Enter)
        .into()
}

fn group_view<'a>(
    group: Group,
    biggest: u64,
    state: &'a State,
    ctx: ViewCtx<'a>,
) -> Option<Element<'a, Message>> {
    let rows: Vec<&DisplayRow> = state
        .group_rows(group)
        .filter(|row| matches_filter(row, state.filter()))
        .collect();
    if rows.is_empty() {
        return None;
    }
    let title = group_title(group, ctx.lang);
    let bytes: u64 = rows
        .iter()
        .fold(0, |sum, row| sum.saturating_add(row.bytes));
    let total = page_line(
        size_shown(bytes, ctx.lang),
        font_size::SMALL,
        ctx.palette.dim_text,
    );
    let head = section_head_with(
        SectionHead {
            title: &title,
            count: rows.len(),
            open: !state.is_folded(group),
            has_focus: state.cursor == Some(CursorKey::Head(group)),
            on_toggle: Message::HeadToggled(group),
        },
        vec![total],
        ctx.palette,
    );
    let mut body = column![head];
    if !state.is_folded(group) {
        for row in rows {
            body = body.push(one_row(row, biggest, state, ctx));
        }
    }
    Some(body.into())
}

fn groups<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let biggest = state.rows().iter().map(|row| row.bytes).max().unwrap_or(0);
    let shown: Vec<Element<'a, Message>> = GROUP_ORDER
        .iter()
        .filter_map(|group| group_view(*group, biggest, state, ctx))
        .collect();
    if shown.is_empty() {
        let note = if state.is_measuring() || state.scan().is_none() {
            String::new()
        } else {
            ctx.lang.text(Msg::SpaceEmpty).to_owned()
        };
        return container(page_line(note, font_size::BODY, ctx.palette.dim_text))
            .padding(spacing::XXL)
            .into();
    }
    column(shown).width(Length::Fill).into()
}

/// The scrolling groups and the detail pane of the row under the cursor.
pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let scroll = page_scroll_tracked_padded(
        LIST_ID,
        groups(state, ctx),
        Padding::from([0.0, spacing::XL]),
        Message::ListScrolled,
    );
    row![
        container(scroll).width(Length::Fill).height(Length::Fill),
        container(super::detail::view(state, ctx))
            .width(size::DETAIL_PANE)
            .height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}
