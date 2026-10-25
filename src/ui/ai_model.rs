//! The card that asks before the AI model is downloaded. Modal. Pure view over
//! [`AiModelCard`]; it shows what would be fetched and from where before
//! anything is.

use iced::widget::text::Wrapping;
use iced::widget::{column, text};
use iced::{Element, Size};
use ncrs_ai::LOCAL_MODEL;

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::kit::{self, dim, error_line, one_line, paragraph, Tone};
use super::theme::{font_size, spacing};
use crate::ai_model::{
    AiModelCard, DownloadError, AI_MODEL_HOST, AI_MODEL_LICENSE, AI_MODEL_PROJECT,
};
use crate::app::fill_template;
use crate::i18n::{Language, Msg};
use crate::messages::{AiModelMessage, Message};
use crate::palette::Palette;

const SIZE: DialogSize = DialogSize::Medium;

/// The buttons from left to right: cancel is `0`, the download button (which
/// stops a running download) is `1`.
pub const CANCEL_FOCUS: usize = 0;
pub const DOWNLOAD_FOCUS: usize = 1;

/// `focus` is the button Enter would press.
pub fn view<'a>(
    card: &AiModelCard,
    focus: usize,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let line_chars = frame::content_chars(SIZE, window, 0.0);
    let size = format::size(LOCAL_MODEL.size);
    let mut body =
        column![paragraph(lang.text(Msg::AiModelIntro), palette)].spacing(spacing::SECTION_GAP);
    body = body
        .push(detail_line(
            fill_template(lang.text(Msg::AiModelHost), &[("{host}", AI_MODEL_HOST)]),
            line_chars,
            palette,
        ))
        .push(detail_line(
            fill_template(
                lang.text(Msg::AiModelFile),
                &[("{name}", LOCAL_MODEL.name), ("{size}", size.as_str())],
            ),
            line_chars,
            palette,
        ))
        .push(
            text(fill_template(
                lang.text(Msg::AiModelChecksum),
                &[("{checksum}", LOCAL_MODEL.sha256)],
            ))
            .size(font_size::BODY)
            .color(palette.dim_text)
            .wrapping(Wrapping::Glyph),
        )
        .push(detail_line(
            fill_template(
                lang.text(Msg::AiModelLicense),
                &[("{license}", AI_MODEL_LICENSE)],
            ),
            line_chars,
            palette,
        ));
    if let Some(target) = card.target() {
        let folder = target.to_string_lossy();
        body = body.push(detail_line(
            fill_template(lang.text(Msg::AiModelTarget), &[("{folder}", &folder)]),
            line_chars,
            palette,
        ));
    }
    body = body.push(paragraph(
        fill_template(
            lang.text(Msg::AiModelSource),
            &[("{project}", AI_MODEL_PROJECT)],
        ),
        palette,
    ));
    if card.is_downloading() {
        let line = if card.is_stopping() {
            Msg::AiModelStopping
        } else {
            Msg::AiModelDownloading
        };
        body = body.push(dim(lang.text(line), palette));
    } else if card.is_paused() {
        body = body.push(dim(lang.text(Msg::AiModelPaused), palette));
    }
    if let Some(error) = card.error() {
        body = body.push(error_line(error_text(error, lang), palette));
    }

    let (action_label, action) = if card.is_downloading() {
        (
            Msg::AiModelStop,
            (!card.is_stopping()).then_some(Message::AiModel(AiModelMessage::Stop)),
        )
    } else if card.continues() {
        (
            Msg::AiModelContinue,
            Some(Message::AiModel(AiModelMessage::Download)),
        )
    } else {
        (
            Msg::AiModelDownload,
            Some(Message::AiModel(AiModelMessage::Download)),
        )
    };
    let cancel = (!card.is_downloading()).then_some(Message::AiModel(AiModelMessage::Close));
    Frame::new(
        lang.text(Msg::AiModelTitle),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::DialogCancel),
            cancel,
            Tone::filled_if(focus == CANCEL_FOCUS),
            palette,
        ),
        kit::button(
            lang.text(action_label),
            action,
            Tone::filled_if(focus == DOWNLOAD_FOCUS),
            palette,
        ),
    ])
    .on_close(Message::AiModel(AiModelMessage::Close))
    // Enter presses the focused button, which starts on Cancel.
    .hints(KeyHints {
        enter: false,
        tab: true,
    })
    .into_element()
}

fn detail_line<'a>(content: String, line_chars: usize, palette: &Palette) -> Element<'a, Message> {
    one_line(format::ellipsize_middle(&content, line_chars), palette)
}

fn error_text(error: &DownloadError, lang: Language) -> String {
    match error {
        DownloadError::NoDataFolder => lang.text(Msg::AiModelErrorNoFolder).to_owned(),
        other => fill_template(
            lang.text(Msg::AiModelError),
            &[("{detail}", other.to_string().as_str())],
        ),
    }
}
