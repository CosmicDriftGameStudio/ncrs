//! The card that fetches a color theme from Open VSX. Modal, like the command
//! palette. Pure view over [`ThemeFetch`].

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, text, text_input, Column};
use iced::{alignment, Element, Length, Size};

use super::filing::{error_line, paragraph};
use super::format;
use super::frame::{self, DialogSize, Frame, KeyHints};
use super::layout::ROW_HEIGHT;
use super::theme::{self, font_size, spacing};
use crate::app::fill_template;
use crate::i18n::{Language, Msg};
use crate::messages::{Message, ThemeFetchMessage};
use crate::open_vsx::FetchError;
use crate::palette::Palette;
use crate::theme_fetch::{Stage, ThemeFetch};
use crate::vsix::Brightness;

pub const FIELD_ID: &str = "theme-fetch-field";

/// Most themes listed at once; the list follows the cursor beyond that.
pub const VISIBLE_THEMES: usize = 10;

/// Characters of a row kept free for "Dark" or "Light" at its right.
const THEME_MARKER_CHARS: usize = 8;

const SIZE: DialogSize = DialogSize::Medium;

fn to_message(message: ThemeFetchMessage) -> Message {
    Message::ThemeFetch(message)
}

pub fn view<'a>(
    card: &'a ThemeFetch,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let line_chars = frame::content_chars(SIZE, window, 0.0);
    let mut body = column![].spacing(spacing::SECTION_GAP);
    match card.stage() {
        Stage::Choosing { fetched, cursor } => {
            let extension = &fetched.extension;
            let extension_line = fill_template(
                lang.text(Msg::ThemeFetchExtension),
                &[
                    ("{name}", extension.display_name.as_str()),
                    ("{version}", extension.version.as_str()),
                ],
            );
            body = body
                .push(
                    text(format::ellipsize_end(&extension_line, line_chars))
                        .size(font_size::STATUS)
                        .color(palette.text)
                        .wrapping(Wrapping::None),
                )
                .push(paragraph(
                    license_line(extension.license.as_deref(), lang),
                    palette,
                ))
                .push(dim(lang.text(Msg::ThemeFetchChooseHint), palette))
                .push(theme_list(card, *cursor, line_chars, lang, palette));
        }
        Stage::Input | Stage::Fetching | Stage::Installing => {
            body = body
                .push(field(card, lang, palette))
                .push(dim(lang.text(Msg::ThemeFetchHelp), palette));
        }
    }

    match card.stage() {
        Stage::Fetching => body = body.push(dim(lang.text(Msg::ThemeFetchFetching), palette)),
        Stage::Installing => body = body.push(dim(lang.text(Msg::ThemeFetchInstalling), palette)),
        Stage::Input | Stage::Choosing { .. } => {}
    }
    if let Some(error) = card.error() {
        body = body.push(error_line(error_text(error, lang), palette));
    }

    let choosing = matches!(card.stage(), Stage::Choosing { .. });
    Frame::new(
        lang.text(Msg::ThemeFetchTitle),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .buttons(footer_buttons(card, lang, palette))
    .on_close(to_message(ThemeFetchMessage::Close))
    // Choosing, Enter takes the highlighted theme: the content defines it.
    .hints(KeyHints {
        enter: !choosing,
        tab: false,
    })
    .into_element()
}

fn dim<'a>(content: &'a str, palette: &Palette) -> Element<'a, Message> {
    text(content)
        .size(font_size::STATUS)
        .color(palette.dim_text)
        .wrapping(Wrapping::Word)
        .into()
}

fn field<'a>(card: &'a ThemeFetch, lang: Language, palette: &Palette) -> Element<'a, Message> {
    let input = text_input(lang.text(Msg::ThemeFetchPlaceholder), card.text())
        .id(FIELD_ID)
        .on_submit(to_message(ThemeFetchMessage::Activate))
        .size(font_size::ROW)
        .padding([3.0, 6.0])
        .style(theme::search_input(palette))
        .width(Length::Fill);
    // Without `on_input` the field is disabled: nothing is typed while the
    // download runs.
    if matches!(card.stage(), Stage::Input) {
        input
            .on_input(|text| to_message(ThemeFetchMessage::TextChanged(text)))
            .into()
    } else {
        input.into()
    }
}

fn license_line(license: Option<&str>, lang: Language) -> String {
    let shown = license.unwrap_or_else(|| lang.text(Msg::ThemeFetchLicenseNotStated));
    fill_template(lang.text(Msg::ThemeFetchLicense), &[("{license}", shown)])
}

