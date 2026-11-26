//! The measured sentences and how a result is scored, shared by the tests
//! that run them against the real model and the ones that check the format.

use jsonc_parser::{parse_to_value, JsonValue, ParseOptions};

use super::turn::{ChatQuestion, ChatTurn, SearchQuery};

pub const MAIN_ROWS: &str = include_str!("eval/main.jsonl");
pub const HOLDOUT_ROWS: &str = include_str!("eval/holdout.jsonl");

pub enum Expect {
    /// Any of these.
    Text(Vec<String>),
    Number(f64),
}

pub struct EvalRow {
    pub id: i64,
    pub intent: String,
    pub text: String,
    pub expected: Vec<(String, Expect)>,
    pub question: Option<String>,
    pub options: Vec<String>,
    pub previous_search: Option<String>,
}

impl EvalRow {
    pub fn turn(&self) -> ChatTurn {
        ChatTurn {
            message: self.text.clone(),
            question: self.question.as_ref().map(|text| ChatQuestion {
                text: text.clone(),
                options: self.options.clone(),
            }),
            previous_search: self.previous_search.as_deref().map(search_of_json),
        }
    }
}

pub fn rows(raw: &str) -> Vec<EvalRow> {
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .map(row_of_line)
        .collect()
}

fn row_of_line(line: &str) -> EvalRow {
    let Some(JsonValue::Object(row)) = parse_to_value(line, &ParseOptions::default()).unwrap()
    else {
        panic!("row is not an object");
    };
    let expected = row
        .get_object("expected")
        .unwrap()
        .clone()
        .take_inner()
        .into_iter()
        .map(|(key, value)| {
            let expect = match value {
                JsonValue::String(text) => {
                    Expect::Text(text.split('|').map(str::to_owned).collect())
                }
                JsonValue::Number(number) => Expect::Number(number.parse().unwrap()),
                _ => panic!("bad expected value"),
            };
            (key.to_string(), expect)
        })
        .collect();
    let options = row
        .get_array("options")
        .map(|array| {
            array
                .iter()
                .filter_map(|value| match value {
                    JsonValue::String(text) => Some(text.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    EvalRow {
        id: row.get_number("id").and_then(|n| n.parse().ok()).unwrap(),
        intent: row.get_string("intent").unwrap().to_string(),
        text: row.get_string("text").unwrap().to_string(),
        expected,
        question: row.get_string("question").map(ToString::to_string),
        previous_search: row.get_string("previous_search").map(ToString::to_string),
        options,
    }
}

/// Reads the compact JSON of a previous search.
pub fn search_of_json(json: &str) -> SearchQuery {
    let Some(JsonValue::Object(object)) = parse_to_value(json, &ParseOptions::default()).unwrap()
    else {
        panic!("previous search is not an object");
    };
    let text = |key: &str| object.get_string(key).map(ToString::to_string);
    let year = |key: &str| object.get_number(key).map(|n| n.parse().unwrap());
    let cents = |key: &str| {
        object
            .get_number(key)
            .map(|n| (n.parse::<f64>().unwrap() * 100.0).round() as i64)
    };
    SearchQuery {
        sender: text("sender"),
        kind: text("kind"),
        year_from: year("year_from"),
        year_to: year("year_to"),
        amount_min_cents: cents("amount_min"),
        amount_max_cents: cents("amount_max"),
        text: text("text"),
    }
}
