//! The cards of the filing module: the encryption question, the recovery key,
//! the restore prompts and the plain-file warning.
//!
//! A card sits on the scrim like every dialog, centered; the panels keep their
//! keys. Pure function, like the other components: module state in,
//! `Element` out.

use iced::widget::span;
use iced::widget::text::Span;
use iced::widget::text::Wrapping;
use iced::widget::{checkbox, column, container, radio, rich_text, text, Column, Row};
use iced::{Background, Color, Element, Font, Length, Size, Theme};

use super::frame::{DialogSize, Frame, KeyHints};
use super::kit::{self, error_line, paragraph, Tone};
use super::theme::{self, font_size, spacing};
use crate::app::fill_template;
use crate::filing::{FilingMessage, FilingModule, KeyError, KeyProblem, Phase, SecretText};
use crate::i18n::{Language, Msg};
use crate::messages::Message;
use crate::palette::Palette;

pub const GROUP_FIELD_ID: &str = "filing-group-field";
pub const KEY_FIELD_ID: &str = "filing-key-field";

const SIZE: DialogSize = DialogSize::Medium;

/// Four characters of the key and a little room.
const GROUP_FIELD_WIDTH: f32 = 160.0;

/// What a phase puts into the frame.
struct Card<'a> {
    title: &'a str,
    body: Column<'a, Message>,
    buttons: Vec<Element<'a, Message>>,
    /// A text field of the card takes Enter and submits it.
    enter_submits: bool,
}

/// The card for the module's current phase, or nothing when it has none.
pub fn view<'a>(
    module: &'a FilingModule,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    let card = match module.phase() {
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
        Phase::Starting | Phase::AskEncryption { .. } | Phase::Ready | Phase::Dismissed => {
            return None
        }
    };
    let mut framed = Frame::new(card.title, card.body, SIZE, window, lang, palette)
        .buttons(card.buttons)
        .hints(KeyHints {
            enter: card.enter_submits,
            tab: false,
        });
    // The cross and Esc leave the card the way "Not now" does; they are absent
    // where that is refused.
    if module.card_can_be_dismissed() {
        framed = framed.on_close(filing_message(FilingMessage::Dismiss));
    }
    Some(framed.into_element())
}

fn filing_message(message: FilingMessage) -> Message {
    Message::Filing(message)
}

fn action<'a>(
    label: &'a str,
    on_press: Option<FilingMessage>,
    primary: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    if primary {
        kit::enter_button(label, on_press.map(filing_message), Tone::Primary, palette)
    } else {
        kit::button(
            label,
            on_press.map(filing_message),
            Tone::Secondary,
            palette,
        )
    }
}

fn quiet_action<'a>(
    label: &'a str,
    on_press: Option<FilingMessage>,
    primary: bool,
    palette: &Palette,
) -> Element<'a, Message> {
    kit::button(
        label,
        on_press.map(filing_message),
        Tone::filled_if(primary),
        palette,
    )
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

/// The key split into its blocks and the dashes between them. The block the
/// user has to type back gets the accent color, everything else the plain text
/// color.
fn key_parts(key_text: &str, group: usize, palette: &Palette) -> Vec<(String, Color)> {
    let mut parts = Vec::new();
    for (index, block) in key_text.split('-').enumerate() {
        if index > 0 {
            parts.push(("-".to_owned(), palette.text));
        }
        let color = if index == group {
            palette.accent
        } else {
            palette.text
        };
        parts.push((block.to_owned(), color));
    }
    parts
}

fn key_text_view<'a>(key_text: &str, group: usize, palette: &Palette) -> Element<'a, Message> {
    let spans: Vec<Span<'a, (), Font>> = key_parts(key_text, group, palette)
        .into_iter()
        .map(|(part, color)| span(part).color(color))
        .collect();
    rich_text(spans)
        .font(Font::MONOSPACE)
        .size(font_size::TITLE)
        .wrapping(Wrapping::Word)
        .width(Length::Fill)
        .into()
}