fn theme_list<'a>(
    card: &'a ThemeFetch,
    cursor: usize,
    line_chars: usize,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let Stage::Choosing { fetched, .. } = card.stage() else {
        return Column::new().into();
    };
    let first = (cursor + 1).saturating_sub(VISIBLE_THEMES);
    let rows = fetched
        .themes
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_THEMES)
        .map(|(index, contributed)| {
            let selected = index == cursor;
            let (label_color, marker_color) = if selected {
                (palette.cursor_text, palette.cursor_text)
            } else {
                (palette.text, palette.dim_text)
            };
            let marker = match contributed.brightness {
                Some(Brightness::Dark) => lang.text(Msg::ThemeFetchDark),
                Some(Brightness::Light) => lang.text(Msg::ThemeFetchLight),
                None => "",
            };
            let content = row![
                container(
                    text(format::ellipsize_end(
                        &contributed.label,
                        line_chars.saturating_sub(THEME_MARKER_CHARS)
                    ))
                    .size(font_size::ROW)
                    .color(label_color)
                    .wrapping(Wrapping::None)
                )
                .width(Length::Fill),
                text(marker).size(font_size::ROW).color(marker_color),
            ]
            .align_y(alignment::Vertical::Center);
            button(content)
                .on_press(to_message(ThemeFetchMessage::Pick(index)))
                .width(Length::Fill)
                .padding([0.0, spacing::CELL_PADDING_X])
                .height(ROW_HEIGHT)
                .style(theme::row(palette, false, selected, true))
                .into()
        });
    container(Column::with_children(rows))
        .height(ROW_HEIGHT * fetched.themes.len().min(VISIBLE_THEMES) as f32)
        .clip(true)
        .into()
}

fn footer_buttons<'a>(
    card: &ThemeFetch,
    lang: Language,
    palette: &Palette,
) -> Vec<Element<'a, Message>> {
    let action = |label: &'a str, on_press: Option<ThemeFetchMessage>, primary: bool| {
        frame::action(label, on_press.map(to_message), primary, palette)
    };
    let mut items = vec![action(
        lang.text(Msg::ThemeFetchCancel),
        // Closing while the theme is written would orphan the config change.
        (!matches!(card.stage(), Stage::Installing)).then_some(ThemeFetchMessage::Close),
        false,
    )];
    if !matches!(card.stage(), Stage::Choosing { .. }) {
        items.push(action(
            lang.text(Msg::ThemeFetchFetch),
            matches!(card.stage(), Stage::Input).then_some(ThemeFetchMessage::Activate),
            true,
        ));
    }
    items
}

/// The words for what went wrong. The technical detail, where there is one,
/// stays English.
pub fn error_text(error: &FetchError, lang: Language) -> String {
    let with = |label: Msg, values: &[(&str, &str)]| fill_template(lang.text(label), values);
    match error {
        FetchError::InvalidId => lang.text(Msg::ThemeFetchErrorInvalidId).to_owned(),
        FetchError::NotFound(id) => with(Msg::ThemeFetchErrorNotFound, &[("{id}", id)]),
        FetchError::Status(code) => with(
            Msg::ThemeFetchErrorStatus,
            &[("{code}", code.to_string().as_str())],
        ),
        FetchError::NoConnection(detail) => {
            with(Msg::ThemeFetchErrorNoConnection, &[("{detail}", detail)])
        }
        FetchError::Timeout => lang.text(Msg::ThemeFetchErrorTimeout).to_owned(),
        FetchError::Tls(detail) => with(Msg::ThemeFetchErrorTls, &[("{detail}", detail)]),
        FetchError::Network(detail) => with(Msg::ThemeFetchErrorNetwork, &[("{detail}", detail)]),
        FetchError::BadAnswer(detail) => {
            with(Msg::ThemeFetchErrorBadAnswer, &[("{detail}", detail)])
        }
        FetchError::NoDownloadLink => lang.text(Msg::ThemeFetchErrorNoDownload).to_owned(),
        FetchError::DownloadRefused { host } => {
            with(Msg::ThemeFetchErrorRefused, &[("{host}", host)])
        }
        FetchError::TooManyRedirects => lang.text(Msg::ThemeFetchErrorRedirects).to_owned(),
        FetchError::TooLarge => lang.text(Msg::ThemeFetchErrorTooLarge).to_owned(),
        FetchError::BadPackage(detail) => {
            with(Msg::ThemeFetchErrorBadPackage, &[("{detail}", detail)])
        }
        FetchError::NoThemes => lang.text(Msg::ThemeFetchErrorNoThemes).to_owned(),
        FetchError::MissingFile(path) => with(Msg::ThemeFetchErrorMissingFile, &[("{path}", path)]),
        FetchError::UnsafePath(path) => with(Msg::ThemeFetchErrorUnsafePath, &[("{path}", path)]),
        FetchError::FileTooLarge(path) => {
            with(Msg::ThemeFetchErrorFileTooLarge, &[("{path}", path)])
        }
        FetchError::BadJson { file, reason } => with(
            Msg::ThemeFetchErrorBadJson,
            &[("{path}", file), ("{detail}", reason)],
        ),
        FetchError::NoConfigPath => lang.text(Msg::ThemeFetchErrorNoConfigPath).to_owned(),
        FetchError::Config(config) => with(
            Msg::ThemeFetchErrorWrite,
            &[
                ("{path}", config.path().display().to_string().as_str()),
                ("{reason}", config.reason()),
            ],
        ),
        FetchError::WriteFailed { path, reason } => with(
            Msg::ThemeFetchErrorWrite,
            &[("{path}", path), ("{reason}", reason)],
        ),
    }
}
