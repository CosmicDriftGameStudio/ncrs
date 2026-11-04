//! Why a document waits for a decision, in words.

use ncrs_filing::review_work::TextState;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::{Field, ReviewReason};

/// Why the document is here, in words; the field a reason names is spelled out.
pub fn reason_text(reason: ReviewReason, text_state: TextState, lang: Language) -> String {
    match reason {
        ReviewReason::NoMatchingRule => lang
            .text(match text_state {
                TextState::NotReadable => Msg::ReviewListReasonImageNoText,
                TextState::NoneFound => Msg::ReviewListReasonNoText,
                TextState::Read => Msg::ReviewListReasonNoRule,
            })
            .to_owned(),
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

fn field_name(field: Field) -> Msg {
    match field {
        Field::Sender => Msg::ReviewListFieldSender,
        Field::Recipient => Msg::ReviewListFieldRecipient,
        Field::Kind => Msg::ReviewListFieldKind,
        Field::Date => Msg::ReviewListFieldDate,
        Field::Number => Msg::ReviewListFieldNumber,
        Field::Amount => Msg::ReviewListFieldAmount,
    }
}
