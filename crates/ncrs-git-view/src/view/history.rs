//! The history of one file: its commits in the middle, what the chosen one
//! changed in it at the right.

use iced::widget::{column, container};
use iced::{Element, Length};
use ncrs_git::FileHistoryEntry;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::commit_graph::{graph_cell, GraphCell, LANE_WIDTH};
use ncrs_ui_kit::format::fit_middle;
use ncrs_ui_kit::git_rows::{commit_message_width, commit_row, CommitRow, RefChipKind};
use ncrs_ui_kit::kit::{button, Tone};
use ncrs_ui_kit::page_scroll::{page_rows, RowWindow};
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use super::commits::date_of;
use super::diff_toolbar::bar;
use super::{diff, notice, HISTORY_DIFF_ID, HISTORY_ID};
use crate::ctx::ViewCtx;
use crate::file_views::HistoryState;
use crate::message::Message;
use crate::state::{Area, State};

/// How wide the old path of a rename is at most.
const RENAME_ROOM: f32 = 260.0;

fn entry_row<'a>(
    state: &'a State,
    history: &'a HistoryState,
    index: usize,
    entry: &'a FileHistoryEntry,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let show_sha = !super::is_narrow(ctx);
    let columns = state.columns_in_view(ctx.columns);
    let commit = &entry.commit;
    let chips = entry.old_path.as_ref().map(|old| {
        let old = fit_middle(old, RENAME_ROOM, font_size::SMALL);
        let note = fill_template(ctx.lang.text(Msg::GitViewHistoryRenamed), &[("{old}", &old)]);
        (note, RefChipKind::Tag)
    });
    commit_row(
        CommitRow {
            graph: graph_cell(
                GraphCell {
                    lane: 0,
                    lines: Vec::new(),
                    merge: commit.is_merge,
                    dimmed: commit.is_merge,
                },
                1,
                ctx.palette,
            ),
            chips: chips.into_iter().collect(),
            message: commit.summary.clone(),
            author: commit.author.clone(),
            date: date_of(commit, ctx),
            sha: commit.short.clone(),
            columns,
            message_width: commit_message_width(
                super::commits_pane_width(state, ctx),
                LANE_WIDTH,
                columns,
                show_sha,
            ),
            show_sha,
            chosen: history.cursor == index,
            dimmed: commit.is_merge,
            is_head: false,
            on_press: Message::HistoryPressed(index),
        },
        ctx.palette,
    )
}

pub fn view<'a>(
    state: &'a State,
    history: &'a HistoryState,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let title = fill_template(
        ctx.lang.text(Msg::GitViewHistoryTitle),
        &[("{path}", &history.path)],
    );
    let room = super::commits_pane_width(state, ctx) - 2.0 * spacing::M;
    let mut page = column![
        bar(page_heading(
            &fit_middle(&title, room, font_size::ROW),
            palette
        )),
        theme::horizontal_rule(palette.border_inactive),
    ];
    match &history.log {
        None => page = page.push(notice(ctx, Msg::GitViewLoading)),
        Some(log) if log.commits.is_empty() => {
            page = page.push(notice(ctx, Msg::GitViewHistoryEmpty));
        }
        Some(log) => {
            let window = RowWindow {
                count: log.commits.len(),
                row_height: size::ROW,
                offset: history.scroll,
                height: ctx.window.height,
            };
            let row = |index: usize| match log.commits.get(index) {
                Some(entry) => entry_row(state, history, index, entry, ctx),
                None => page_line(String::new(), font_size::BODY, palette.text),
            };
            page = page.push(page_rows(
                HISTORY_ID,
                window,
                row,
                Message::HistoryScrolled,
            ));
            if log.has_more {
                page = page.push(
                    container(button(
                        ctx.lang.text(Msg::GitViewLoadMore),
                        Some(Message::HistoryMore),
                        Tone::Secondary,
                        palette,
                    ))
                    .padding(spacing::M),
                );
            }
        }
    }
    page.height(Length::Fill).into()
}

/// What the chosen commit changed in the file.
pub fn diff_pane<'a>(
    state: &'a State,
    history: &'a HistoryState,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let Some(entry) = history.entry() else {
        return notice(
            ctx,
            match &history.log {
                Some(_) => Msg::GitViewHistoryEmpty,
                None => Msg::GitViewLoading,
            },
        );
    };
    let room = super::detail_width(state, ctx) - 2.0 * spacing::M;
    let ink = if state.area == Area::Detail {
        palette.text
    } else {
        palette.dim_text
    };
    let head = format!("{} \u{b7} {}", entry.commit.short, entry.commit.summary);
    let body = match &history.diff {
        Some(found) => diff::lines(HISTORY_DIFF_ID, found, true, ctx),
        None => notice(ctx, Msg::GitViewDiffLoading),
    };
    column![
        bar(page_line(
            fit_middle(&head, room, font_size::BODY),
            font_size::BODY,
            ink
        )),
        theme::horizontal_rule(palette.border_inactive),
        container(body).height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}
