//! The filing tab: two halves side by side, the bar below.

use std::borrow::Cow;

use iced::widget::{column, container, row, text};
use iced::{Element, Length};
use ncrs_ui_kit::kit::error_line;
use ncrs_ui_kit::theme;

use crate::ctx::ViewCtx;
use crate::footer::footer;
use crate::left::left;
use crate::message::Message;
use crate::right::right;
use crate::state::State;

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    let Some(session) = state.session.as_ref() else {
        return text("").into();
    };
    let plan = session.shown_ref().unwrap_or_else(|| {
        Cow::Owned(ncrs_filing::chat_work::LivePlan {
            source_root: session.source.clone(),
            target_root: session.target.clone(),
            files: Vec::new(),
            done: false,
            folder_choice_unsupported: false,
        })
    });
    let half = ctx.window.width / 2.0;
    let halves = row![
        container(left(session, &plan, half, ctx.lang, ctx.palette)).width(Length::FillPortion(1)),
        theme::vertical_rule(ctx.palette.border),
        container(right(session, &plan, half, ctx.lang, ctx.palette)).width(Length::FillPortion(1)),
    ]
    .height(Length::Fill);
    let mut page = column![halves];
    if let Some(reason) = &session.error {
        page = page.push(error_line(reason.clone(), ctx.palette));
    }
    page.push(footer(session, &plan, ctx.lang, ctx.palette))
        .into()
}
