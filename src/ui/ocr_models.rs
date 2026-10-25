//! The card that asks before the models for reading scanned pages are
//! downloaded. Modal. Pure view over [`OcrModelsCard`]; it shows what would be
//! sent and where before anything is.

use iced::widget::text::Wrapping;
use iced::widget::{column, text};
use iced::{Element, Size};

use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::kit::{self, error_line, paragraph, Tone};
use super::theme::{font_size, spacing};
use crate::app::fill_template;
use crate::i18n::{Language, Msg};
use crate::messages::{Message, OcrModelsMessage};
use crate::ocr_models::{DownloadError, OcrModelsCard};
use crate::palette::Palette;
use ncrs_intake::{OCR_MODELS_HOST, OCR_MODELS_PROJECT, OCR_MODEL_FILES};

const SIZE: DialogSize = DialogSize::Medium;

/// The buttons from left to right: cancel is `0`, download is `1`.
pub const CANCEL_FOCUS: usize = 0;
pub const DOWNLOAD_FOCUS: usize = 1;

/// `focus` is the button Enter would press.
pub fn view<'a>(
    card: &OcrModelsCard,
    focus: usize,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let line_chars = frame::content_chars(SIZE, window, 0.0);
    let mut body =
        column![paragraph(lang.text(Msg::OcrModelsIntro), palette)].spacing(spacing::LINE_GAP);
    body = body.push(detail_line(
        fill_template(
            lang.text(Msg::OcrModelsHost),
            &[("{host}", OCR_MODELS_HOST)],
        ),
        line_chars,
        palette,
    ));
    for file in &OCR_MODEL_FILES {
        let size = format::size(file.size);
        body = body
            .push(detail_line(
                fill_template(
                    lang.text(Msg::OcrModelsFile),
                    &[("{name}", file.name), ("{size}", size.as_str())],
                ),
                line_chars,
                palette,
            ))
            .push(
                text(fill_template(
                    lang.text(Msg::OcrModelsChecksum),
                    &[("{checksum}", file.sha256)],
                ))
                .size(font_size::BODY)
                .color(palette.dim_text)
                .wrapping(Wrapping::Glyph),
            );
    }
    if let Some(target) = card.target() {
        let folder = target.to_string_lossy();
        body = body.push(detail_line(
            fill_template(lang.text(Msg::OcrModelsTarget), &[("{folder}", &folder)]),
            line_chars,
            palette,
        ));
    }
    body = body.push(paragraph(
        fill_template(
            lang.text(Msg::OcrModelsSource),
            &[("{project}", OCR_MODELS_PROJECT)],
        ),
        palette,
    ));
    if card.is_downloading() {
        body = body.push(
            text(lang.text(Msg::OcrModelsDownloading))
                .size(font_size::BODY)
                .color(palette.dim_text),
        );
    }
    if let Some(error) = card.error() {
        body = body.push(error_line(error_text(error, lang), palette));
    }

    let download =
        (!card.is_downloading()).then_some(Message::OcrModels(OcrModelsMessage::Download));
    let cancel = (!card.is_downloading()).then_some(Message::OcrModels(OcrModelsMessage::Close));
    Frame::new(
        lang.text(Msg::OcrModelsTitle),
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
            lang.text(Msg::OcrModelsDownload),
            download,
            Tone::filled_if(focus == DOWNLOAD_FOCUS),
            palette,
        ),
    ])
    .on_close(Message::OcrModels(OcrModelsMessage::Close))
    // Enter presses the focused button, which starts on Cancel.
    .hints(KeyHints {
        enter: false,
        tab: true,
    })
    .into_element()
}

fn detail_line<'a>(content: String, line_chars: usize, palette: &Palette) -> Element<'a, Message> {
    text(format::ellipsize_middle(&content, line_chars))
        .size(font_size::BODY)
        .color(palette.text)
        .wrapping(Wrapping::None)
        .into()
}

fn error_text(error: &DownloadError, lang: Language) -> String {
    match error {
        DownloadError::NoDataFolder => lang.text(Msg::OcrModelsErrorNoFolder).to_owned(),
        other => fill_template(
            lang.text(Msg::OcrModelsError),
            &[("{detail}", other.to_string().as_str())],
        ),
    }
}
