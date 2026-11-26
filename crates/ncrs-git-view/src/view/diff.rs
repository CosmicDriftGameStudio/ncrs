//! The diff of one file, side by side or unified, with its toolbar, and the
//! list of the files it can show.

use iced::widget::{column, container, responsive, row, Column, Row, Space};
use iced::{alignment, Element, Length};
use ncrs_git::{ChangeKind, FileDiff, RowKind};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::action_rows::context_area;
use ncrs_ui_kit::format::{ellipsize_end, fit_middle};
use ncrs_ui_kit::git_rows::{
    change_row, diff_cell, diff_hunk_header, diff_unified_line, ChangeRow, DiffTone, COUNT_WIDTH,
};
use ncrs_ui_kit::page_scroll::page_scroll;
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use super::diff_toolbar::{
    bar, change_stepper, file_row, kind_letter, layout_choice, whitespace_choice, FileButtons,
    TOOLBAR_NARROW,
};
use super::{notice, DIFF_ID};
use crate::actions::MenuTarget;
use crate::commit_menu::compare_ends;
use crate::ctx::ViewCtx;
use crate::diff_layout::{hunk_title, pairs};
use crate::file_views::commit_of;
use crate::message::Message;
use crate::state::{DiffState, State, Subject};

const COMMIT_LINE_CHARS: usize = 48;
/// How much of the path the toolbar shows before the middle of it is cut out.
const PATH_ROOM: f32 = 360.0;
const PATH_ROOM_NARROW: f32 = 200.0;

/// Below this width of the middle the toolbar leaves out which change of how many is shown.
const STEPPER_NARROW: f32 = 880.0;
/// What the counts, the scroll bar and the padding take from a name in the list of files.
const FILE_LABEL_TAKEN: f32 = COUNT_WIDTH + 70.0;
/// The side of a comparison that has no commit: before the first one.
const NO_SIDE: &str = "\u{2013}";
/// How many characters of a commit's id are written.
const SHORT: usize = 7;

/// The commit the diff is about: short id, title and the short id of its parent.
struct CommitLine {
    short: String,
    title: String,
    parent: Option<String>,
}

pub fn short(sha: &str) -> String {
    sha.chars().take(SHORT).collect()
}

fn commit_of_diff(state: &State, shown: &DiffState) -> Option<CommitLine> {
    let Subject::Commit(sha) = &shown.subject else {
        return None;
    };
    let detail = state
        .detail
        .as_ref()
        .filter(|detail| detail.subject == shown.subject)
        .and_then(|detail| detail.info.as_ref());
    if let Some(info) = detail {
        return Some(CommitLine {
            short: info.short.clone(),
            title: info.message.lines().next().unwrap_or_default().to_owned(),
            parent: info.parents.first().map(|parent| short(parent)),
        });
    }
    let entry = state
        .history
        .as_ref()
        .and_then(|history| history.entry())
        .filter(|entry| entry.commit.id == *sha)?;
    Some(CommitLine {
        short: entry.commit.short.clone(),
        title: entry.commit.summary.clone(),
        parent: entry.commit.parents.first().map(|parent| short(parent)),
    })
}

/// "a/{old} ({old side}) → b/{new} ({new side})" for the file that is shown.
fn sides(state: &State, shown: &DiffState, ctx: ViewCtx<'_>) -> String {
    let Some(file) = shown.files.get(shown.file) else {
        return String::new();
    };
    let working = || ctx.lang.text(Msg::GitViewCompareWorking).to_owned();
    let (old_side, new_side) = match &shown.subject {
        Subject::Working => ("HEAD".to_owned(), working()),
        Subject::Commit(sha) => {
            let commit = commit_of_diff(state, shown);
            let parent = commit.and_then(|line| line.parent);
            (parent.unwrap_or_else(|| NO_SIDE.to_owned()), short(sha))
        }
        Subject::Compare { from, to } => (short(from), to.as_deref().map_or_else(working, short)),
    };
    fill_template(
        ctx.lang.text(Msg::GitViewDiffSides),
        &[
            ("{old_side}", &old_side),
            ("{new_side}", &new_side),
            ("{old}", file.old_path.as_ref().unwrap_or(&file.path)),
            ("{new}", &file.path),
        ],
    )
}

