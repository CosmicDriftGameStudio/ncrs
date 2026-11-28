//! The decide pane: the items of Downloads or of a big folder by reason, and what goes to the
//! trash. An overview of the tab on the left, the list in the middle, the item and the
//! selection on the right.

use iced::widget::{column, container, row};
use iced::{Element, Length};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::format::{shown_path, size_shown, time};
use ncrs_ui_kit::theme::{self, size};

use super::{decide_detail, decide_foot, decide_head, decide_list, decide_side};
use crate::ctx::ViewCtx;
use crate::decide::DecideState;
use crate::message::Message;
use crate::state::State;

pub fn view<'a>(pane: &'a DecideState, state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let summary = fill_template(
        ctx.lang.text(Msg::SpaceDecideMeasured),
        &[
            ("{folder}", &shown_path(pane.folder(), ctx.home)),
            ("{size}", &size_shown(pane.total_bytes(), ctx.lang)),
            ("{time}", &time(state.scan().map(|scan| scan.measured_at))),
        ],
    );
    let middle = column![
        decide_list::folder_line(pane, summary, ctx),
        theme::horizontal_rule(palette.border_inactive),
        decide_list::hermes(pane, ctx),
        decide_list::list(pane, ctx),
        decide_foot::view(pane, state, ctx),
    ]
    .width(Length::Fill)
    .height(Length::Fill);
    column![
        decide_head::view(pane, state, ctx),
        theme::horizontal_rule(palette.border_inactive),
        row![
            container(decide_side::view(pane, state, ctx))
                .width(size::SIDE_PANE)
                .height(Length::Fill),
            theme::vertical_rule(palette.border_inactive),
            middle,
            theme::vertical_rule(palette.border_inactive),
            container(decide_detail::view(pane, ctx))
                .width(size::DETAIL_PANE)
                .height(Length::Fill),
        ]
        .height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}
