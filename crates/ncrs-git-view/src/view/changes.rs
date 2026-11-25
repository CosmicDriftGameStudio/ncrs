//! The diff of the chosen local change, in the middle: a toolbar for the
//! file, a button per hunk, and the end of the file.

use iced::widget::{column, container, responsive, row, Column, Row, Space};
use iced::{alignment, Element, Length};
use ncrs_git::{ChangeKind, DiffHunk, DiffRow, RowKind};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::action_rows::hunk_bar;
use ncrs_ui_kit::format::fit_middle;
use ncrs_ui_kit::git_rows::{diff_cell, diff_note, diff_unified_line, DiffTone};
use ncrs_ui_kit::kit::{
    compact_button, focused_frame, icon_button, key_button, labelled_checkbox, segmented, Check,
    Icon, Tone,
};
use ncrs_ui_kit::page_scroll::page_scroll;
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use crate::actions::{ChangeSide, LocalChanges};
use crate::ctx::ViewCtx;
use crate::diff_layout::{hunk_title, pairs};
use crate::message::Message;
use crate::shortcuts::Shortcut;

pub const HUNKS_ID: &str = "git-local-diff";

/// How much of the path the first row of the toolbar shows before the middle of it is cut out.
const PATH_ROOM: f32 = 240.0;

pub fn kind_letter(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "A",
        ChangeKind::Modified => "M",
        ChangeKind::Deleted => "D",
        ChangeKind::Renamed => "R",
    }
}

fn tone_of(kind: RowKind) -> DiffTone {
    match kind {
        RowKind::Context => DiffTone::Context,
        RowKind::Added => DiffTone::Added,
        RowKind::Removed => DiffTone::Removed,
    }
}

/// Below this width of the middle the toolbar drops what the buttons already say.
const TOOLBAR_NARROW: f32 = 1000.0;

/// The file with what can be done to it and how it is shown, then where it
/// comes from and the way to the editor: two rows of one line each.
fn toolbar<'a>(local: &'a LocalChanges, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    responsive(move |bounds| toolbar_rows(local, bounds.width < TOOLBAR_NARROW, ctx))
        .height(2.0 * size::COMPARE_BAR)
        .into()
}

