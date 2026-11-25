//! The clean-up dialog: what a fetch showed that can go, what stays and why,
//! and the button that deletes what is ticked.

use std::path::Path;

use iced::widget::{column, container, scrollable};
use iced::{Element, Length, Padding};
use ncrs_git::{CleanupBranch, CleanupReason, KeepReason};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::check_rows::{check_row, worktree_detail, CheckRow, RowOutcome};
use ncrs_ui_kit::frame::{DialogSize, Frame};
use ncrs_ui_kit::kit::{button, dim, enter_button, error_line, labelled_checkbox, Check, Tone};
use ncrs_ui_kit::section_head::{section_head, SectionHead};
use ncrs_ui_kit::theme::spacing;

use crate::cleanup::{CleanupDialog, CleanupMessage, Line, Phase, Pick};
use crate::ctx::ViewCtx;
use crate::message::Message;

/// The height of the list inside the dialog; the rest of it is fixed text.
const LIST_HEIGHT: f32 = 320.0;

fn keep_text(why: KeepReason, lang: Language) -> String {
    let fixed = |msg: Msg| lang.text(msg).to_owned();
    match why {
        KeepReason::DefaultBranch => fixed(Msg::GitCleanupKeepDefault),
        KeepReason::CurrentBranch => fixed(Msg::GitCleanupKeepCurrent),
        KeepReason::Empty => fixed(Msg::GitCleanupKeepEmpty),
        KeepReason::Unpushed { commits: 1 } => fixed(Msg::GitCleanupKeepUnpushedOne),
        KeepReason::Unpushed { commits } => fill_template(
            lang.text(Msg::GitCleanupKeepUnpushedMany),
            &[("{count}", &commits.to_string())],
        ),
        KeepReason::NotMerged => fixed(Msg::GitCleanupKeepNotMerged),
        KeepReason::AfterLastPush => fixed(Msg::GitCleanupKeepAfterLastPush),
        KeepReason::WorktreeChanges { count } => fill_template(
            lang.text(Msg::GitCleanupKeepWorktreeChanges),
            &[("{count}", &count.to_string())],
        ),
        KeepReason::MainWorktree => fixed(Msg::GitCleanupKeepMainWorktree),
        KeepReason::CurrentWorktree => fixed(Msg::GitCleanupKeepCurrentWorktree),
        KeepReason::Detached => fixed(Msg::GitCleanupKeepDetached),
        KeepReason::Locked => fixed(Msg::GitCleanupKeepLocked),
        KeepReason::InKeptWorktree => fixed(Msg::GitCleanupKeepInKeptWorktree),
    }
}

fn offer_text(
    dialog: &CleanupDialog,
    branch: &CleanupBranch,
    worktree: bool,
    lang: Language,
) -> String {
    let base = match (worktree, branch.reason) {
        (true, _) => lang.text(Msg::GitCleanupReasonWorktree).to_owned(),
        (false, CleanupReason::InDefault) => fill_template(
            lang.text(Msg::GitCleanupReasonInMain),
            &[("{main}", &dialog.main)],
        ),
        (false, CleanupReason::UpstreamGone) => lang.text(Msg::GitCleanupReasonGone).to_owned(),
    };
    match (
        &branch.upstream,
        branch.remote_tip.is_some(),
        dialog.delete_remote,
    ) {
        (Some(remote), true, true) => format!("{base} + {remote}"),
        _ => base,
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |last| last.to_string_lossy().into_owned(),
    )
}

fn outcome(dialog: &CleanupDialog, pick: &Pick, ctx: ViewCtx<'_>) -> Option<RowOutcome> {
    Some(match dialog.result_of(pick)? {
        Ok(()) => RowOutcome::Done(ctx.lang.text(Msg::ReposCleanupRowDeleted).to_owned()),
        Err(reason) => RowOutcome::Failed(fill_template(
            ctx.lang.text(Msg::GitCleanupRowFailed),
            &[("{reason}", &reason.to_string())],
        )),
    })
}

fn item_row<'a>(
    dialog: &CleanupDialog,
    index: usize,
    pick: Pick,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let (branch, worktree) = match &pick {
        Pick::Worktree(path) => {
            let offered = dialog
                .list
                .worktrees
                .iter()
                .find(|found| &found.path == path);
            (offered.map(|found| &found.branch), offered)
        }
        Pick::Branch(name) => (
            dialog
                .list
                .branches
                .iter()
                .find(|found| &found.name == name),
            None,
        ),
    };
    let Some(branch) = branch else {
        return column![].into();
    };
    let ready = dialog.phase == Phase::Ready;
    check_row(
        CheckRow {
            check: if dialog.ticked.contains(&pick) {
                Check::On
            } else {
                Check::Off
            },
            name: branch.name.clone(),
            detail: worktree
                .map(|found| worktree_detail(&found.path, &found.ignored_files, ctx.lang)),
            reason: offer_text(dialog, branch, worktree.is_some(), ctx.lang),
            worktree: worktree.is_some(),
            kept: false,
            cursor: dialog.cursor == index,
            on_toggle: ready.then(|| Message::Cleanup(CleanupMessage::Toggle(pick.clone()))),
            outcome: outcome(dialog, &pick, ctx),
        },
        ctx.palette,
    )
}

