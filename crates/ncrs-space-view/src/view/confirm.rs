//! The question before anything is deleted for good.

use std::path::Path;

use iced::widget::column;
use iced::Element;
use ncrs_i18n::Msg;
use ncrs_ui_kit::confirm::{confirm_dialog, Confirm, ConfirmButton};
use ncrs_ui_kit::format::size_shown;
use ncrs_ui_kit::kit::{self, Check, Tone};
use ncrs_ui_kit::theme::{size, spacing};

use crate::confirm::{ConfirmKind, ConfirmState};
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::texts::{fill_count_size, fill_plural, fill_size};

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// One line per thing that goes: its name and its size.
fn names(ask: &ConfirmState, ctx: ViewCtx<'_>) -> Vec<String> {
    let line = |name: String, bytes: u64| format!("{name}  {}", size_shown(bytes, ctx.lang));
    let safe = ask
        .safe
        .iter()
        .map(|entry| line(file_name(&entry.path), entry.size.bytes));
    let worktrees = ask
        .worktrees_taken()
        .iter()
        .map(|worktree| line(worktree.branch.clone(), worktree.size.bytes));
    safe.chain(worktrees).collect()
}

pub fn dialog<'a>(ask: &'a ConfirmState, ctx: ViewCtx<'a>) -> Element<'a, Message> {
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
    let mut list = column![kit::name_list(
        names(ask, ctx),
        None,
        Some(size::LIST_BOX_HEIGHT),
        palette,
    )]
    .spacing(spacing::M);
    list = list.push(kit::dim_line(
        fill_size(Msg::SpaceConfirmTotal, bytes, ctx.lang),
        palette,
    ));
    if ask.kind == ConfirmKind::Safe && !ctx.store_build && !ask.worktrees.is_empty() {
        let worktree_bytes = ask.worktrees.iter().fold(0_u64, |sum, worktree| {
            sum.saturating_add(worktree.size.bytes)
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
        let state = if ask.with_worktrees {
            Check::On
        } else {
            Check::Off
        };
        list = list.push(kit::labelled_checkbox(
            state,
            Some(Message::ConfirmWithWorktreesToggled),
            Some(&label),
            palette,
        ));
    }
    list = list.push(kit::dim(ctx.lang.text(Msg::SpaceConfirmRecheck), palette));
    if ask.unknown_use {
        list = list.push(kit::warning_line(
            ctx.lang.text(Msg::SpaceConfirmUnknownUse),
            palette,
        ));
    }
    confirm_dialog(
        Confirm {
            title,
            body,
            list: Some(list.into()),
            cancel: ConfirmButton {
                label: ctx.lang.text(Msg::DialogCancel),
                key: Some("Esc"),
            },
            confirm: ConfirmButton {
                label: &ask.confirm_label,
                key: None,
            },
            tone: Tone::Danger,
            focus: ask.focus,
            on_cancel: Message::ConfirmCancel,
            on_confirm: Message::ConfirmAccept,
            window: ctx.window,
            lang: ctx.lang,
        },
        palette,
    )
}
