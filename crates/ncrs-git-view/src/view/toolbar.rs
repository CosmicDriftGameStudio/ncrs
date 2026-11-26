//! The buttons of the header: fetch, pull, push, a new branch, stash and
//! undo, as their icons with the words in the tooltips. Where the actions are off they stay and are
//! locked; the tooltip says why.

use iced::widget::row;
use iced::{alignment, Element};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::kit::{icon_button, GitAction, Tone};
use ncrs_ui_kit::split_button::split_button;
use ncrs_ui_kit::theme::spacing;

use crate::actions::{MenuAction, Operation, SyncBar, Unavailable};
use crate::cleanup::CleanupMessage;
use crate::ctx::ViewCtx;
use crate::git_keys::GitKeys;
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::State;
use crate::sync_menu::SyncKind;

/// Shown beside the icon of the button whose operation is running.
const BUSY_LABEL: &str = "\u{2026}";

/// The tooltip: what it does and the key that does the same.
pub(in crate::view) fn tip(what: &str, shortcut: Shortcut, keys: &GitKeys) -> String {
    let key = shortcut.label(keys);
    if key.is_empty() {
        what.to_owned()
    } else {
        format!("{what} ({key})")
    }
}

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let busy = state.actions.busy;
    let idle = SyncBar::default();
    let sync = state.actions.sync.as_ref().unwrap_or(&idle);
    let locked_reason = state.actions.unavailable.map(|reason| match reason {
        Unavailable::GitMissing => Msg::GitViewHintGitMissing,
        Unavailable::Sandbox => Msg::GitViewHintSandbox,
    });
    let help = |what: &str, shortcut: Shortcut| match locked_reason {
        Some(reason) => lang.text(reason).to_owned(),
        None => tip(what, shortcut, ctx.keys),
    };
    // The count beside the icon, or the dots while it runs.
    let beside = |operation: Option<Operation>, count: usize| {
        if operation.is_some() && busy == operation {
            Some(BUSY_LABEL.to_owned())
        } else {
            (count > 0).then(|| count.to_string())
        }
    };
    let enabled = |operation: Option<Operation>| {
        locked_reason.is_none() && operation.is_none_or(|op| busy != Some(op))
    };
    let action = |operation: Option<Operation>,
                  git_action: GitAction,
                  what: Msg,
                  shortcut: Shortcut,
                  message: Message| {
        icon_button(
            git_action.icon(),
            beside(operation, 0),
            &help(lang.text(what), shortcut),
            enabled(operation).then_some(message),
            Tone::Secondary,
            palette,
        )
    };
    let split = |operation: Operation,
                 kind: SyncKind,
                 git_action: GitAction,
                 count: u32,
                 what: Msg,
                 shortcut: Shortcut,
                 message: Message| {
        let tone = if kind == SyncKind::Pull && count > 0 {
            Tone::Primary
        } else {
            Tone::Secondary
        };
        let shown_help = help(lang.text(what), shortcut);
        let is_enabled = enabled(Some(operation));
        let open = state
            .actions
            .sync_menu
            .filter(|menu| menu.kind == kind)
            .map(|menu| super::sync_menu::rows(&menu, sync.force_blocked, ctx));
        split_button(
            icon_button(
                git_action.icon(),
                beside(Some(operation), usize::try_from(count).unwrap_or(usize::MAX)),
                &shown_help,
                is_enabled.then_some(message),
                tone,
                palette,
            ),
            &shown_help,
            is_enabled.then_some(Message::OpenSyncMenu(kind)),
            open,
            tone,
            palette,
        )
    };
    let undo_help = match (&sync.undo, locked_reason) {
        (_, Some(reason)) => lang.text(reason).to_owned(),
        (Some(what), None) => tip(
            &fill_template(lang.text(Msg::GitViewToolbarUndo), &[("{what}", what)]),
            Shortcut::Undo,
            ctx.keys,
        ),
        (None, None) => lang.text(Msg::GitViewToolbarNothingToUndo).to_owned(),
    };
    let undo_enabled =
        locked_reason.is_none() && sync.undo.is_some() && busy != Some(Operation::Undo);
    let mut bar = row![
        action(
            Some(Operation::Fetch),
            GitAction::Fetch,
            Msg::GitViewToolbarFetch,
            Shortcut::Fetch,
            Message::Fetch,
        ),
        split(
            Operation::Pull,
            SyncKind::Pull,
            GitAction::Pull,
            sync.behind,
            Msg::GitViewToolbarPull,
            Shortcut::Pull,
            Message::Pull,
        ),
        split(
            Operation::Push,
            SyncKind::Push,
            GitAction::Push,
            sync.ahead,
            Msg::GitViewToolbarPush,
            Shortcut::Push,
            Message::Push,
        ),
        action(
            None,
            GitAction::NewBranch,
            Msg::GitViewToolbarBranch,
            Shortcut::NewBranch,
            Message::MenuPressed(MenuAction::NewBranchFrom),
        ),
        action(
            Some(Operation::Stash),
            GitAction::Stash,
            Msg::GitViewToolbarStash,
            Shortcut::Stash,
            Message::Stash,
        ),
        icon_button(
            GitAction::Undo.icon(),
            beside(Some(Operation::Undo), 0),
            &undo_help,
            undo_enabled.then_some(Message::Undo),
            Tone::Secondary,
            palette,
        ),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    let cleanable = state.actions.cleanable.len();
    if cleanable > 0 {
        bar = bar.push(icon_button(
            GitAction::CleanUp.icon(),
            Some(cleanable.to_string()),
            &help(lang.text(Msg::GitViewCommandCleanup), Shortcut::CleanUp),
            enabled(None).then_some(Message::Cleanup(CleanupMessage::Open)),
            Tone::Secondary,
            palette,
        ));
    }
    bar.into()
}