fn remote_line<'a>(dialog: &CleanupDialog, on_it: bool, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let remote = dialog
        .list
        .branches
        .iter()
        .find_map(|found| {
            found
                .upstream
                .as_deref()?
                .split_once('/')
                .map(|(name, _)| name)
        })
        .unwrap_or("origin");
    let label = format!(
        "{}{}",
        if on_it { "\u{25b8} " } else { "" },
        ctx.lang.text(Msg::ReposCleanupDeleteRemote)
    );
    let sub = fill_template(
        ctx.lang.text(Msg::ReposCleanupDeleteRemoteSub),
        &[
            ("{count}", &dialog.remote_count().to_string()),
            ("{remote}", remote),
        ],
    );
    column![
        labelled_checkbox(
            if dialog.delete_remote {
                Check::On
            } else {
                Check::Off
            },
            (dialog.phase == Phase::Ready)
                .then_some(Message::Cleanup(CleanupMessage::ToggleRemote)),
            Some(&label),
            ctx.palette,
        ),
        dim(sub, ctx.palette),
    ]
    .spacing(spacing::XS)
    .into()
}

fn kept_section<'a>(dialog: &CleanupDialog, ctx: ViewCtx<'a>) -> Option<Element<'a, Message>> {
    if dialog.list.kept.is_empty() {
        return None;
    }
    let mut section = column![section_head(
        SectionHead {
            title: ctx.lang.text(Msg::ReposCleanupKeepTitle),
            count: dialog.list.kept.len(),
            open: dialog.keep_open,
            has_focus: false,
            on_toggle: Message::Cleanup(CleanupMessage::ToggleKeep),
        },
        ctx.palette,
    )];
    if dialog.keep_open {
        for kept in &dialog.list.kept {
            let name = if kept.is_worktree {
                folder_name(Path::new(&kept.name))
            } else {
                kept.name.clone()
            };
            section = section.push(check_row(
                CheckRow {
                    check: Check::Off,
                    name,
                    detail: None,
                    reason: keep_text(kept.why, ctx.lang),
                    worktree: kept.is_worktree,
                    kept: true,
                    cursor: false,
                    on_toggle: None,
                    outcome: None,
                },
                ctx.palette,
            ));
        }
    }
    Some(section.into())
}

pub fn cleanup_dialog<'a>(
    dialog: &'a CleanupDialog,
    name: &str,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let lang = ctx.lang;
    let palette = ctx.palette;
    let mut body = column![].spacing(spacing::M);
    let header = match dialog.phase {
        Phase::Fetching => lang.text(Msg::GitViewCleanupFetching).to_owned(),
        Phase::Deleting => lang.text(Msg::GitViewCleanupDeleting).to_owned(),
        Phase::Done => fill_template(
            lang.text(Msg::ReposCleanupDeleted),
            &[("{count}", &dialog.deleted_count().to_string())],
        ),
        Phase::Ready if dialog.list.cleanable_count() == 0 => {
            lang.text(Msg::GitViewCleanupNothing).to_owned()
        }
        Phase::Ready => fill_template(
            lang.text(Msg::GitViewCleanupSummary),
            &[
                ("{branches}", &dialog.list.branches.len().to_string()),
                ("{worktrees}", &dialog.list.worktrees.len().to_string()),
            ],
        ),
    };
    body = body.push(dim(header, palette));
    if let Some(reason) = &dialog.fetch_error {
        body = body.push(error_line(
            fill_template(
                lang.text(Msg::GitViewCleanupFetchFailed),
                &[("{reason}", reason)],
            ),
            palette,
        ));
    }
    let mut list = column![];
    let picks = dialog.picks();
    for (index, pick) in picks.into_iter().enumerate() {
        list = list.push(item_row(dialog, index, pick, ctx));
    }
    if let Some(kept) = kept_section(dialog, ctx) {
        list = list.push(kept);
    }
    body = body.push(scrollable(list).height(Length::Fixed(LIST_HEIGHT)));
    if dialog.remote_count() > 0 {
        let on_it = dialog.line_at_cursor() == Some(Line::Remote);
        body = body.push(remote_line(dialog, on_it, ctx));
    }
    body = body.push(dim(lang.text(Msg::ReposCleanupUndoHint), palette));
    let content: Element<'a, Message> = container(body)
        .padding(Padding::from([spacing::M, 0.0]))
        .into();
    let chosen = dialog.ticked.len();
    let buttons = match dialog.phase {
        Phase::Done => vec![enter_button(
            lang.text(Msg::GitViewCleanupClose),
            Some(Message::CloseDialog),
            Tone::Primary,
            palette,
        )],
        phase => vec![
            button(
                lang.text(Msg::GitViewDialogCancel),
                Some(Message::CloseDialog),
                Tone::Secondary,
                palette,
            ),
            enter_button(
                &fill_template(
                    lang.text(Msg::GitViewCleanupConfirm),
                    &[("{count}", &chosen.to_string())],
                ),
                (phase == Phase::Ready && chosen > 0)
                    .then_some(Message::Cleanup(CleanupMessage::Confirm)),
                Tone::Danger,
                palette,
            ),
        ],
    };
    let title = fill_template(lang.text(Msg::GitViewCleanupTitle), &[("{name}", name)]);
    Frame::new(
        &title,
        content,
        DialogSize::Large,
        ctx.window,
        lang,
        palette,
    )
    .buttons(buttons)
    .on_close(Message::CloseDialog)
    .into_element()
}
