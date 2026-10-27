//! The dialog that asks the App Store version's user to allow access to the
//! home folder. Modal. A pure view: what the buttons do is decided by `App`.

use iced::widget::column;
use iced::{Element, Size};

use super::frame::{DialogSize, Frame, KeyHints};
use super::kit::{self, paragraph, Tone};
use super::theme::spacing;
use crate::i18n::{Language, Msg};
use crate::messages::{GrantMessage, Message};
use crate::palette::Palette;

const SIZE: DialogSize = DialogSize::Medium;

/// The buttons from left to right: "Later" is `0`, "Grant access…" is `1`.
pub const LATER_FOCUS: usize = 0;
pub const GRANT_FOCUS: usize = 1;

/// `focus` is the button Enter would press. The dialog opens on "Grant
/// access…": it only opens a folder dialog and changes nothing by itself.
pub fn view<'a>(
    focus: usize,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let body = column![
        paragraph(lang.text(Msg::GrantIntro), palette),
        paragraph(lang.text(Msg::GrantRemembered), palette),
    ]
    .spacing(spacing::LINE_GAP);
    Frame::new(
        lang.text(Msg::GrantTitle),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::GrantLater),
            Some(Message::Grant(GrantMessage::Later)),
            Tone::filled_if(focus == LATER_FOCUS),
            palette,
        ),
        kit::button(
            lang.text(Msg::GrantChoose),
            Some(Message::Grant(GrantMessage::Choose)),
            Tone::filled_if(focus == GRANT_FOCUS),
            palette,
        ),
    ])
    .on_close(Message::Grant(GrantMessage::Later))
    .hints(KeyHints {
        enter: false,
        tab: true,
    })
    .into_element()
}