fn toolbar_rows<'a>(
    local: &'a LocalChanges,
    narrow: bool,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let Some((side, index)) = local.selected else {
        return Space::new().into();
    };
    let files = match side {
        ChangeSide::Unstaged => &local.unstaged,
        ChangeSide::Staged => &local.staged,
    };
    let Some(file) = files.get(index) else {
        return Space::new().into();
    };
    let (state_text, toggle_label, toggle, hunk_label) = match side {
        ChangeSide::Unstaged => (
            Msg::GitViewChangesUnstaged,
            Msg::GitViewStage,
            Message::Stage(index),
            Msg::GitViewHunkStage,
        ),
        ChangeSide::Staged => (
            Msg::GitViewChangesStaged,
            Msg::GitViewUnstage,
            Message::Unstage(index),
            Msg::GitViewHunkUnstage,
        ),
    };
    let comparison = match side {
        ChangeSide::Unstaged => Msg::GitViewDiffIndexToWorktree,
        ChangeSide::Staged => Msg::GitViewDiffHeadToIndex,
    };
    let total = local.diff.as_ref().map_or(0, |diff| diff.hunks.len());
    let current = local.hunk.min(total.saturating_sub(1));
    let hunk_message = match side {
        ChangeSide::Unstaged => Message::StageHunk(current),
        ChangeSide::Staged => Message::UnstageHunk(current),
    };
    let hunk_tip = local
        .ignore_whitespace
        .then(|| lang.text(Msg::GitViewHunkLockedWhitespace));
    let mut first = Row::new()
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center)
        .push(page_line(
            kind_letter(file.kind).to_owned(),
            font_size::BODY,
            palette.text,
        ))
        .push(
            container(page_heading(
                &fit_middle(&file.path, PATH_ROOM, font_size::ROW),
                palette,
            ))
            .width(Length::Fill)
            .clip(true),
        )
        .extend((!narrow).then(|| {
            page_line(
                lang.text(state_text).to_owned(),
                font_size::SMALL,
                palette.dim_text,
            )
        }))
        .push(compact_button(
            lang.text(toggle_label),
            None,
            Some(toggle),
            Tone::Primary,
            palette,
        ))
        .push(compact_button(
            lang.text(hunk_label),
            hunk_tip,
            (total > 0 && !local.ignore_whitespace).then_some(hunk_message),
            Tone::Secondary,
            palette,
        ))
        .push(segmented(
            &[
                (
                    lang.text(Msg::GitViewDiffSideBySide),
                    Message::SetUnified(false),
                ),
                (
                    lang.text(Msg::GitViewDiffUnified),
                    Message::SetUnified(true),
                ),
            ],
            usize::from(local.unified),
            true,
            palette,
        ))
        .push(labelled_checkbox(
            if local.ignore_whitespace {
                Check::On
            } else {
                Check::Off
            },
            Some(Message::ToggleIgnoreWhitespace),
            Some(lang.text(Msg::GitViewDiffIgnoreWhitespace)),
            palette,
        ));
    first = first.push(change_stepper(local, narrow, ctx));
    column![
        bar(first.into()),
        bar(row![
            page_line(
                fill_template(lang.text(comparison), &[("{path}", &file.path)]),
                font_size::SMALL,
                palette.dim_text
            ),
            Space::new().width(Length::Fill),
            key_button(
                lang.text(Msg::GitViewOpenEditor),
                &Shortcut::OpenEditor.label(ctx.keys),
                Some(Message::OpenSelectedInEditor),
                Tone::Secondary,
                palette,
            ),
        ]
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center)
        .into()),
    ]
    .into()
}

/// One line of the toolbar: 32 px, no wrapping.
fn bar<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .padding([0.0, spacing::M])
        .height(size::COMPARE_BAR)
        .center_y(size::COMPARE_BAR)
        .clip(true)
        .into()
}

/// "↑ ↓ Change x of y": which change of the file has the keys.
fn change_stepper<'a>(
    local: &'a LocalChanges,
    narrow: bool,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let total = local.diff.as_ref().map_or(0, |diff| diff.hunks.len());
    let current = local.hunk.min(total.saturating_sub(1)) + usize::from(total > 0);
    row![
        icon_button(
            Icon::ArrowUp,
            None,
            lang.text(Msg::GitViewDiffPreviousChange),
            (current > 1).then_some(Message::PreviousHunk),
            Tone::Secondary,
            palette,
        ),
        icon_button(
            Icon::ArrowDown,
            None,
            lang.text(Msg::GitViewDiffNextChange),
            (current < total).then_some(Message::NextHunk),
            Tone::Secondary,
            palette,
        ),
    ]
    .extend((!narrow).then(|| {
        page_line(
            fill_template(
                lang.text(Msg::GitViewDiffChangeOf),
                &[
                    ("{current}", &current.to_string()),
                    ("{total}", &total.to_string()),
                ],
            ),
            font_size::SMALL,
            palette.dim_text,
        )
    }))
    .spacing(spacing::XS)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn side_cell<'a>(line: Option<&DiffRow>, old: bool, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    match line {
        Some(shown) => diff_cell(
            if old { shown.old_line } else { shown.new_line },
            shown.text.clone(),
            tone_of(shown.kind),
            ctx.palette,
        ),
        None => diff_cell(None, String::new(), DiffTone::Empty, ctx.palette),
    }
}

