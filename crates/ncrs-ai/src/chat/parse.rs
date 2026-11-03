//! The one place where the reply of the model becomes a `ChatIntent`. The
//! grammar already keeps the shape; this checks the values again, because a
//! build without the grammar (a fake, a future provider) must not widen them.

use jsonc_parser::{parse_to_value, JsonObject, JsonValue, ParseOptions};
use ncrs_index::Currency;

use super::intent::{ChatIntent, Order, QuestionKind, Schedule, Transfer};
use super::turn::{ChatTurn, SearchQuery};
use crate::contract::{clean_value, text_has_minor_units};
use crate::error::AiError;

const MAX_FIELD_CHARS: usize = 80;
const YEARS: std::ops::RangeInclusive<u16> = 1000..=2999;

pub fn parse_chat_reply(raw: &str, turn: &ChatTurn) -> Result<ChatIntent, AiError> {
    let parsed = parse_to_value(raw, &ParseOptions::default())
        .map_err(|_syntax| AiError::BadReply("not valid JSON"))?;
    let Some(JsonValue::Object(reply)) = parsed else {
        return Err(AiError::BadReply("not a JSON object"));
    };
    let name = reply
        .get_string("intent")
        .ok_or(AiError::BadReply("no intent"))?;
    match name.as_ref() {
        "tidy_folder" => Ok(ChatIntent::TidyFolder {
            source: required_text(&reply, "source")?,
            target: text(&reply, "target"),
            schedule: enum_of(&reply, "schedule", schedule_of),
            transfer: enum_of(&reply, "transfer", transfer_of),
        }),
        "change_structure" => Ok(ChatIntent::ChangeStructure {
            area: required_text(&reply, "area")?,
            scope: text(&reply, "scope"),
            order: reply
                .get_string("order")
                .and_then(|word| order_of(word))
                .ok_or(AiError::BadReply("no order"))?,
        }),
        "undo" => Ok(ChatIntent::Undo),
        "search" => Ok(ChatIntent::Search(SearchQuery {
            sender: text(&reply, "sender"),
            kind: text(&reply, "kind"),
            year_from: year(&reply, "year_from"),
            year_to: year(&reply, "year_to"),
            amount_min_cents: cents(&reply, "amount_min", &turn.message),
            amount_max_cents: cents(&reply, "amount_max", &turn.message),
            text: text(&reply, "text"),
        })),
        "ask_content" => Ok(ChatIntent::AskContent {
            question_kind: reply
                .get_string("question_kind")
                .and_then(|word| question_kind_of(word))
                .ok_or(AiError::BadReply("no question kind"))?,
            sender: text(&reply, "sender"),
            kind: text(&reply, "kind"),
        }),
        "explain" => Ok(ChatIntent::Explain {
            file: required_text(&reply, "file")?,
        }),
        "bulk_move" => Ok(ChatIntent::BulkMove {
            filter_kind: text(&reply, "filter_kind"),
            filter_sender: text(&reply, "filter_sender"),
            filter_year: year(&reply, "filter_year"),
            target: required_text(&reply, "target")?,
        }),
        "answer" => {
            let choice = reply
                .get_string("choice")
                .ok_or(AiError::BadReply("no choice"))?;
            turn.question
                .iter()
                .flat_map(|question| &question.options)
                .position(|option| option == choice.as_ref())
                .map(|index| ChatIntent::Answer { choice: index })
                .ok_or(AiError::BadReply("the choice is not one of the options"))
        }
        "smalltalk" => Ok(ChatIntent::SmallTalk),
        "leave_as_is" => Ok(ChatIntent::LeaveAsIs),
        "other" => Ok(ChatIntent::Other),
        _ => Err(AiError::BadReply("unknown intent")),
    }
}

fn text(reply: &JsonObject<'_>, key: &str) -> Option<String> {
    let cleaned = clean_value(reply.get_string(key)?)?;
    (cleaned.chars().count() <= MAX_FIELD_CHARS).then_some(cleaned)
}

fn required_text(reply: &JsonObject<'_>, key: &'static str) -> Result<String, AiError> {
    text(reply, key).ok_or(AiError::BadReply("a required field is missing"))
}

fn year(reply: &JsonObject<'_>, key: &str) -> Option<u16> {
    let value = reply.get_number(key)?.parse::<f64>().ok()?;
    if value.fract() != 0.0 {
        return None;
    }
    let year = u16::try_from(value as i64).ok()?;
    YEARS.contains(&year).then_some(year)
}

/// Half away from zero; zero or less is no amount. An amount the typed
/// sentence does not contain is dropped: a model invents numbers.
fn cents(reply: &JsonObject<'_>, key: &str, typed: &str) -> Option<i64> {
    let value = reply.get_number(key)?.parse::<f64>().ok()?;
    let rounded = (value * 100.0).round();
    let cents = (1.0..1e15).contains(&rounded).then_some(rounded as i64)?;
    // The sentence names no currency; any one reads its numbers the same.
    let currency = Currency::new("EUR")?;
    text_has_minor_units(typed, cents, &currency).then_some(cents)
}

fn enum_of<T>(reply: &JsonObject<'_>, key: &str, read: fn(&str) -> Option<T>) -> Option<T> {
    read(reply.get_string(key)?)
}

fn schedule_of(word: &str) -> Option<Schedule> {
    match word {
        "once" => Some(Schedule::Once),
        "always" => Some(Schedule::Always),
        _ => None,
    }
}

fn transfer_of(word: &str) -> Option<Transfer> {
    match word {
        "copy" => Some(Transfer::Copy),
        "move" => Some(Transfer::Move),
        _ => None,
    }
}

fn order_of(word: &str) -> Option<Order> {
    match word {
        "by_month" => Some(Order::ByMonth),
        "by_year" => Some(Order::ByYear),
        "by_sender" => Some(Order::BySender),
        "by_kind" => Some(Order::ByKind),
        "flat" => Some(Order::Flat),
        _ => None,
    }
}

fn question_kind_of(word: &str) -> Option<QuestionKind> {
    match word {
        "sum" => Some(QuestionKind::Sum),
        "deadline" => Some(QuestionKind::Deadline),
        "number" => Some(QuestionKind::Number),
        "date" => Some(QuestionKind::Date),
        _ => None,
    }
}
