//! The diff of one file, side by side or unified, with the files of the
//! commit at the right.

use iced::widget::{column, container, row, Column, Space};
use iced::{alignment, Element, Length};
use ncrs_git::{FileDiff, RowKind};
use ncrs_i18n::Msg;
use ncrs_ui_kit::format::ellipsize_end;
use ncrs_ui_kit::git_rows::{
    change_row, diff_cell, diff_hunk_header, diff_unified_line, ChangeRow, DiffTone,
};
use ncrs_ui_kit::kit::{labelled_checkbox, segmented, Check};
use ncrs_ui_kit::page_scroll::page_scroll;
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::theme::{self, font_size, size, spacing};

use super::DIFF_ID;
use crate::commit_menu::compare_ends;
use crate::ctx::ViewCtx;
use crate::diff_layout::pairs;
use crate::message::Message;
use crate::state::{Detail, DiffState};

const COMMIT_LINE_CHARS: usize = 48;

/// The commit line of the header: the short hash and the first line of the
/// message, as far as `detail` is the commit this diff is about.
fn commit_line(shown: &DiffState, detail: Option<&Detail>, ctx: ViewCtx<'_>) -> Option<String> {
    if let Some((from, to)) = compare_ends(&shown.subject) {
        let to = to.unwrap_or_else(|| ctx.lang.text(Msg::GitViewCompareWorking).to_owned());
        return Some(format!("{from} \u{2192} {to}"));
    }
    let info = detail
        .filter(|candidate| candidate.subject == shown.subject)?
        .info
        .as_ref()?;
    let title = info.message.lines().next().unwrap_or_default();
    Some(format!(
        "{} \u{b7} {}",
        info.short,
        ellipsize_end(title, COMMIT_LINE_CHARS)
    ))
}

pub fn view<'a>(
    shown: &'a DiffState,
    detail: Option<&Detail>,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let path = shown
        .files
        .get(shown.file)
        .map(|file| file.path.clone())
        .unwrap_or_default();
    let (added, removed) = shown
        .files
        .get(shown.file)
        .filter(|file| !file.binary)
        .map_or((0, 0), |file| (file.added, file.removed));
    let mut header = row![
        page_line(
            ctx.lang.text(Msg::GitViewDiffBack),
            font_size::BODY,
            palette.dim_text
        ),
        page_heading(&path, palette),
    ];
    if let Some(commit) = commit_line(shown, detail, ctx) {
        header = header.push(page_line(commit, font_size::BODY, palette.dim_text));
    }
    if added + removed > 0 {
        header = header
            .push(page_line(
                format!("+{added}"),
                font_size::BODY,
                palette.success,
            ))
            .push(page_line(
                format!("\u{2212}{removed}"),
                font_size::BODY,
                palette.error,
            ));
    }
    let bar = container(
        header
            .push(Space::new().width(Length::Fill))
            .push(segmented(
                &[
                    (
                        ctx.lang.text(Msg::GitViewDiffSideBySide),
                        Message::SetUnified(false),
                    ),
                    (
                        ctx.lang.text(Msg::GitViewDiffUnified),
                        Message::SetUnified(true),
                    ),
                ],
                usize::from(shown.unified),
                true,
                palette,
            ))
            .push(labelled_checkbox(
                if shown.ignore_whitespace {
                    Check::On
                } else {
                    Check::Off
                },
                Some(Message::ToggleIgnoreWhitespace),
                Some(ctx.lang.text(Msg::GitViewDiffIgnoreWhitespace)),
                palette,
            ))
            .spacing(spacing::XL)
            .align_y(alignment::Vertical::Center),
    )
    .padding([0.0, spacing::XXL])
    .height(size::HEADER)
    .center_y(size::HEADER);
    let body = match &shown.diff {
        Some(diff) => lines(diff, shown.unified, ctx),
        None => notice(ctx, Msg::GitViewDiffLoading),
    };
    let files = Column::with_children(shown.files.iter().enumerate().map(|(index, file)| {
        change_row(
            ChangeRow {
                depth: 0,
                label: file.path.clone(),
                added: file.added,
                removed: file.removed,
                no_counts: file.binary,
                chosen: index == shown.file,
                on_press: Message::FilePressed(index),
            },
            palette,
        )
    }));
    row![
        column![
            bar,
            theme::horizontal_rule(palette.border_inactive),
            container(body).height(Length::Fill)
        ]
        .width(Length::Fill),
        theme::vertical_rule(palette.border_inactive),
        container(page_scroll("git-diff-files", files.into())).width(size::SIDE_PANE),
    ]
    .height(Length::Fill)
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

fn tone_of(kind: RowKind) -> DiffTone {
    match kind {
        RowKind::Context => DiffTone::Context,
        RowKind::Added => DiffTone::Added,
        RowKind::Removed => DiffTone::Removed,
    }
}

fn lines<'a>(diff: &'a FileDiff, unified: bool, ctx: ViewCtx<'a>) -> Element<'a, Message> {
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
        rows = rows.push(diff_hunk_header(crate::diff_layout::hunk_title(hunk), palette));
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
                        if line.kind == RowKind::Context {
                            DiffTone::Context
                        } else {
                            tone_of(line.kind)
                        },
                        palette,
                    ),
                    None => diff_cell(None, String::new(), DiffTone::Empty, palette),
                };
                rows = rows.push(row![side(pair.left, true), side(pair.right, false)]);
            }
        }
    }
    page_scroll(DIFF_ID, rows.into())
}