/// The lines of one hunk, in one column or old and new next to each other.
fn hunk_lines<'a>(
    hunk: &'a DiffHunk,
    unified: bool,
    ctx: ViewCtx<'a>,
) -> Vec<Element<'a, Message>> {
    if unified {
        hunk.rows
            .iter()
            .map(|line| {
                diff_unified_line(
                    line.old_line,
                    line.new_line,
                    line.text.clone(),
                    tone_of(line.kind),
                    ctx.palette,
                )
            })
            .collect()
    } else {
        pairs(hunk)
            .into_iter()
            .map(|pair| {
                row![
                    side_cell(pair.left, true, ctx),
                    side_cell(pair.right, false, ctx)
                ]
                .into()
            })
            .collect()
    }
}

/// "End of file · n lines": the length of the whole file on each side, under
/// each side when they are next to each other. Without a length (a binary or
/// too large file) there is no line.
fn end_of_file<'a>(
    diff: &ncrs_git::FileDiff,
    unified: bool,
    ctx: ViewCtx<'a>,
) -> Option<Element<'a, Message>> {
    let footer = |total: Option<u32>| {
        total.map(|count| {
            diff_note(
                fill_template(
                    ctx.lang.text(Msg::GitViewDiffEndOfFile),
                    &[("{count}", &count.to_string())],
                ),
                ctx.palette,
            )
        })
    };
    if unified {
        footer(diff.new_total)
    } else if diff.old_total.is_none() && diff.new_total.is_none() {
        None
    } else {
        Some(
            row![
                footer(diff.old_total).unwrap_or_else(|| Space::new().into()),
                footer(diff.new_total).unwrap_or_else(|| Space::new().into()),
            ]
            .into(),
        )
    }
}

fn hunk_actions<'a>(
    local: &'a LocalChanges,
    staged: bool,
    index: usize,
    ctx: ViewCtx<'a>,
) -> Vec<Element<'a, Message>> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let locked = local.ignore_whitespace;
    let tip = locked.then(|| lang.text(Msg::GitViewHunkLockedWhitespace));
    let press = |message: Message| (!locked).then_some(message);
    if staged {
        vec![compact_button(
            lang.text(Msg::GitViewHunkUnstage),
            tip,
            press(Message::UnstageHunk(index)),
            Tone::Secondary,
            palette,
        )]
    } else {
        vec![
            compact_button(
                lang.text(Msg::GitViewHunkStage),
                tip,
                press(Message::StageHunk(index)),
                Tone::Secondary,
                palette,
            ),
            compact_button(
                lang.text(Msg::GitViewHunkDiscard),
                tip,
                press(Message::DiscardHunk(index)),
                Tone::DangerOutline,
                palette,
            ),
        ]
    }
}

fn hunks<'a>(local: &'a LocalChanges, has_keys: bool, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let Some(diff) = &local.diff else {
        return Space::new().into();
    };
    let staged = matches!(local.selected, Some((ChangeSide::Staged, _)));
    let current = local.hunk.min(diff.hunks.len().saturating_sub(1));
    let mut rows = Column::new().width(Length::Fill);
    for (index, hunk) in diff.hunks.iter().enumerate() {
        let mut block = Column::new().width(Length::Fill).push(hunk_bar(
            hunk_title(hunk),
            hunk_actions(local, staged, index, ctx),
            palette,
        ));
        for line in hunk_lines(hunk, local.unified, ctx) {
            block = block.push(line);
        }
        let ring = index == current && local.diff_open && has_keys;
        rows = rows.push(focused_frame(block, ring, palette));
    }
    if let Some(footer) = end_of_file(diff, local.unified, ctx) {
        rows = rows.push(footer);
    }
    page_scroll(HUNKS_ID, rows.into())
}

/// The middle while a change is open: the toolbar and the hunks.
/// `has_keys`: the diff, not a commit field, has the keyboard.
pub fn diff_view<'a>(
    local: &'a LocalChanges,
    has_keys: bool,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    column![
        toolbar(local, ctx),
        theme::horizontal_rule(ctx.palette.border_inactive),
        hunks(local, has_keys, ctx),
    ]
    .height(Length::Fill)
    .into()
}
