//! The middle of the decide pane: what Hermes says, how much is chosen, the column titles and
//! the groups with their rows.

use iced::widget::{column, container, mouse_area, row};
use iced::{alignment, Element, Length, Padding};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::color_chip::color_chip;
use ncrs_ui_kit::decide_row::{
    file_columns_header, file_row, group_head, more_row, FileRow, GroupHead,
};
use ncrs_ui_kit::format::size_shown;
use ncrs_ui_kit::kit::{self, Check};
use ncrs_ui_kit::page_scroll::page_scroll_padded;
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::row_scroll::FOCUSED_ROW_ID;
use ncrs_ui_kit::stacked_bar::{stacked_bar, BarSegment};
use ncrs_ui_kit::theme::{self, font_size, spacing};
use ncrs_ui_kit::APP_FONT_BOLD;

use super::decide_text::{day, file_name, group_msg, hint_msg, reason_text};
use crate::ctx::ViewCtx;
use crate::decide::{
    DecideArea, DecideGroup, DecideItem, DecideKey, DecideMessage, DecideState, DECIDE_LIST_ID,
};
use crate::message::Message;

fn decide(message: DecideMessage) -> Message {
    Message::Decide(message)
}

/// The rows of the cursor are found again by this id to scroll them into view.
fn marked<'a>(content: Element<'a, Message>, is_cursor: bool) -> Element<'a, Message> {
    if is_cursor {
        container(content).id(FOCUSED_ROW_ID).into()
    } else {
        content
    }
}

/// What Hermes says about the folder, and the bar of what is chosen and what is not.
pub fn hermes<'a>(pane: &'a DecideState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let chosen = pane.chosen_bytes();
    let rest = pane.total_bytes().saturating_sub(chosen);
    let bar = stacked_bar(
        &[
            BarSegment {
                portion: chosen,
                color: theme::space::decide(),
            },
            BarSegment {
                portion: rest,
                color: theme::space::rest(palette),
            },
        ],
        palette,
    );
    let legend = |msg: Msg, bytes: u64, color| {
        color_chip(
            &fill_template(
                ctx.lang.text(msg),
                &[("{size}", &size_shown(bytes, ctx.lang))],
            ),
            color,
            None::<Message>,
        )
    };
    let mut block = column![].spacing(spacing::M);
    if pane.is_downloads() {
        block = block
            .push(
                iced::widget::text(ctx.lang.text(Msg::SpaceDecideHermesLabel).to_uppercase())
                    .size(font_size::SMALL)
                    .font(APP_FONT_BOLD)
                    .color(theme::git::purple(palette)),
            )
            .push(kit::paragraph(
                ctx.lang.text(Msg::SpaceDecideHermesDownloads),
                palette,
            ));
    }
    block = block.push(bar).push(
        row![
            legend(Msg::SpaceDecideChosen, chosen, theme::space::decide()),
            legend(Msg::SpaceDecideNotChosen, rest, palette.dim_text),
        ]
        .spacing(spacing::XS),
    );
    container(block)
        .padding([spacing::M, spacing::XXL])
        .width(Length::Fill)
        .into()
}

fn item_row<'a>(
    entry: &'a DecideItem,
    biggest: u64,
    pane: &'a DecideState,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let path = &entry.item.path;
    let is_cursor = pane.cursor == Some(DecideKey::Item(path.clone()));
    let is_chosen = pane.is_chosen(path);
    let built = file_row(
        FileRow {
            check: Some(if is_chosen { Check::On } else { Check::Off }),
            name: file_name(path),
            reason: reason_text(&entry.reason, ctx),
            date: day(entry.item.modified),
            size_text: size_shown(entry.item.size, ctx.lang),
            fraction: entry.item.size as f32 / biggest.max(1) as f32,
            color: theme::space::decide(),
            dimmed: !is_chosen,
            cursor: is_cursor,
            focused: pane.area() == DecideArea::List,
            on_press: Some(decide(DecideMessage::RowPressed(path.clone()))),
            on_toggle: Some(decide(DecideMessage::RowToggled(path.clone()))),
        },
        ctx.palette,
    );
    mouse_area(marked(built, is_cursor))
        .on_double_click(decide(DecideMessage::RowToggled(path.clone())))
        .into()
}

