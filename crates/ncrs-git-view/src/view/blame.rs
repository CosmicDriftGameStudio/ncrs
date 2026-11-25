//! The lines of one file, each run of them with the commit it comes from.

use iced::widget::{column, container, row};
use iced::{alignment, Element, Length};
use ncrs_git::{Blame, BlameLimit};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::blame_rows::{blame_row, BlameRow};
use ncrs_ui_kit::format::{estimated_width, fit_middle};
use ncrs_ui_kit::page_scroll::{page_rows, RowWindow};
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use super::commits::moment;
use super::diff::short;
use super::diff_toolbar::bar;
use super::{notice, BLAME_ID};
use crate::ctx::ViewCtx;
use crate::file_views::BlameState;
use crate::message::Message;
use crate::state::State;

fn refusal(limit: &BlameLimit) -> Option<Msg> {
    match limit {
        BlameLimit::None => None,
        BlameLimit::Binary => Some(Msg::GitViewBlameBinary),
        BlameLimit::TooLarge => Some(Msg::GitViewBlameTooLarge),
        BlameLimit::NotAFile => Some(Msg::GitViewBlameNotAFile),
        BlameLimit::NotCommitted => Some(Msg::GitViewBlameNotCommitted),
    }
}

fn line<'a>(
    found: &'a Blame,
    shown: &'a BlameState,
    index: usize,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let of_line = found
        .lines
        .get(index)
        .and_then(|line| Some((line, found.groups.get(line.group)?)));
    let Some((line, group)) = of_line else {
        return page_line(String::new(), font_size::BODY, ctx.palette.text);
    };
    blame_row(
        BlameRow {
            sha_short: group.short.clone(),
            author: group.author.clone(),
            date: moment(group.time, ctx),
            line_no: line.line_no,
            text: line.text.clone(),
            first_of_group: group.first_line == line.line_no,
            chosen: shown.cursor == index,
            on_sha_press: Message::JumpToCommit(group.sha.clone()),
            on_press: Message::BlamePressed(index),
        },
        ctx.palette,
    )
}

pub fn view<'a>(state: &'a State, shown: &'a BlameState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let at = shown
        .at
        .as_ref()
        .map_or_else(|| "HEAD".to_owned(), |sha| short(sha));
    let title = fill_template(
        ctx.lang.text(Msg::GitViewBlameTitle),
        &[("{sha}", &at), ("{path}", &shown.path)],
    );
    let note = match shown.at {
        Some(_) => "",
        None => ctx.lang.text(Msg::GitViewBlameHeadOnly),
    };
    let room = super::commits_pane_width(state, ctx)
        - estimated_width(note, font_size::SMALL)
        - 3.0 * spacing::M;
    let head = row![
        page_heading(&fit_middle(&title, room, font_size::ROW), palette),
        container(page_line(
            note.to_owned(),
            font_size::SMALL,
            palette.dim_text
        ))
        .width(Length::Fill)
        .clip(true),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    let body = match &shown.blame {
        None => notice(ctx, Msg::GitViewLoading),
        Some(found) => match refusal(&found.refused) {
            Some(reason) => notice(ctx, reason),
            None => {
                let window = RowWindow {
                    count: found.lines.len(),
                    row_height: size::ROW,
                    offset: shown.scroll,
                    height: ctx.window.height,
                };
                page_rows(
                    BLAME_ID,
                    window,
                    |index| line(found, shown, index, ctx),
                    Message::BlameScrolled,
                )
            }
        },
    };
    column![
        bar(head.into()),
        theme::horizontal_rule(palette.border_inactive),
        container(body).height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}
