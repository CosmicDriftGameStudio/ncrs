//! The words of the tab, in the user's language.

use std::path::Path;

use iced::Color;
use ncrs_filing::sorting::{ReadProblem, WaitReason};
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::{Field, TargetId};
use ncrs_ui_kit::hermes_state::HermesFileState;
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::theme::hermes_state_color;

use crate::state::Session;

pub fn say(lang: Language, msg: Msg, values: &[(&str, &str)]) -> String {
    let named: Vec<(String, &str)> = values
        .iter()
        .map(|(name, value)| (format!("{{{name}}}"), *value))
        .collect();
    let borrowed: Vec<(&str, &str)> = named
        .iter()
        .map(|(name, value)| (name.as_str(), *value))
        .collect();
    fill_template(lang.text(msg), &borrowed)
}

/// The text with the one figure it has.
pub fn count(lang: Language, msg: Msg, count: usize) -> String {
    say(lang, msg, &[("count", &count.to_string())])
}

/// What waits for the user.
pub fn open_color(palette: &Palette) -> Color {
    hermes_state_color(palette, HermesFileState::Inquiry)
}

pub fn wait_reason(reason: WaitReason) -> Msg {
    match reason {
        WaitReason::Missing => Msg::FilingViewWaitMissing,
        WaitReason::NotMounted => Msg::FilingViewWaitNotMounted,
        WaitReason::NotAccessible => Msg::FilingViewWaitNotAccessible,
        WaitReason::Unconfirmed => Msg::FilingViewWaitUnconfirmed,
        WaitReason::Paused => Msg::FilingViewWaitPaused,
    }
}

pub fn read_problem(lang: Language, problem: &ReadProblem) -> String {
    match problem {
        ReadProblem::Unknown => lang.text(Msg::FilingViewReasonUnknown).to_owned(),
        ReadProblem::StillWriting => lang.text(Msg::FilingViewReasonStillWriting).to_owned(),
        ReadProblem::Unsupported => lang.text(Msg::FilingViewReasonUnsupported).to_owned(),
        ReadProblem::LibraryMissing => lang.text(Msg::FilingViewReasonLibraryMissing).to_owned(),
        ReadProblem::Failed(reason) => reason.clone(),
    }
}

pub fn field_name(field: Field) -> Msg {
    match field {
        Field::Sender => Msg::ReviewListFieldSender,
        Field::Recipient => Msg::ReviewListFieldRecipient,
        Field::Kind => Msg::ReviewListFieldKind,
        Field::Date => Msg::ReviewListFieldDate,
        Field::Number => Msg::ReviewListFieldNumber,
        Field::Amount => Msg::ReviewListFieldAmount,
    }
}

/// "T1", "T1 and T2", "T1, T2 and T3".
pub fn target_list<'a>(
    lang: Language,
    session: &Session,
    ids: impl Iterator<Item = &'a TargetId>,
) -> String {
    let mut numbers: Vec<String> = ids.map(|id| session.target_number(id)).collect();
    match (numbers.pop(), numbers.is_empty()) {
        (None, _) => String::new(),
        (Some(only), true) => only,
        (Some(last), false) => say(
            lang,
            Msg::SortingTargetsAnd,
            &[("first", &numbers.join(", ")), ("second", &last)],
        ),
    }
}

/// `path` below the target `id`, for an absolute `path`.
pub fn below_target(session: &Session, id: &TargetId, path: &Path) -> String {
    session
        .targets
        .iter()
        .find(|target| target.id == *id)
        .and_then(|target| path.strip_prefix(&target.path).ok())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// The room a half's header leaves besides its text: padding, the checkbox
/// and the gaps.
pub const HEADER_MARGIN: f32 = 48.0;
