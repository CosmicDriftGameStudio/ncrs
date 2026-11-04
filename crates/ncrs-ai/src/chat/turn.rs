//! What the model is shown of the conversation: the last question of Hermes,
//! the search it showed, and the message.

use std::fmt::Write as _;

use super::intent::{ChatIntent, Schedule, Transfer};
use crate::contract::clean_value;
use crate::error::AiError;

#[derive(Debug, Clone, PartialEq)]
pub struct ChatTurn {
    pub message: String,
    pub question: Option<ChatQuestion>,
    pub previous_search: Option<SearchQuery>,
    /// The compact JSON of the last understood `tidy_folder` or
    /// `change_structure`, see `intent_json`.
    pub previous_request: Option<String>,
    /// What the user typed before, when Hermes could do nothing with it.
    pub previous_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatQuestion {
    pub text: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchQuery {
    pub sender: Option<String>,
    pub kind: Option<String>,
    pub year_from: Option<u16>,
    pub year_to: Option<u16>,
    pub amount_min_cents: Option<i64>,
    pub amount_max_cents: Option<i64>,
    pub text: Option<String>,
}

/// The user turn of the prompt, in the format the model was measured with.
/// An empty message is an error: there is nothing to understand.
pub fn chat_user_message(turn: &ChatTurn) -> Result<String, AiError> {
    let message = clean_value(&turn.message).ok_or(AiError::BadReply("the message is empty"))?;
    let question = turn.question.as_ref().map_or_else(
        || "none".to_owned(),
        |question| {
            let options: Vec<String> = question
                .options
                .iter()
                .map(|option| without_control(option))
                .collect();
            format!(
                "{} Options: {}",
                without_control(&question.text),
                options.join(" | ")
            )
        },
    );
    let previous = turn
        .previous_search
        .as_ref()
        .map_or_else(|| "none".to_owned(), search_json);
    let previous_request = turn
        .previous_request
        .as_deref()
        .map_or_else(|| "none".to_owned(), without_control);
    let previous_message = turn
        .previous_message
        .as_deref()
        .map_or_else(|| "none".to_owned(), without_control);
    Ok(format!(
        "Hermes' last question: {question}\nPrevious search: {previous}\n\
         Previous request: {previous_request}\nPrevious message: {previous_message}\nMessage: {message}"
    ))
}

fn without_control(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// Compact, keys in a fixed order, absent keys left out.
pub fn search_json(query: &SearchQuery) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut text_entry = |key: &str, entry: &Option<String>| {
        if let Some(text) = entry {
            parts.push(format!("\"{key}\":{}", json_string(text)));
        }
    };
    text_entry("sender", &query.sender);
    text_entry("kind", &query.kind);
    let mut number_entry = |key: &str, entry: Option<String>| {
        if let Some(number) = entry {
            parts.push(format!("\"{key}\":{number}"));
        }
    };
    number_entry("year_from", query.year_from.map(|year| year.to_string()));
    number_entry("year_to", query.year_to.map(|year| year.to_string()));
    number_entry("amount_min", query.amount_min_cents.map(amount_json));
    number_entry("amount_max", query.amount_max_cents.map(amount_json));
    if let Some(text) = &query.text {
        parts.push(format!("\"text\":{}", json_string(text)));
    }
    format!("{{{}}}", parts.join(","))
}

/// The request a follow-up can refine: only a tidy or a structure change, in
/// the form the model replies with.
pub fn intent_json(intent: &ChatIntent) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    match intent {
        ChatIntent::TidyFolder {
            source,
            target,
            schedule,
            transfer,
        } => {
            parts.push(format!(
                "\"intent\":\"tidy_folder\",\"source\":{}",
                json_string(source)
            ));
            if let Some(target) = target {
                parts.push(format!("\"target\":{}", json_string(target)));
            }
            if let Some(schedule) = schedule {
                let word = match schedule {
                    Schedule::Once => "once",
                    Schedule::Always => "always",
                };
                parts.push(format!("\"schedule\":\"{word}\""));
            }
            if let Some(transfer) = transfer {
                let word = match transfer {
                    Transfer::Copy => "copy",
                    Transfer::Move => "move",
                };
                parts.push(format!("\"transfer\":\"{word}\""));
            }
        }
        ChatIntent::ChangeStructure {
            area,
            scope,
            order,
            name,
        } => {
            parts.push(format!(
                "\"intent\":\"change_structure\",\"area\":{}",
                json_string(area)
            ));
            if let Some(scope) = scope {
                parts.push(format!("\"scope\":{}", json_string(scope)));
            }
            if let Some(order) = order {
                parts.push(format!("\"order\":\"{}\"", order.word()));
            }
            if let Some(spec) = name {
                let blocks: Vec<String> = spec
                    .blocks
                    .iter()
                    .map(|block| format!("\"{}\"", block.word()))
                    .collect();
                parts.push(format!("\"name\":[{}]", blocks.join(",")));
                parts.push(format!(
                    "\"separator\":{}",
                    json_string(&spec.separator.to_string())
                ));
            }
        }
        _ => return None,
    }
    Some(format!("{{{}}}", parts.join(",")))
}

/// `100` when whole, else `12.5` or `12.34`.
fn amount_json(cents: i64) -> String {
    let (whole, rest) = (cents.div_euclid(100), cents.rem_euclid(100));
    match (rest, rest.rem_euclid(10)) {
        (0, _) => whole.to_string(),
        (_, 0) => format!("{whole}.{}", rest.div_euclid(10)),
        _ => format!("{whole}.{rest:02}"),
    }
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
