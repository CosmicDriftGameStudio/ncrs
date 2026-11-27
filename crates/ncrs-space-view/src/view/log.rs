//! The log of the runs, read back from the files `append_log` wrote.

use iced::widget::{column, container, row, Space};
use iced::{alignment, Element, Length, Padding};
use ncrs_i18n::Msg;
use ncrs_ui_kit::format::{shown_path, size_shown, time};
use ncrs_ui_kit::kit::{self, Tone};
use ncrs_ui_kit::page_scroll::page_scroll_padded;
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use crate::ctx::ViewCtx;
use crate::log_lines::{parse_all, LogLine};
use crate::message::Message;
use crate::state::ResultState;
use crate::texts::fill_size;

const LOG_LIST_ID: &str = "space-log-list";

fn line_view<'a>(line: &LogLine, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let (words, ink) = match (&line.skipped, line.freed) {
        (Some(skipped), _) => (skipped.clone(), palette.error),
        (None, Some(freed)) => (
            fill_size(Msg::SpaceResultRowDeleted, freed, ctx.lang),
            palette.text,
        ),
        (None, None) => (String::new(), palette.dim_text),
    };
    row![
        page_line(time(Some(line.when)), font_size::SMALL, palette.dim_text),
        page_line(
            shown_path(&line.path, ctx.home),
            font_size::BODY,
            palette.text
        ),
        page_line(
            size_shown(line.size, ctx.lang),
            font_size::SMALL,
            palette.dim_text
        ),
        page_line(words, font_size::BODY, ink),
    ]
    .spacing(spacing::XL)
    .into()
}

pub fn view<'a>(done: &'a ResultState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lines = done.log_lines.as_deref().map(parse_all).unwrap_or_default();
    let body: Element<'a, Message> = if lines.is_empty() {
        let note = if done.log_lines.is_some() {
            ctx.lang.text(Msg::SpaceEmpty)
        } else {
            ""
        };
        page_line(note, font_size::BODY, palette.dim_text)
    } else {
        column(lines.iter().map(|line| line_view(line, ctx)))
            .spacing(spacing::M)
            .width(Length::Fill)
            .into()
    };
    column![
        container(
            row![
                page_heading(ctx.lang.text(Msg::SpaceLogTitle), palette),
                Space::new().width(Length::Fill),
                kit::key_button(
                    ctx.lang.text(Msg::SpaceCloseResult),
                    "Esc",
                    Some(Message::HideLog),
                    Tone::Secondary,
                    palette,
                ),
            ]
            .align_y(alignment::Vertical::Center),
        )
        .padding([0.0, spacing::XXL])
        .height(size::HEADER)
        .center_y(size::HEADER),
        theme::horizontal_rule(palette.border_inactive),
        page_scroll_padded(
            LOG_LIST_ID,
            body,
            Padding::from([spacing::XL, spacing::XXL])
        ),
    ]
    .height(Length::Fill)
    .into()
}
