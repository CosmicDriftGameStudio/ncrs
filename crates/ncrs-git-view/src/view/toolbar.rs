//! The buttons of the header: fetch, pull, push, stash and undo, as icons.

use iced::widget::row;
use iced::{alignment, Element};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::kit::{icon_button, Icon, Tone};
use ncrs_ui_kit::theme::spacing;

use crate::actions::SyncBar;
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::shortcuts::Shortcut;

/// The tooltip: what it does and the key that does the same.
pub(in crate::view) fn tip(what: &str, shortcut: Shortcut) -> String {
    format!("{what} ({})", shortcut.label())
}

fn counted(count: u32) -> Option<String> {
    (count > 0).then(|| count.to_string())
}

pub fn view<'a>(sync: &'a SyncBar, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let mut bar = row![
        icon_button(
            Icon::Download,
            None,
            &tip(lang.text(Msg::GitViewToolbarFetch), Shortcut::Fetch),
            Some(Message::Fetch),
            Tone::Secondary,
            palette
        ),
        icon_button(
            Icon::ArrowDownToLine,
            counted(sync.behind).map(|count| format!("\u{2193}{count}")),
            &tip(lang.text(Msg::GitViewToolbarPull), Shortcut::Pull),
            Some(Message::Pull),
            Tone::Secondary,
            palette
        ),
        icon_button(
            Icon::ArrowUpFromLine,
            counted(sync.ahead).map(|count| format!("\u{2191}{count}")),
            &tip(lang.text(Msg::GitViewToolbarPush), Shortcut::Push),
            Some(Message::Push),
            Tone::Secondary,
            palette
        ),
        icon_button(
            Icon::Archive,
            None,
            &tip(lang.text(Msg::GitViewToolbarStash), Shortcut::Stash),
            Some(Message::Stash),
            Tone::Secondary,
            palette
        ),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    if let Some(what) = &sync.undo {
        bar = bar.push(icon_button(
            Icon::Undo,
            None,
            &tip(
                &fill_template(lang.text(Msg::GitViewToolbarUndo), &[("{what}", what)]),
                Shortcut::Undo,
            ),
            Some(Message::Undo),
            Tone::Secondary,
            palette,
        ));
    }
    bar.into()
}
