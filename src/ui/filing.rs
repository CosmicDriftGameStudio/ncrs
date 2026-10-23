//! The cards of the filing module: the encryption question, the recovery key,
//! the restore prompts and the plain-file warning.
//!
//! A card is not modal. It sits above the status bar without a scrim, so the
//! panels stay usable while it is up. Pure function, like the other
//! components: module state in, `Element` out.

use iced::widget::text::Wrapping;
use iced::widget::{button, checkbox, column, container, radio, text, text_input, Column, Row};
use iced::{Background, Element, Font, Length, Theme};

use super::theme::{self, font_size, spacing};
use crate::app::fill_template;
use crate::filing::{FilingMessage, FilingModule, KeyError, KeyProblem, Phase, SecretText};
use crate::i18n::{Language, Msg};
use crate::messages::Message;
use crate::palette::Palette;

pub const GROUP_FIELD_ID: &str = "filing-group-field";
pub const KEY_FIELD_ID: &str = "filing-key-field";

const CARD_WIDTH: f32 = 520.0;

/// The card for the module's current phase, or nothing when it has none.
pub fn view<'a>(
    module: &'a FilingModule,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    let body = match module.phase() {
        Phase::AskEncryption {
            encrypted,
            busy,
            error,
        } => ask_encryption(*encrypted, *busy, error.as_deref(), lang, palette),
        Phase::ShowRecoveryKey {
            key,
            group,
            typed,
            mismatch,
        } => show_recovery_key(&key.display_text(), *group, typed, *mismatch, lang, palette),
        Phase::AskRecoveryKey {
            reason,
            typed,
            confirm_replace,
            busy,
            error,
        } => ask_recovery_key(
            *reason,
            typed,
            *confirm_replace,
            *busy,
            error.as_ref(),
            lang,
            palette,
        ),
        Phase::PlainFileWarning => plain_file_warning(lang, palette),
        Phase::Failed(reason) => failed(reason, lang, palette),
        Phase::Starting | Phase::Ready | Phase::Dismissed => return None,
    };
    Some(
        container(body)
            .padding(spacing::OUTER_PADDING * 2.0)
            .width(CARD_WIDTH)
            .style(theme::dialog(palette))
            .into(),
    )
}

fn filing_message(message: FilingMessage) -> Message {
    Message::Filing(message)
}

pub fn title<'a>(label: &'a str, palette: &Palette) -> Element<'a, Message> {
    text(label)
        .size(font_size::TITLE)
        .color(palette.accent)
        .into()
}

pub fn paragraph<'a>(
    content: impl iced::widget::text::IntoFragment<'a>,
    palette: &Palette,
) -> Element<'a, Message> {
    text(content)
        .size(font_size::STATUS)
        .color(palette.text)
        .wrapping(Wrapping::Word)
        .into()
}

pub fn error_line<'a>(content: String, palette: &Palette) -> Element<'a, Message> {
    text(content)
        .size(font_size::STATUS)
        .color(palette.error)
        .wrapping(Wrapping::Word)
        .into()
}

fn action<'a>(
    label: &'a str,
    on_press: Option<FilingMessage>,
    primary: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    button(text(label))
        .on_press_maybe(on_press.map(filing_message))
        .style(theme::dialog_button(palette, primary))
        .into()
}

pub fn buttons<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    Row::with_children(items)
        .spacing(spacing::CELL_PADDING_X)
        .into()
}

pub fn radio_look(palette: &Palette) -> impl Fn(&Theme, radio::Status) -> radio::Style {
    let colors = *palette;
    move |_theme, _status| radio::Style {
        background: Background::Color(colors.panel),
        dot_color: colors.accent,
        border_width: 1.0,
        border_color: colors.border,
        text_color: Some(colors.text),
    }
}

fn ask_encryption<'a>(
    encrypted: bool,
    busy: bool,
    error: Option<&str>,
    lang: Language,
    palette: &Palette,
) -> Column<'a, Message> {
    let choice = |label: Msg, value: bool| {
        radio(lang.text(label), value, Some(encrypted), move |chosen| {
            filing_message(FilingMessage::ChooseEncrypted(chosen))
        })
        .size(14)
        .text_size(font_size::STATUS)
        .style(radio_look(palette))
    };
    let mut body = column![
        title(lang.text(Msg::FilingEncryptionTitle), palette),
        choice(Msg::FilingEncryptionEncrypted, true),
        choice(Msg::FilingEncryptionPlain, false),
        paragraph(lang.text(Msg::FilingEncryptionNoteFiles), palette),
        paragraph(lang.text(Msg::FilingEncryptionNoteIndex), palette),
        paragraph(lang.text(Msg::FilingEncryptionNoteVault), palette),
    ]
    .spacing(spacing::SECTION_GAP);
    if let Some(reason) = error {
        body = body.push(error_line(
            fill_template(lang.text(Msg::FilingCreateFailed), &[("{reason}", reason)]),
            palette,
        ));
    }
    body.push(buttons(vec![
        action(
            lang.text(Msg::FilingEncryptionCreate),
            (!busy).then_some(FilingMessage::CreateIndex),
            true,
            palette,
        ),
        action(
            lang.text(Msg::FilingNotNow),
            (!busy).then_some(FilingMessage::Dismiss),
            false,
            palette,
        ),
    ]))
}