fn toolbar_rows<'a>(
    state: &'a State,
    shown: &'a DiffState,
    width: f32,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let narrow = width < TOOLBAR_NARROW;
    let Some(file) = shown.files.get(shown.file) else {
        return Space::new().into();
    };
    let subject = match compare_ends(&shown.subject) {
        Some((from, to)) => {
            let to_side =
                to.unwrap_or_else(|| ctx.lang.text(Msg::GitViewCompareWorking).to_owned());
            Some(format!("{from} \u{2192} {to_side}"))
        }
        None => commit_of_diff(state, shown).map(|commit| {
            if narrow {
                commit.short
            } else {
                let title = ellipsize_end(&commit.title, COMMIT_LINE_CHARS);
                format!("{} \u{b7} {title}", commit.short)
            }
        }),
    };
    let path_room = if narrow { PATH_ROOM_NARROW } else { PATH_ROOM };
    let total = shown.diff.as_ref().map_or(0, |diff| diff.hunks.len());
    let first = Row::new()
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center)
        .push(page_line(
            kind_letter(file.kind).to_owned(),
            font_size::BODY,
            palette.text,
        ))
        .push(page_heading(
            &fit_middle(&file.path, path_room, font_size::ROW),
            palette,
        ))
        .push(
            container(page_line(
                subject.unwrap_or_default(),
                font_size::SMALL,
                palette.dim_text,
            ))
            .width(Length::Fill)
            .clip(true),
        )
        .push(layout_choice(shown.unified, ctx))
        .push(whitespace_choice(shown.ignore_whitespace, ctx))
        .push(change_stepper(
            shown.hunk,
            total,
            width < STEPPER_NARROW,
            ctx,
        ));
    let buttons = FileButtons {
        committed: !file.untracked,
        on_disk: file.kind != ChangeKind::Deleted,
    };
    column![
        bar(first.into()),
        file_row(buttons, sides(state, shown, ctx), ctx),
    ]
    .into()
}

pub fn view<'a>(state: &'a State, shown: &'a DiffState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let body = match &shown.diff {
        _ if shown.files.is_empty() => notice(ctx, Msg::GitViewCompareNoDifferences),
        Some(diff) => lines(DIFF_ID, diff, shown.unified, ctx),
        None => notice(ctx, Msg::GitViewDiffLoading),
    };
    column![
        responsive(move |bounds| toolbar_rows(state, shown, bounds.width, ctx))
            .height(2.0 * size::COMPARE_BAR),
        theme::horizontal_rule(ctx.palette.border_inactive),
        container(body).height(Length::Fill),
    ]
    .height(Length::Fill)
    .into()
}

/// The right side of a comparison: the files that differ.
pub fn file_list<'a>(shown: &'a DiffState, width: f32, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let files = Column::with_children(shown.files.iter().enumerate().map(|(index, file)| {
        let line = change_row(
            ChangeRow {
                depth: 0,
                label: fit_middle(&file.path, width - FILE_LABEL_TAKEN, font_size::BODY),
                added: file.added,
                removed: file.removed,
                no_counts: file.binary,
                chosen: index == shown.file,
                on_press: Message::FilePressed(index),
            },
            ctx.palette,
        );
        let target = MenuTarget::File {
            path: file.path.clone(),
            at: commit_of(&shown.subject),
        };
        context_area(line, Message::OpenMenu(target))
    }));
    page_scroll("git-diff-files", files.into())
}

fn tone_of(kind: RowKind) -> DiffTone {
    match kind {
        RowKind::Context => DiffTone::Context,
        RowKind::Added => DiffTone::Added,
        RowKind::Removed => DiffTone::Removed,
    }
}

/// The lines of the diff in a scroll area of their own.
pub fn lines<'a>(
    id: &'static str,
    diff: &'a FileDiff,
    unified: bool,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    if diff.binary {
        return notice(ctx, Msg::GitViewDiffBinary);
    }
    if diff.too_large {
        return notice(ctx, Msg::GitViewDiffTooLarge);
    }
    if diff.hunks.is_empty() {
        return notice(ctx, Msg::GitViewDiffNoChanges);
    }
    let palette = ctx.palette;
    let mut rows = Column::new().width(Length::Fill);
    for hunk in &diff.hunks {
        rows = rows.push(diff_hunk_header(hunk_title(hunk), palette));
        if unified {
            for line in &hunk.rows {
                rows = rows.push(diff_unified_line(
                    line.old_line,
                    line.new_line,
                    line.text.clone(),
                    tone_of(line.kind),
                    palette,
                ));
            }
        } else {
            for pair in pairs(hunk) {
                let side = |cell: Option<&ncrs_git::DiffRow>, old: bool| match cell {
                    Some(line) => diff_cell(
                        if old { line.old_line } else { line.new_line },
                        line.text.clone(),
                        tone_of(line.kind),
                        palette,
                    ),
                    None => diff_cell(None, String::new(), DiffTone::Empty, palette),
                };
                rows = rows.push(row![side(pair.left, true), side(pair.right, false)]);
            }
        }
    }
    page_scroll(id, rows.into())
}

/// "{file} · +a −b" for the status bar, counted from the diff that is read.
pub fn summary(path: &str, diff: Option<&FileDiff>, ctx: ViewCtx<'_>) -> Option<String> {
    let shown = diff?;
    let count = |kind: RowKind| {
        let rows = shown.hunks.iter().flat_map(|hunk| &hunk.rows);
        rows.filter(|row| row.kind == kind).count().to_string()
    };
    Some(fill_template(
        ctx.lang.text(Msg::GitViewStatusDiffRight),
        &[
            ("{added}", &count(RowKind::Added)),
            ("{removed}", &count(RowKind::Removed)),
            ("{file}", path.rsplit('/').next().unwrap_or(path)),
        ],
    ))
}
