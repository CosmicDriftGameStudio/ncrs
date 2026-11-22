//! The buttons of the header: fetch, pull, push, stash and undo, as icons.

use iced::widget::row;
use iced::{alignment, Element};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::kit::{icon_button, Icon, Tone};
use ncrs_ui_kit::theme::spacing;

use crate::actions::{Operation, SyncBar};
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::shortcuts::Shortcut;

/// Shown on the button whose operation is running.
const BUSY_LABEL: &str = "\u{2026}";

/// The tooltip: what it does and the key that does the same.
pub(in crate::view) fn tip(what: &str, shortcut: Shortcut) -> String {
    format!("{what} ({})", shortcut.label())
}

fn counted(count: u32) -> Option<String> {
    (count > 0).then(|| count.to_string())
}

pub fn view<'a>(
    sync: &'a SyncBar,
    busy: Option<Operation>,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    // The button of the running operation shows that and waits.
    let label = |operation: Operation, idle: Option<String>| {
        if busy == Some(operation) {
            Some(BUSY_LABEL.to_owned())
        } else {
            idle
        }
    };
    let press = |operation: Operation, message: Message| (busy != Some(operation)).then_some(message);
    let mut bar = row![
        icon_button(
            Icon::Download,
            label(Operation::Fetch, None),
            &tip(lang.text(Msg::GitViewToolbarFetch), Shortcut::Fetch),
            press(Operation::Fetch, Message::Fetch),
            Tone::Secondary,
            palette
        ),
        icon_button(
            Icon::ArrowDownToLine,
            label(
                Operation::Pull,
                counted(sync.behind).map(|count| format!("\u{2193}{count}"))
            ),
            &tip(lang.text(Msg::GitViewToolbarPull), Shortcut::Pull),
            press(Operation::Pull, Message::Pull),
            Tone::Secondary,
            palette
        ),
        icon_button(
            Icon::ArrowUpFromLine,
            label(
                Operation::Push,
                counted(sync.ahead).map(|count| format!("\u{2191}{count}"))
            ),
            &tip(lang.text(Msg::GitViewToolbarPush), Shortcut::Push),
            press(Operation::Push, Message::Push),
            Tone::Secondary,
            palette
        ),
        icon_button(
            Icon::Archive,
            label(Operation::Stash, None),
            &tip(lang.text(Msg::GitViewToolbarStash), Shortcut::Stash),
            press(Operation::Stash, Message::Stash),
            Tone::Secondary,
            palette
        ),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    if let Some(what) = &sync.undo {
        bar = bar.push(icon_button(
            Icon::Undo,
            label(Operation::Undo, None),
            &tip(
                &fill_template(lang.text(Msg::GitViewToolbarUndo), &[("{what}", what)]),
                Shortcut::Undo,
            ),
            press(Operation::Undo, Message::Undo),
            Tone::Secondary,
            palette,
        ));
    }
    bar.into()
}
