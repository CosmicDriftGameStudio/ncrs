//! The buttons of the header: fetch, pull, push, a new branch, stash and
//! undo, with their words. Where the actions are off they stay and are
//! locked; the tooltip says why.

use iced::widget::row;
use iced::{alignment, Element};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::kit::{toolbar_button, Tone};
use ncrs_ui_kit::split_button::split_button;
use ncrs_ui_kit::theme::spacing;

use crate::cleanup::CleanupMessage;
use crate::actions::{MenuAction, Operation, SyncBar, Unavailable};
use crate::ctx::ViewCtx;
use crate::git_keys::GitKeys;
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::State;
use crate::sync_menu::SyncKind;

/// Shown on the button whose operation is running.
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

fn counted(word: &str, glyph: &str, count: u32) -> String {
    if count > 0 {
        format!("{word} {glyph}{count}")
    } else {
        word.to_owned()
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
    let word = |operation: Operation, idle_word: String| {
        if busy == Some(operation) {
            BUSY_LABEL.to_owned()
        } else {
            idle_word
        }
    };
    let action = |operation: Option<Operation>,
                  label: String,
                  what: Msg,
                  shortcut: Shortcut,
                  message: Message,
                  tone: Tone| {
        let enabled = locked_reason.is_none() && operation.is_none_or(|op| busy != Some(op));
        let shown = match operation {
            Some(op) => word(op, label),
            None => label,
        };
        let help = match locked_reason {
            Some(reason) => lang.text(reason).to_owned(),
            None => tip(lang.text(what), shortcut, ctx.keys),
        };
        toolbar_button(
            &shown,
            None,
            &help,
            enabled.then_some(message),
            tone,
            palette,
        )
    };
    let pull_tone = if sync.behind > 0 {
        Tone::Primary
    } else {
        Tone::Secondary
    };
    let split = |operation: Operation,
                 kind: SyncKind,
                 label: String,
                 what: Msg,
                 shortcut: Shortcut,
                 message: Message,
                 tone: Tone| {
        let enabled = locked_reason.is_none() && busy != Some(operation);
        let help = match locked_reason {
            Some(reason) => lang.text(reason).to_owned(),
            None => tip(lang.text(what), shortcut, ctx.keys),
        };
        let open = state
            .actions
            .sync_menu
            .filter(|menu| menu.kind == kind)
            .map(|menu| super::sync_menu::rows(&menu, sync.force_blocked, ctx));
        split_button(
            &word(operation, label),
            &help,
            enabled.then_some(message),
            enabled.then_some(Message::OpenSyncMenu(kind)),
            open,
            tone,
            palette,
        )
    };
    let pull_label = counted(lang.text(Msg::GitViewToolbarPull), "\u{2193}", sync.behind);
    let push_label = counted(lang.text(Msg::GitViewToolbarPush), "\u{2191}", sync.ahead);
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
            lang.text(Msg::GitViewToolbarFetch).to_owned(),
            Msg::GitViewToolbarFetch,
            Shortcut::Fetch,
            Message::Fetch,
            Tone::Secondary
        ),
        split(
            Operation::Pull,
            SyncKind::Pull,
            pull_label,
            Msg::GitViewToolbarPull,
            Shortcut::Pull,
            Message::Pull,
            pull_tone
        ),
        split(
            Operation::Push,
            SyncKind::Push,
            push_label,
            Msg::GitViewToolbarPush,
            Shortcut::Push,
            Message::Push,
            Tone::Secondary
        ),
        action(
            None,
            lang.text(Msg::GitViewToolbarBranch).to_owned(),
            Msg::GitViewToolbarBranch,
            Shortcut::NewBranch,
            Message::MenuPressed(MenuAction::NewBranchFrom),
            Tone::Secondary
        ),
        action(
            Some(Operation::Stash),
            lang.text(Msg::GitViewToolbarStash).to_owned(),
            Msg::GitViewToolbarStash,
            Shortcut::Stash,
            Message::Stash,
            Tone::Secondary
        ),
        toolbar_button(
            &word(
                Operation::Undo,
                lang.text(Msg::GitViewToolbarUndoLabel).to_owned()
            ),
            None,
            &undo_help,
            undo_enabled.then_some(Message::Undo),
            Tone::Secondary,
            palette
        ),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    let cleanable = state.actions.cleanable.len();
    if cleanable > 0 {
        bar = bar.push(action(
            None,
            fill_template(
                lang.text(Msg::GitViewToolbarCleanup),
                &[("{count}", &cleanable.to_string())],
            ),
            Msg::GitViewCommandCleanup,
            Shortcut::CleanUp,
            Message::Cleanup(CleanupMessage::Open),
            Tone::Secondary,
        ));
    }
    bar.into()
}
