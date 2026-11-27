//! What the last delete run did: the numbers, every row with its outcome, the way to try
//! the skipped ones again.

use std::path::Path;

use iced::widget::{column, container, row, Space};
use iced::{alignment, Element, Length, Padding};
use ncrs_i18n::Msg;
use ncrs_ui_kit::format::{shown_path, size_shown};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::page_scroll::page_scroll_padded;
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::size_row::{size_row, SizeRow};
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::ResultState;
use crate::texts::{fill_count, fill_size, outcome_of};

const RESULT_LIST_ID: &str = "space-result-list";

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn numbers<'a>(done: &'a ResultState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let figure = |text: String, ink| page_line(text, font_size::FIGURE, ink);
    let mut line = row![
        figure(
            fill_size(Msg::SpaceResultFreed, done.freed(), ctx.lang),
            palette.success
        ),
        figure(
            fill_count(Msg::SpaceResultDeleted, done.deleted(), ctx.lang),
            palette.text
        ),
        figure(
            fill_count(Msg::SpaceResultSkipped, done.skipped(), ctx.lang),
            if done.skipped() > 0 {
                palette.error
            } else {
                palette.dim_text
            },
        ),
    ]
    .spacing(spacing::XXL)
    .align_y(alignment::Vertical::Center);
    if let Some(free_after) = done.free_after {
        line = line.push(page_line(
            fill_size(Msg::SpaceResultFreeNow, free_after, ctx.lang),
            font_size::BODY,
            palette.text,
        ));
        line = line.push(page_line(
            fill_size(Msg::SpaceResultBefore, done.free_before, ctx.lang),
            font_size::BODY,
            palette.dim_text,
        ));
    }
    line = line.push(page_line(
        ctx.lang.text(Msg::SpaceResultPermanent),
        font_size::SMALL,
        palette.dim_text,
    ));
    container(line).padding([spacing::L, spacing::XXL]).into()
}

fn buttons<'a>(done: &'a ResultState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    container(
        row![
            kit::key_button(
                ctx.lang.text(Msg::SpaceRetrySkipped),
                &ctx.keys.label(Shortcut::Retry),
                (done.skipped() > 0).then_some(Message::Retry),
                Tone::Secondary,
                palette,
            ),
            kit::button(
                ctx.lang.text(Msg::SpaceLogTitle),
                Some(Message::ShowLog),
                Tone::Secondary,
                palette,
            ),
            Space::new().width(Length::Fill),
            kit::key_button(
                ctx.lang.text(Msg::SpaceCloseResult),
                "Esc",
                Some(Message::CloseResult),
                Tone::Primary,
                palette,
            ),
        ]
        .spacing(spacing::L),
    )
    .padding([0.0, spacing::XXL])
    .into()
}

fn rows<'a>(done: &'a ResultState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let biggest = done.log.rows.iter().map(|row| row.size).max().unwrap_or(0);
    let mut ordered: Vec<_> = done.log.rows.iter().collect();
    ordered.sort_by_key(|logged| (logged.outcome.is_ok(), std::cmp::Reverse(logged.size)));
    let lines = ordered.into_iter().map(|logged| {
        size_row(
            SizeRow {
                check: None,
                name: file_name(&logged.path),
                path: shown_path(&logged.path, ctx.home),
                size_text: size_shown(logged.size, ctx.lang),
                fraction: logged.size as f32 / biggest.max(1) as f32,
                color: theme::space::safe(ctx.palette),
                reason: String::new(),
                outcome: Some(outcome_of(&logged.outcome, ctx.lang)),
                cursor: false,
                on_press: None,
                on_toggle: None,
            },
            ctx.palette,
        )
    });
    page_scroll_padded(
        RESULT_LIST_ID,
        column(lines).width(Length::Fill).into(),
        Padding::from([0.0, spacing::XL]),
    )
}

pub fn view<'a>(done: &'a ResultState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    column![
        container(page_heading(ctx.lang.text(Msg::SpaceResultTitle), palette))
            .padding([0.0, spacing::XXL])
            .height(size::HEADER)
            .center_y(size::HEADER),
        theme::horizontal_rule(palette.border_inactive),
        numbers(done, ctx),
        buttons(done, ctx),
        container(rows(done, ctx))
            .height(Length::Fill)
            .padding([spacing::M, 0.0]),
        container(kit::dim(ctx.lang.text(Msg::SpaceResultNote), palette))
            .padding([spacing::M, spacing::XXL]),
    ]
    .height(Length::Fill)
    .into()
}
