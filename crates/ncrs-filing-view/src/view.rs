//! The filing tab: two halves side by side, the bar below, and a question over
//! them.

use iced::widget::{column, container, row, stack, text};
use iced::{Element, Length};
use ncrs_i18n::Msg;
use ncrs_ui_kit::kit::error_line;
use ncrs_ui_kit::plan_rows::plan_side_line;
use ncrs_ui_kit::theme;

use crate::ctx::ViewCtx;
use crate::footer::footer;
use crate::left::left;
use crate::message::Message;
use crate::question::question_dialog;
use crate::right::right;
use crate::run::waiting_line;
use crate::state::{Session, State};
use crate::texts::count;

/// How many files that were not filed are named; the rest is a figure.
const PROBLEMS_SHOWN: usize = 5;

/// What is wrong, one line each.
pub fn trouble(session: &Session, ctx: ViewCtx<'_>) -> Vec<String> {
    let mut lines: Vec<String> = session
        .waiting
        .iter()
        .map(|(id, reason)| waiting_line(ctx.lang, &session.target_number(id), *reason))
        .collect();
    lines.extend(session.error.clone());
    let problems = session
        .outcome
        .as_ref()
        .map_or(&[][..], |outcome| outcome.problems.as_slice());
    lines.extend(problems.iter().take(PROBLEMS_SHOWN).cloned());
    if problems.len() > PROBLEMS_SHOWN {
        lines.push(count(
            ctx.lang,
            Msg::FilingViewProblemsMore,
            problems.len() - PROBLEMS_SHOWN,
        ));
    }
    lines
}

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    let Some(session) = state.session.as_ref() else {
        return text("").into();
    };
    let half = ctx.window.width / 2.0;
    let halves = row![
        container(left(session, half, ctx)).width(Length::FillPortion(1)),
        theme::vertical_rule(ctx.palette.border),
        container(right(session, half, ctx)).width(Length::FillPortion(1)),
    ]
    .height(Length::Fill);
    let mut page = column![halves];
    for line in trouble(session, ctx) {
        page = page.push(plan_side_line(error_line(line, ctx.palette)));
    }
    let page = page.push(footer(session, ctx.lang, ctx.palette));
    match &session.question {
        Some(question) => stack![page, question_dialog(question, ctx)].into(),
        None => page.into(),
    }
}
