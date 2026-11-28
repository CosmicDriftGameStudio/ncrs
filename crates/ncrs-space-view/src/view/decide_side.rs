//! The narrow overview beside the decide list: the groups of the tab with their rows, the
//! folder that is open marked, and the note that Hermes deletes nothing here by itself.

use iced::widget::{column, container};
use iced::{Element, Length, Padding};
use ncrs_i18n::Msg;
use ncrs_space::Group;
use ncrs_ui_kit::decide_row::{side_head, side_row, SideLook, SideRow};
use ncrs_ui_kit::format::size_shown;
use ncrs_ui_kit::kit;
use ncrs_ui_kit::page_scroll::page_scroll_padded;
use ncrs_ui_kit::theme::{self, spacing};

use super::list::group_color;
use crate::ctx::ViewCtx;
use crate::decide::{DecideArea, DecideMessage, DecideState, DECIDE_SIDE_ID};
use crate::message::Message;
use crate::rows::GROUP_ORDER;
use crate::state::State;
use crate::texts::{group_title, row_name};

pub fn view<'a>(pane: &'a DecideState, state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let biggest = state.rows().iter().map(|row| row.bytes).max().unwrap_or(0);
    let mut list = column![];
    for group in GROUP_ORDER {
        if state.group_rows(group).next().is_none() {
            continue;
        }
        list = list.push(side_head(
            &group_title(group, ctx.lang),
            size_shown(state.group_bytes(group), ctx.lang),
            palette,
        ));
        for row in state.group_rows(group) {
            let is_open = row.path == pane.folder();
            let look = if is_open {
                SideLook::Current
            } else if group == Group::InUse {
                SideLook::Dim
            } else {
                SideLook::Normal
            };
            list = list.push(side_row(
                SideRow {
                    name: row_name(row, ctx.lang),
                    size_text: size_shown(row.bytes, ctx.lang),
                    fraction: row.bytes as f32 / biggest.max(1) as f32,
                    color: group_color(group, palette),
                    look,
                    focused: pane.area() == DecideArea::Side && row.path == pane.side_cursor(),
                    on_press: Some(Message::Decide(DecideMessage::SidePressed(
                        row.path.clone(),
                    ))),
                },
                palette,
            ));
        }
    }
    column![
        container(page_scroll_padded(
            DECIDE_SIDE_ID,
            list.width(Length::Fill).into(),
            Padding::ZERO,
        ))
        .height(Length::Fill),
        theme::horizontal_rule(palette.border_inactive),
        container(kit::dim(ctx.lang.text(Msg::SpaceDecideNote), palette))
            .padding(spacing::XL)
            .width(Length::Fill),
    ]
    .into()
}