fn group_view<'a>(
    group: &'a DecideGroup,
    biggest_group: u64,
    biggest_item: u64,
    pane: &'a DecideState,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let check = pane.group_check(group);
    let is_open = !pane.is_folded(group.kind);
    let head_cursor = pane.cursor == Some(DecideKey::Head(group.kind));
    let head = group_head(
        GroupHead {
            title: ctx.lang.text(group_msg(group.kind)).to_owned(),
            hint: ctx.lang.text(hint_msg(group.kind)).to_owned(),
            count: group.items.len(),
            size_text: size_shown(DecideState::group_bytes(group), ctx.lang),
            fraction: DecideState::group_bytes(group) as f32 / biggest_group.max(1) as f32,
            color: theme::space::decide(),
            open: is_open,
            check,
            dimmed: check == Check::Off,
            focused: head_cursor && pane.area() == DecideArea::List,
            on_toggle: decide(DecideMessage::HeadToggled(group.kind)),
            on_check: decide(DecideMessage::GroupToggled(group.kind)),
        },
        ctx.palette,
    );
    let mut body = column![marked(head, head_cursor)];
    if is_open {
        for entry in group.items.get(..pane.shown_len(group)).unwrap_or_default() {
            body = body.push(item_row(entry, biggest_item, pane, ctx));
        }
        let (hidden, bytes) = pane.hidden_of(group);
        if hidden > 0 {
            let at_more = pane.cursor == Some(DecideKey::More(group.kind));
            body = body.push(marked(
                more_row(
                    fill_template(
                        ctx.lang.text(Msg::SpaceDecideMore),
                        &[("{count}", &hidden.to_string())],
                    ),
                    size_shown(bytes, ctx.lang),
                    at_more,
                    at_more && pane.area() == DecideArea::List,
                    decide(DecideMessage::MoreOpened(group.kind)),
                    ctx.palette,
                ),
                at_more,
            ));
        }
    }
    body.into()
}

fn groups<'a>(pane: &'a DecideState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    if pane.groups().is_empty() {
        let note = if pane.is_loaded() {
            ctx.lang.text(Msg::SpaceEmpty)
        } else {
            ""
        };
        return container(page_line(note, font_size::BODY, ctx.palette.dim_text))
            .padding(spacing::XXL)
            .into();
    }
    let biggest_group = pane
        .groups()
        .iter()
        .map(DecideState::group_bytes)
        .max()
        .unwrap_or(0);
    let biggest_item = pane
        .groups()
        .iter()
        .flat_map(|group| group.items.iter())
        .map(|entry| entry.item.size)
        .max()
        .unwrap_or(0);
    column(
        pane.groups()
            .iter()
            .map(|group| group_view(group, biggest_group, biggest_item, pane, ctx)),
    )
    .width(Length::Fill)
    .into()
}

/// The column titles above the scrolling groups.
pub fn list<'a>(pane: &'a DecideState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    column![
        file_columns_header(
            [
                ctx.lang.text(Msg::SpaceDecideColumnName),
                ctx.lang.text(Msg::SpaceColumnReason),
                ctx.lang.text(Msg::SpaceDecideColumnModified),
                ctx.lang.text(Msg::SpaceColumnSize),
            ],
            ctx.palette,
        ),
        container(page_scroll_padded(
            DECIDE_LIST_ID,
            groups(pane, ctx),
            Padding::ZERO,
        ))
        .height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}

/// The folder as a heading line: its name, then where it is, how big and when it was measured.
pub fn folder_line<'a>(
    pane: &'a DecideState,
    summary: String,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let name = if pane.is_downloads() {
        ctx.lang.text(Msg::SpaceDecideDownloads).to_owned()
    } else {
        file_name(pane.folder())
    };
    container(
        row![
            iced::widget::text(name)
                .size(font_size::ROW)
                .font(APP_FONT_BOLD)
                .color(palette.text),
            page_line(summary, font_size::SMALL, palette.dim_text),
        ]
        .spacing(spacing::L)
        .align_y(alignment::Vertical::Center),
    )
    .padding([spacing::M, spacing::XXL])
    .width(Length::Fill)
    .into()
}
