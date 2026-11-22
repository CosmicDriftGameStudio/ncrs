//! The middle: the commit grid with its graph.

use std::time::{Duration, UNIX_EPOCH};

use iced::widget::{column, container, Column};
use iced::{Element, Length};
use ncrs_git::{LogCommit, RefKind, SegmentHalf};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::commit_graph::{graph_cell, GraphCell, GraphLine, LineHalf, LANE_WIDTH};
use ncrs_ui_kit::format;
use ncrs_ui_kit::git_rows::{
    commit_header, commit_message_width, commit_row, CommitRow, RefChipKind,
};
use ncrs_ui_kit::kit::{button, Tone};
use ncrs_ui_kit::page_scroll::page_scroll;
use ncrs_ui_kit::page_text::page_line;
use ncrs_ui_kit::theme::{font_size, spacing};

use super::COMMITS_ID;
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::state::{Area, State};

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let Some(log) = &state.log else {
        return notice(ctx, Msg::GitViewLoading);
    };
    if state.row_count() == 0 {
        return notice(ctx, Msg::GitViewNoCommits);
    }
    let show_sha = !super::is_narrow(ctx);
    let columns = state.columns_in_view(ctx.columns);
    let lanes = log.lane_count.max(1);
    let graph_width = LANE_WIDTH * lanes as f32;
    let message_width = commit_message_width(
        super::commits_pane_width(ctx),
        graph_width,
        columns,
        show_sha,
    );
    let working = state.has_working_row().then(|| {
        let count = state.current_worktree().map_or(0, |worktree| worktree.wip);
        let empty = GraphCell {
            lane: 0,
            lines: Vec::new(),
            merge: false,
            dimmed: false,
        };
        commit_row(
            CommitRow {
                graph: graph_cell(empty, lanes, palette),
                chips: Vec::new(),
                message: fill_template(
                    ctx.lang.text(Msg::GitViewWorkingCopy),
                    &[("{count}", &count.to_string())],
                ),
                author: String::new(),
                date: String::new(),
                sha: String::new(),
                columns,
                message_width,
                show_sha,
                chosen: state.area == Area::Commits && state.commit_cursor == 0,
                dimmed: false,
                on_press: Message::CommitPressed(0),
            },
            palette,
        )
    });
    let offset = usize::from(working.is_some());
    let commits = log.commits.iter().enumerate().map(|(index, commit)| {
        let row = index + offset;
        commit_row(
            CommitRow {
                graph: graph_cell(graph_of(commit), lanes, palette),
                chips: commit
                    .refs
                    .iter()
                    .map(|label| (label.name.clone(), chip_kind(label.kind)))
                    .collect(),
                message: commit.summary.clone(),
                author: commit.author.clone(),
                date: date_of(commit, ctx),
                sha: commit.short.clone(),
                columns,
                message_width,
                show_sha,
                chosen: state.area == Area::Commits && state.commit_cursor == row,
                dimmed: commit.is_merge,
                on_press: Message::CommitPressed(row),
            },
            palette,
        )
    });
    let mut rows = Column::new();
    rows = rows.extend(working);
    rows = rows.extend(commits);
    if log.has_more {
        rows = rows.push(
            container(button(
                ctx.lang.text(Msg::GitViewLoadMore),
                Some(Message::LoadMore),
                Tone::Secondary,
                palette,
            ))
            .padding(spacing::M),
        );
    }
    let header = commit_header(
        graph_width,
        [
            ctx.lang.text(Msg::GitViewColumnMessage),
            ctx.lang.text(Msg::GitViewColumnAuthor),
            ctx.lang.text(Msg::GitViewColumnDate),
            ctx.lang.text(Msg::GitViewColumnSha),
        ],
        columns,
        show_sha,
        Some(Box::new(Message::ResizePressed)),
        palette,
    );
    column![
        header,
        page_scroll(COMMITS_ID, rows.width(Length::Fill).into())
    ]
    .into()
}

fn notice<'a>(ctx: ViewCtx<'a>, text: Msg) -> Element<'a, Message> {
    container(page_line(
        ctx.lang.text(text),
        font_size::BODY,
        ctx.palette.dim_text,
    ))
    .padding(spacing::M)
    .into()
}

fn chip_kind(kind: RefKind) -> RefChipKind {
    match kind {
        RefKind::Branch => RefChipKind::Branch,
        RefKind::Remote => RefChipKind::Remote,
        RefKind::Tag => RefChipKind::Tag,
    }
}

fn graph_of(commit: &LogCommit) -> GraphCell {
    GraphCell {
        lane: commit.graph.lane,
        lines: commit
            .graph
            .segments
            .iter()
            .map(|segment| GraphLine {
                half: match segment.half {
                    SegmentHalf::Upper => LineHalf::Upper,
                    SegmentHalf::Lower => LineHalf::Lower,
                    SegmentHalf::Through => LineHalf::Through,
                },
                from: segment.from,
                to: segment.to,
            })
            .collect(),
        merge: commit.is_merge,
        dimmed: commit.is_merge,
    }
}

pub(in crate::view) fn date_of(commit: &LogCommit, ctx: ViewCtx<'_>) -> String {
    moment(commit.time, ctx)
}

pub(in crate::view) fn moment(seconds: i64, ctx: ViewCtx<'_>) -> String {
    let at = UNIX_EPOCH + Duration::from_secs(u64::try_from(seconds).unwrap_or_default());
    format::time_in(Some(at), ctx.date_format, ctx.lang)
}