fn show_recovery_key<'a>(
    key_text: &str,
    group: usize,
    typed: &SecretText,
    mismatch: bool,
    lang: Language,
    palette: &Palette,
) -> Column<'a, Message> {
    let group_number = (group + 1).to_string();
    let field = text_input("", typed.as_str())
        .id(GROUP_FIELD_ID)
        .on_input(|input| filing_message(FilingMessage::GroupInput(SecretText::from(input))))
        .on_submit(filing_message(FilingMessage::GroupSubmit))
        .width(160);
    let mut body = column![
        title(lang.text(Msg::FilingKeyTitle), palette),
        paragraph(lang.text(Msg::FilingKeyExplain), palette),
        Row::with_children([
            text(key_text.to_owned())
                .font(Font::MONOSPACE)
                .size(font_size::TITLE)
                .color(palette.accent)
                .wrapping(Wrapping::Word)
                .width(Length::Fill)
                .into(),
            action(
                lang.text(Msg::FilingKeyCopy),
                Some(FilingMessage::CopyRecoveryKey),
                false,
                palette,
            ),
        ])
        .spacing(spacing::CELL_PADDING_X),
        paragraph(
            fill_template(
                lang.text(Msg::FilingKeyGroupPrompt),
                &[("{group}", &group_number)],
            ),
            palette,
        ),
        field,
    ]
    .spacing(spacing::SECTION_GAP);
    if mismatch {
        body = body.push(error_line(
            fill_template(
                lang.text(Msg::FilingKeyGroupWrong),
                &[("{group}", &group_number)],
            ),
            palette,
        ));
    }
    body.push(buttons(vec![action(
        lang.text(Msg::FilingKeyContinue),
        Some(FilingMessage::GroupSubmit),
        true,
        palette,
    )]))
}

fn key_error_text(error: &KeyError, lang: Language) -> String {
    match error {
        KeyError::InvalidFormat => lang.text(Msg::FilingErrorKeyFormat).to_owned(),
        KeyError::DoesNotOpen => lang.text(Msg::FilingErrorKeyDoesNotOpen).to_owned(),
        KeyError::ConfirmFirst => lang.text(Msg::FilingErrorConfirmFirst).to_owned(),
        KeyError::Other(reason) => {
            fill_template(lang.text(Msg::FilingErrorKeyOther), &[("{reason}", reason)])
        }
    }
}

fn ask_recovery_key<'a>(
    reason: KeyProblem,
    typed: &SecretText,
    confirm_replace: bool,
    busy: bool,
    error: Option<&KeyError>,
    lang: Language,
    palette: &Palette,
) -> Column<'a, Message> {
    let (explanation, submit_label) = match reason {
        KeyProblem::Missing => (Msg::FilingRestoreMissing, Msg::FilingRestoreButton),
        KeyProblem::Mismatch => (Msg::FilingRestoreMismatch, Msg::FilingReplaceButton),
    };
    let field = text_input(lang.text(Msg::FilingRestoreField), typed.as_str())
        .id(KEY_FIELD_ID)
        .on_input(|input| filing_message(FilingMessage::KeyInput(SecretText::from(input))))
        .on_submit(filing_message(FilingMessage::KeySubmit))
        .width(Length::Fill);
    let mut body = column![
        title(lang.text(Msg::FilingRestoreTitle), palette),
        paragraph(lang.text(explanation), palette),
    ]
    .spacing(spacing::SECTION_GAP);
    if reason == KeyProblem::Mismatch {
        body = body.push(
            Row::with_children([
                checkbox(confirm_replace)
                    .on_toggle(|confirmed| filing_message(FilingMessage::ConfirmReplace(confirmed)))
                    .style(theme::dialog_checkbox(palette))
                    .into(),
                text(lang.text(Msg::FilingRestoreReplaceCheckbox))
                    .size(font_size::STATUS)
                    .into(),
            ])
            .spacing(spacing::CELL_PADDING_X),
        );
    }
    body = body.push(field);
    if let Some(failure) = error {
        body = body.push(error_line(key_error_text(failure, lang), palette));
    }
    body.push(buttons(vec![
        action(
            lang.text(submit_label),
            (!busy).then_some(FilingMessage::KeySubmit),
            true,
            palette,
        ),
        action(
            lang.text(Msg::FilingNotNow),
            (!busy).then_some(FilingMessage::Dismiss),
            false,
            palette,
        ),
    ]))
}

fn plain_file_warning<'a>(lang: Language, palette: &Palette) -> Column<'a, Message> {
    column![
        title(lang.text(Msg::FilingPlainTitle), palette),
        text(lang.text(Msg::FilingPlainWarning))
            .size(font_size::STATUS)
            .color(palette.error)
            .wrapping(Wrapping::Word),
        buttons(vec![
            action(
                lang.text(Msg::FilingPlainUse),
                Some(FilingMessage::ConfirmPlainFile),
                false,
                palette,
            ),
            action(
                lang.text(Msg::FilingNotNow),
                Some(FilingMessage::Dismiss),
                true,
                palette,
            ),
        ]),
    ]
    .spacing(spacing::SECTION_GAP)
}

fn failed<'a>(reason: &str, lang: Language, palette: &Palette) -> Column<'a, Message> {
    let line = if reason == crate::filing::NO_DATA_DIRECTORY {
        lang.text(Msg::FilingFailedNoDataDir).to_owned()
    } else {
        fill_template(lang.text(Msg::FilingFailedReason), &[("{reason}", reason)])
    };
    column![
        title(lang.text(Msg::FilingFailedTitle), palette),
        error_line(line, palette),
        buttons(vec![action(
            lang.text(Msg::FilingFailedClose),
            Some(FilingMessage::Dismiss),
            true,
            palette,
        )]),
    ]
    .spacing(spacing::SECTION_GAP)
}