fn show_recovery_key<'a>(
    key_text: &str,
    group: usize,
    typed: &SecretText,
    mismatch: bool,
    lang: Language,
    palette: &Palette,
) -> Card<'a> {
    let group_number = (group + 1).to_string();
    let field = container(kit::text_field(
        "",
        typed.as_str(),
        GROUP_FIELD_ID,
        Some(|input| filing_message(FilingMessage::GroupInput(SecretText::from(input)))),
        filing_message(FilingMessage::GroupSubmit),
        palette,
    ))
    .width(GROUP_FIELD_WIDTH);
    let mut body = column![
        paragraph(lang.text(Msg::FilingKeyExplain), palette),
        Row::with_children([
            key_text_view(key_text, group, palette),
            action(
                lang.text(Msg::FilingKeyCopy),
                Some(FilingMessage::CopyRecoveryKey),
                false,
                palette,
            ),
        ])
        .spacing(spacing::M),
        paragraph(
            fill_template(
                lang.text(Msg::FilingKeyGroupPrompt),
                &[("{group}", &group_number)],
            ),
            palette,
        ),
        field,
    ]
    .spacing(spacing::LINE_GAP);
    if mismatch {
        body = body.push(error_line(
            fill_template(
                lang.text(Msg::FilingKeyGroupWrong),
                &[("{group}", &group_number)],
            ),
            palette,
        ));
    }
    Card {
        title: lang.text(Msg::FilingKeyTitle),
        body,
        buttons: vec![action(
            lang.text(Msg::FilingKeyContinue),
            Some(FilingMessage::GroupSubmit),
            true,
            palette,
        )],
        enter_submits: true,
    }
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
) -> Card<'a> {
    let (explanation, submit_label) = match reason {
        KeyProblem::Missing => (Msg::FilingRestoreMissing, Msg::FilingRestoreButton),
        KeyProblem::Mismatch => (Msg::FilingRestoreMismatch, Msg::FilingReplaceButton),
    };
    let field = kit::text_field(
        lang.text(Msg::FilingRestoreField),
        typed.as_str(),
        KEY_FIELD_ID,
        Some(|input| filing_message(FilingMessage::KeyInput(SecretText::from(input)))),
        filing_message(FilingMessage::KeySubmit),
        palette,
    );
    let mut body = column![paragraph(lang.text(explanation), palette)].spacing(spacing::LINE_GAP);
    if reason == KeyProblem::Mismatch {
        body = body.push(
            Row::with_children([
                checkbox(confirm_replace)
                    .on_toggle(|confirmed| filing_message(FilingMessage::ConfirmReplace(confirmed)))
                    .style(theme::dialog_checkbox(palette))
                    .into(),
                text(lang.text(Msg::FilingRestoreReplaceCheckbox))
                    .size(font_size::BODY)
                    .into(),
            ])
            .spacing(spacing::CELL_PADDING_X),
        );
    }
    body = body.push(field);
    if let Some(failure) = error {
        body = body.push(error_line(key_error_text(failure, lang), palette));
    }
    Card {
        title: lang.text(Msg::FilingRestoreTitle),
        body,
        buttons: vec![
            action(
                lang.text(Msg::FilingNotNow),
                (!busy).then_some(FilingMessage::Dismiss),
                false,
                palette,
            ),
            action(
                lang.text(submit_label),
                (!busy).then_some(FilingMessage::KeySubmit),
                true,
                palette,
            ),
        ],
        enter_submits: true,
    }
}

fn plain_file_warning<'a>(lang: Language, palette: &Palette) -> Card<'a> {
    Card {
        title: lang.text(Msg::FilingPlainTitle),
        body: column![text(lang.text(Msg::FilingPlainWarning))
            .size(font_size::BODY)
            .color(palette.error)
            .wrapping(Wrapping::Word)]
        .spacing(spacing::LINE_GAP),
        buttons: vec![
            quiet_action(
                lang.text(Msg::FilingPlainUse),
                Some(FilingMessage::ConfirmPlainFile),
                false,
                palette,
            ),
            quiet_action(
                lang.text(Msg::FilingNotNow),
                Some(FilingMessage::Dismiss),
                true,
                palette,
            ),
        ],
        enter_submits: false,
    }
}

fn failed<'a>(reason: &str, lang: Language, palette: &Palette) -> Card<'a> {
    let line = if reason == crate::filing::NO_DATA_DIRECTORY {
        lang.text(Msg::FilingFailedNoDataDir).to_owned()
    } else {
        fill_template(lang.text(Msg::FilingFailedReason), &[("{reason}", reason)])
    };
    Card {
        title: lang.text(Msg::FilingFailedTitle),
        body: column![error_line(line, palette)].spacing(spacing::LINE_GAP),
        buttons: vec![action(
            lang.text(Msg::FilingFailedClose),
            Some(FilingMessage::Dismiss),
            true,
            palette,
        )],
        enter_submits: false,
    }
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::ThemePreset;

    #[test]
    fn only_the_four_characters_of_the_requested_block_are_highlighted() {
        let palette = ThemePreset::Cosmic.palette();
        let key = "AAAA-BBBB-CCCC-DDDD";
        for group in 0..4 {
            let parts = key_parts(key, group, &palette);
            let texts: Vec<&str> = parts.iter().map(|(part, _)| part.as_str()).collect();
            assert_eq!(texts, ["AAAA", "-", "BBBB", "-", "CCCC", "-", "DDDD"]);
            let highlighted: Vec<&str> = parts
                .iter()
                .filter(|(_, color)| *color == palette.accent)
                .map(|(part, _)| part.as_str())
                .collect();
            assert_eq!(highlighted, [key.split('-').nth(group).unwrap()]);
        }
    }

    #[test]
    fn the_blocks_of_the_dummy_key_are_pairwise_different() {
        let shown = crate::filing::FilingModule::varied_recovery_key().display_text();
        let blocks: Vec<&str> = shown.split('-').collect();
        assert_eq!(blocks.len(), 13);
        let distinct: std::collections::HashSet<&str> = blocks.iter().copied().collect();
        assert_eq!(distinct.len(), blocks.len(), "{shown}");
    }

    #[test]
    fn the_parts_join_to_the_key_unchanged() {
        let palette = ThemePreset::Cosmic.palette();
        let shown = crate::filing::FilingModule::varied_recovery_key().display_text();
        let joined: String = key_parts(&shown, 7, &palette)
            .into_iter()
            .map(|(part, _)| part)
            .collect();
        assert_eq!(joined, shown);
    }
}
