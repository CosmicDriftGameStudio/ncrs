//! Why a document waits for a decision, in words.

use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::{Field, ReviewReason};

/// Why the document is here, as far as the reason alone says it.
pub fn reason_of(reason: ReviewReason, lang: Language) -> String {
    match reason {
        ReviewReason::NoMatchingRule => lang.text(Msg::ReviewListReasonNoRule).to_owned(),
        ReviewReason::NoText => lang.text(Msg::ReviewListReasonNoTextRecognised).to_owned(),
        ReviewReason::SeveralTargets => lang.text(Msg::ReviewListReasonSeveralTargets).to_owned(),
        ReviewReason::Undone => lang.text(Msg::ReviewListReasonUndone).to_owned(),
        ReviewReason::UnconfirmedRule => lang.text(Msg::ReviewListReasonUnconfirmed).to_owned(),
        ReviewReason::TargetExists => lang.text(Msg::ReviewListReasonTargetExists).to_owned(),
        ReviewReason::MissingField(field) => named(Msg::ReviewListReasonMissingField, field, lang),
        ReviewReason::LowConfidence(field) => {
            named(Msg::ReviewListReasonLowConfidence, field, lang)
        }
    }
}

fn named(template: Msg, field: Field, lang: Language) -> String {
    fill_template(
        lang.text(template),
        &[("{field}", lang.text(field_name(field)))],
    )
}

pub(in crate::ui) fn field_name(field: Field) -> Msg {
    match field {
        Field::Sender => Msg::ReviewListFieldSender,
        Field::Recipient => Msg::ReviewListFieldRecipient,
        Field::Kind => Msg::ReviewListFieldKind,
        Field::Date => Msg::ReviewListFieldDate,
        Field::Number => Msg::ReviewListFieldNumber,
        Field::Amount => Msg::ReviewListFieldAmount,
    }
}
