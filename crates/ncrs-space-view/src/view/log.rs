//! The log panel under the list of a result: this run, row by row, in the order it wrote them.

use std::time::SystemTime;

use iced::widget::{column, container, row, Space};
use iced::{alignment, Element, Length, Padding};
use ncrs_i18n::{fill_template, Msg};
use ncrs_space::Skip;
use ncrs_ui_kit::format::{clock_seconds, shown_path, size_shown};
use ncrs_ui_kit::page_scroll::page_scroll_padded;
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::state::{ResultState, State};
use crate::texts::{fill_size, skip_text};

const LOG_LIST_ID: &str = "space-log-list";

struct Line {
    when: Option<SystemTime>,
    path: std::path::PathBuf,
    size: u64,
    outcome: Result<u64, Skip>,
    last_used: Option<SystemTime>,
    use_checked: bool,
}

/// The rows of the run: from the log file when it could be read, else as the run reported them.
fn lines_of(done: &ResultState) -> Vec<Line> {
    match &done.log_run {
        Some(run) if !run.rows.is_empty() => run
            .rows
            .iter()
            .map(|logged| Line {
                when: Some(logged.when),
                path: logged.path.clone(),
                size: logged.size,
                outcome: logged.outcome.clone(),
                last_used: logged.last_used,
                use_checked: logged.use_checked,
            })
            .collect(),
        _ => done
            .log
            .rows
            .iter()
            .map(|logged| Line {
                when: None,
                path: logged.path.clone(),
                size: logged.size,
                outcome: logged.outcome.clone(),
                last_used: logged.last_used,
                use_checked: logged.use_checked,
            })
            .collect(),
    }
}

fn result_words(line: &Line, ctx: ViewCtx<'_>) -> String {
    match &line.outcome {
        Err(skip) => skip_text(skip, ctx.lang),
        Ok(_) if line.use_checked && line.last_used.is_some() => fill_template(
            ctx.lang.text(Msg::SpaceLogDeletedChecked),
            &[(
                "{date}",
                &ncrs_ui_kit::format::day_in(line.last_used, ctx.lang),
            )],
        ),
        Ok(_) => ctx.lang.text(Msg::SpaceLogDeleted).to_owned(),
    }
}

fn line_view<'a>(line: &Line, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let ink = if line.outcome.is_ok() {
        palette.text
    } else {
        palette.error
    };
    row![
        container(page_line(
            clock_seconds(line.when),
            font_size::SMALL,
            palette.dim_text
        ))
        .width(size::LOG_TIME_WIDTH),
        container(page_line(
            shown_path(&line.path, ctx.home),
            font_size::BODY,
            palette.text
        ))
        .width(Length::FillPortion(3)),
        container(page_line(
            size_shown(line.size, ctx.lang),
            font_size::SMALL,
            palette.dim_text
        ))
        .width(size::LOG_TIME_WIDTH)
        .align_x(alignment::Horizontal::Right),
        container(page_line(result_words(line, ctx), font_size::BODY, ink))
            .width(Length::FillPortion(4)),
    ]
    .spacing(spacing::XL)
    .into()
}

pub fn panel<'a>(
    state: &'a State,
    done: &'a ResultState,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lines = lines_of(done);
    let span = done.log_run.as_ref().map_or_else(String::new, |run| {
        fill_template(
            ctx.lang.text(Msg::SpaceLogSpan),
            &[
                ("{from}", &clock_seconds(Some(run.started))),
                ("{to}", &clock_seconds(Some(run.finished))),
            ],
        )
    });
    let mut body = column(lines.iter().map(|line| line_view(line, ctx)))
        .spacing(spacing::XS)
        .width(Length::Fill);
    let closing_free = done.free_after.map(|free| {
        fill_template(
            ctx.lang.text(Msg::SpaceLogDoneFreed),
            &[
                ("{freed}", &size_shown(done.freed(), ctx.lang)),
                ("{free}", &size_shown(free, ctx.lang)),
            ],
        )
    });
    if let Some(words) = closing_free {
        body = body.push(
            row![
                container(page_line(
                    clock_seconds(Some(done.log.finished)),
                    font_size::SMALL,
                    palette.dim_text
                ))
                .width(size::LOG_TIME_WIDTH),
                page_line(
                    ctx.lang.text(Msg::SpaceLogDone),
                    font_size::BODY,
                    palette.text
                ),
                page_line(words, font_size::BODY, palette.success),
            ]
            .spacing(spacing::XL),
        );
    }
    let _ = state;
    column![
        theme::horizontal_rule(palette.border_inactive),
        container(
            row![
                page_line(
                    ctx.lang.text(Msg::SpaceLogTitle).to_uppercase(),
                    font_size::SMALL,
                    palette.text
                ),
                page_line(span, font_size::SMALL, palette.dim_text),
                Space::new().width(Length::Fill),
                page_line(
                    fill_size(Msg::SpaceResultFreed, done.freed(), ctx.lang),
                    font_size::SMALL,
                    palette.dim_text
                ),
            ]
            .spacing(spacing::L),
        )
        .padding([spacing::XS, spacing::XL]),
        container(page_scroll_padded(
            LOG_LIST_ID,
            body.into(),
            Padding::from([0.0, spacing::XL])
        ))
        .height(size::LOG_PANEL_HEIGHT),
    ]
    .into()
}
