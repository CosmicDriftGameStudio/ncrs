//! The question before many files of the Lesen folders are processed again.

use iced::widget::column;
use iced::{Element, Size};
use ncrs_config::palette::Palette;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::kit::{self, paragraph, Tone};
use ncrs_ui_kit::theme::spacing;

use crate::message::{HermesFileMessage, Message};

/// Asks about `count` files. Enter confirms, Escape cancels.
pub fn dialog<'a>(
    count: usize,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let title = fill_template(
        lang.text(Msg::StatusHermesConfirmAgainTitle),
        &[("{count}", &count.to_string())],
    );
    let body = column![paragraph(
        lang.text(Msg::StatusHermesConfirmAgainNote),
        palette
    )]
    .spacing(spacing::LINE_GAP);
    let cancel = Message::HermesFile(HermesFileMessage::CancelProcessAgain);
    Frame::new(&title, body, DialogSize::Medium, window, lang, palette)
        .buttons(vec![
            kit::button(
                lang.text(Msg::StatusHermesConfirmAgainNo),
                Some(cancel.clone()),
                Tone::Secondary,
                palette,
            ),
            kit::enter_button(
                lang.text(Msg::StatusHermesConfirmAgainYes),
                Some(Message::HermesFile(HermesFileMessage::ConfirmProcessAgain)),
                Tone::Primary,
                palette,
            ),
        ])
        .on_close(cancel)
        .hints(KeyHints {
            enter: true,
            tab: false,
        })
        .into_element()
}
