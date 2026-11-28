//! The question before anything is deleted for good.

use std::path::Path;

use iced::widget::column;
use iced::Element;
use ncrs_i18n::Msg;
use ncrs_ui_kit::confirm::{confirm_dialog, Confirm, ConfirmButton};
use ncrs_ui_kit::confirm_list::{confirm_list, ConfirmLine, ConfirmSum};
use ncrs_ui_kit::format::{shown_path, size_shown};
use ncrs_ui_kit::frame::DialogSize;
use ncrs_ui_kit::kit::{self, Check, Tone};
use ncrs_ui_kit::size_row::CheckTone;
use ncrs_ui_kit::theme::{self, size, spacing};

use super::list::group_color;
use crate::confirm::{ConfirmKind, ConfirmState};
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::rows::DisplayRow;
use crate::shortcuts::Shortcut;
use crate::state::State;
use crate::texts::{fill_count_size, fill_plural, fill_size, row_check, row_name};

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// One line per thing that goes, as the list of the tab names it.
fn lines(ask: &ConfirmState, state: &State, ctx: ViewCtx<'_>) -> Vec<ConfirmLine> {
    let things = ask
        .safe
        .iter()
        .map(|entry| (entry.path.as_path(), entry.size.bytes))
        .chain(
            ask.worktrees_taken()
                .iter()
                .map(|worktree| (worktree.path.as_path(), worktree.size.bytes)),
        );
    let biggest = things.clone().map(|(_, bytes)| bytes).max().unwrap_or(0);
    things
        .map(|(path, bytes)| {
            let row: Option<&DisplayRow> = state.row_by_path(path);
            let (name, check, color) = match row {
                Some(listed) => (
                    row_name(listed, ctx.lang),
                    row_check(listed, ctx.processes_known, ctx.lang)
                        .filter(|(tone, _)| *tone == CheckTone::Ok)
                        .map(|(_, words)| words),
                    group_color(listed.group, ctx.palette),
                ),
                None => (file_name(path), None, theme::space::safe(ctx.palette)),
            };
            ConfirmLine {
                name,
                path: shown_path(path, ctx.home),
                check,
                size_text: size_shown(bytes, ctx.lang),
                fraction: bytes as f32 / biggest.max(1) as f32,
                color,
            }
        })
        .collect()
}

pub fn dialog<'a>(
    ask: &'a ConfirmState,
    state: &'a State,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let bytes = ask.total_bytes();
    let (title, body) = match ask.kind {
        ConfirmKind::Worktrees => (
            fill_plural(
                Msg::SpaceConfirmWorktreesTitleOne,
                Msg::SpaceConfirmWorktreesTitle,
                ask.count(),
                0,
                ctx.lang,
            ),
            fill_size(Msg::SpaceConfirmWorktreesBody, bytes, ctx.lang),
        ),
        ConfirmKind::Safe | ConfirmKind::One => (
            fill_size(Msg::SpaceConfirmTitle, bytes, ctx.lang),
            fill_plural(
                Msg::SpaceConfirmBodyOne,
                Msg::SpaceConfirmBody,
                ask.count(),
                0,
                ctx.lang,
            ),
        ),
    };
    let sum = ConfirmSum {
        label: ctx
            .lang
            .text(Msg::SpaceConfirmTotal)
            .replace("{size}", "")
            .trim()
            .to_owned(),
        size_text: size_shown(bytes, ctx.lang),
    };
    let mut list = column![confirm_list(
        lines(ask, state, ctx),
        Some(sum),
        size::LIST_BOX_HEIGHT,
        palette,
    )]
    .spacing(spacing::M);
    list = list.push(kit::dim(ctx.lang.text(Msg::SpaceConfirmRecheck), palette));
    if ask.offers_worktrees(ctx.store_build) {
        let worktree_bytes = ask.worktrees.iter().fold(0, |so_far: u64, worktree| {
            so_far.saturating_add(worktree.size.bytes)
        });
        let label = format!(
            "{} {}",
            ctx.lang.text(Msg::SpaceConfirmWithWorktrees),
            fill_count_size(
                Msg::SpaceConfirmWithWorktreesCount,
                ask.worktrees.len(),
                worktree_bytes,
                ctx.lang
            )
        );
        let checked = if ask.with_worktrees {
            Check::On
        } else {
            Check::Off
        };
        list = list.push(kit::labelled_checkbox(
            checked,
            Some(Message::ConfirmWithWorktreesToggled),
            Some(&label),
            palette,
        ));
    }
    if ask.unknown_use {
        list = list.push(kit::warning_line(
            ctx.lang.text(Msg::SpaceConfirmUnknownUse),
            palette,
        ));
    }
    let delete_key = match ask.kind {
        ConfirmKind::Worktrees => Shortcut::RemoveWorktrees,
        ConfirmKind::Safe | ConfirmKind::One => Shortcut::DeleteSafe,
    };
    confirm_dialog(
        Confirm {
            title,
            body,
            list: Some(list.into()),
            cancel: ConfirmButton {
                label: ctx.lang.text(Msg::DialogCancel),
                key: Some(ctx.lang.text(Msg::SpaceKeyNameEscape).to_owned()),
            },
            confirm: ConfirmButton {
                label: &ask.confirm_label,
                key: Some(ctx.keys.label(delete_key)).filter(|label| !label.is_empty()),
            },
            tone: Tone::Danger,
            focus: ask.focus,
            on_cancel: Message::ConfirmCancel,
            on_confirm: Message::ConfirmAccept,
            window: ctx.window,
            lang: ctx.lang,
            size: DialogSize::Large,
            hint: Some(ctx.lang.text(Msg::SpaceConfirmHint).to_owned()),
        },
        palette,
    )
}
