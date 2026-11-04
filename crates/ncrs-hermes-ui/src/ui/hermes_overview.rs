//! What the Index settings page says about the protection of the index, and
//! the question before it is removed.

use iced::widget::column;
use iced::{Element, Size};

use crate::hermes_overview::{ModuleState, OverviewFacts, Protection};
use crate::message::{HermesOverviewMessage, Message};
use ncrs_config::palette::Palette;
use ncrs_filing::filing::{KeyProblem, ProtectionNote};
use ncrs_i18n::fill_template;
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::frame::{DialogSize, Frame, KeyHints};
use ncrs_ui_kit::kit::{self, paragraph, Tone};
use ncrs_ui_kit::theme::spacing;

fn on_this_computer(lang: Language, mac: Msg, other: Msg) -> &'static str {
    lang.text(if cfg!(target_os = "macos") {
        mac
    } else {
        other
    })
}

fn send(message: HermesOverviewMessage) -> Message {
    Message::HermesOverview(message)
}

fn key_sentence(facts: &OverviewFacts, lang: Language) -> &'static str {
    match facts.key_problem {
        Some(KeyProblem::Mismatch) => lang.text(Msg::HermesOverviewKeyMismatch),
        _ => on_this_computer(
            lang,
            Msg::HermesOverviewKeyMissingMac,
            Msg::HermesOverviewKeyMissingOther,
        ),
    }
}

/// How the index is protected, for the settings page: the state word and
/// whether it is a good one, what it means, and why the last change failed.
#[derive(Debug)]
pub struct ProtectionText {
    pub state: Option<&'static str>,
    pub good: bool,
    pub sentence: &'static str,
    pub failure: Option<String>,
}

pub fn protection_text(facts: &OverviewFacts, lang: Language) -> ProtectionText {
    let key_missing = facts.module == ModuleState::KeyMissing;
    let (state, sentence) = match facts.protection {
        Protection::Encrypted if key_missing => (
            Some(Msg::HermesOverviewProtectionEncrypted),
            key_sentence(facts, lang),
        ),
        Protection::Encrypted => (
            Some(Msg::HermesOverviewProtectionEncrypted),
            on_this_computer(
                lang,
                Msg::HermesOverviewProtectionEncryptedMac,
                Msg::HermesOverviewProtectionEncryptedOther,
            ),
        ),
        Protection::Plain => (
            Some(Msg::HermesOverviewProtectionPlain),
            on_this_computer(
                lang,
                Msg::HermesOverviewProtectionPlainMac,
                Msg::HermesOverviewProtectionPlainOther,
            ),
        ),
        Protection::Encrypting => (
            Some(Msg::HermesOverviewProtectionEncrypting),
            lang.text(Msg::HermesOverviewProtectionWorking),
        ),
        Protection::StartingOver => (
            Some(Msg::HermesOverviewProtectionStartingOver),
            lang.text(Msg::HermesOverviewProtectionWorking),
        ),
        Protection::Unknown => (None, lang.text(Msg::HermesOverviewProtectionNone)),
    };
    let failure = facts.protection_note.as_ref().map(|note| {
        let (template, reason) = match note {
            ProtectionNote::EncryptFailed(reason) => (Msg::HermesOverviewEncryptFailed, reason),
            ProtectionNote::StartOverFailed(reason) => (Msg::HermesOverviewStartOverFailed, reason),
            ProtectionNote::KeyEntryNotRemoved(reason) => (Msg::HermesOverviewKeyEntryKept, reason),
        };
        fill_template(lang.text(template), &[("{reason}", reason)])
    });
    ProtectionText {
        state: state.map(|word| lang.text(word)),
        good: facts.protection == Protection::Encrypted && !key_missing,
        sentence,
        failure,
    }
}

/// One thing the user can do about the protection.
#[derive(Debug)]
pub struct ProtectionAction {
    pub label: Msg,
    pub message: HermesOverviewMessage,
    pub enabled: bool,
    pub primary: bool,
}

/// The buttons for the protection. While a change runs they are inactive.
pub fn protection_actions(facts: &OverviewFacts) -> Vec<ProtectionAction> {
    let key_missing = facts.module == ModuleState::KeyMissing;
    let action = |label, message, enabled, primary| ProtectionAction {
        label,
        message,
        enabled,
        primary,
    };
    let mut actions = Vec::new();
    if key_missing {
        actions.push(action(
            Msg::HermesOverviewEnterRecoveryKey,
            HermesOverviewMessage::EnterRecoveryKey,
            facts.can_start_over,
            true,
        ));
        actions.push(action(
            Msg::HermesOverviewTryAgain,
            HermesOverviewMessage::TryAgain,
            facts.can_start_over,
            false,
        ));
    } else if facts.can_encrypt || facts.protection == Protection::Encrypting {
        actions.push(action(
            Msg::HermesOverviewEncryptNow,
            HermesOverviewMessage::EncryptNow,
            facts.can_encrypt,
            false,
        ));
    }
    let changing = matches!(
        facts.protection,
        Protection::Encrypting | Protection::StartingOver
    );
    if facts.can_start_over || key_missing || changing {
        actions.push(action(
            Msg::HermesOverviewStartOver,
            HermesOverviewMessage::AskStartOver,
            facts.can_start_over,
            false,
        ));
    }
    actions
}

/// Buttons: Cancel `0`, "Remove index" `1`.
pub const START_OVER_CANCEL_FOCUS: usize = 0;
pub const START_OVER_CONFIRM_FOCUS: usize = 1;

/// The question before the index and its key are removed. Opens on Cancel,
/// so Enter alone never removes anything.
pub fn start_over_dialog<'a>(
    focus: usize,
    window: Size,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let body = column![
        paragraph(lang.text(Msg::HermesOverviewStartOverLoss), palette),
        paragraph(lang.text(Msg::HermesOverviewStartOverFiles), palette),
    ]
    .spacing(spacing::LINE_GAP);
    let cancel = send(HermesOverviewMessage::CancelStartOver);
    Frame::new(
        lang.text(Msg::HermesOverviewStartOverTitle),
        body,
        DialogSize::Medium,
        window,
        lang,
        palette,
    )
    .buttons(vec![
        kit::button(
            lang.text(Msg::DialogCancel),
            Some(cancel.clone()),
            Tone::filled_if(focus == START_OVER_CANCEL_FOCUS),
            palette,
        ),
        kit::button(
            lang.text(Msg::HermesOverviewStartOverConfirm),
            Some(send(HermesOverviewMessage::ConfirmStartOver)),
            Tone::Danger,
            palette,
        ),
    ])
    .on_close(cancel)
    .hints(KeyHints {
        enter: false,
        tab: true,
    })
    .into_element()
}
