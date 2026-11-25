//! The last line of the log under a run (G21).

use iced::widget::{container, row, Space};
use iced::{alignment, Element, Length};
use ncrs_i18n::Msg;
use ncrs_ui_kit::git_state::outcome_mark;
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::state::Run;

pub fn view<'a>(run: &'a Run, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let details = run.details().map(std::path::Path::to_path_buf);
    let last = run.log().lines(details.as_ref()).last();
    let line: Element<'a, Message> = match last {
        Some(entry) => row![
            page_line(entry.time.clone(), font_size::SMALL, palette.dim_text),
            outcome_mark(entry.outcome, ctx.lang, palette),
            page_line(entry.repo.clone(), font_size::SMALL, palette.text),
            page_line(entry.command.clone(), font_size::SMALL, palette.dim_text),
            page_line(entry.result.clone(), font_size::SMALL, palette.text),
        ]
        .spacing(spacing::L)
        .align_y(alignment::Vertical::Center)
        .into(),
        None => page_line(
            ctx.lang.text(Msg::ReposLogNothing),
            font_size::SMALL,
            palette.dim_text,
        ),
    };
    iced::widget::column![
        theme::horizontal_rule(palette.border_inactive),
        container(
            row![
                page_line(
                    ctx.lang.text(Msg::ReposLogTitle),
                    font_size::SMALL,
                    palette.dim_text
                ),
                line,
                Space::new().width(Length::Fill),
            ]
            .spacing(spacing::XL)
            .align_y(alignment::Vertical::Center),
        )
        .padding([0.0, spacing::XXL])
        .height(size::HEADER)
        .center_y(size::HEADER),
    ]
    .into()
}
