//! Scoring of the measured sentences, as the probe that measured them did.

use super::eval::{EvalRow, Expect};
use super::intent::{ChatIntent, Order, QuestionKind, Schedule, Transfer};
use super::turn::ChatTurn;

pub enum Got {
    Text(String),
    Number(f64),
}

/// The name of the intent and its fields as the rows spell them.
pub fn describe(intent: &ChatIntent, turn: &ChatTurn) -> (&'static str, Vec<(&'static str, Got)>) {
    let mut fields: Vec<(&'static str, Got)> = Vec::new();
    let mut text = |key: &'static str, value: &Option<String>| {
        if let Some(value) = value {
            fields.push((key, Got::Text(value.clone())));
        }
    };
    match intent {
        ChatIntent::TidyFolder {
            source,
            target,
            schedule,
            transfer,
        } => {
            text("source", &Some(source.clone()));
            text("target", target);
            text(
                "schedule",
                &schedule.map(|schedule| match schedule {
                    Schedule::Once => "once".to_owned(),
                    Schedule::Always => "always".to_owned(),
                }),
            );
            text(
                "transfer",
                &transfer.map(|transfer| match transfer {
                    Transfer::Copy => "copy".to_owned(),
                    Transfer::Move => "move".to_owned(),
                }),
            );
            ("tidy_folder", fields)
        }
        ChatIntent::ChangeStructure { area, scope, order } => {
            text("area", &Some(area.clone()));
            text("scope", scope);
            let order = match order {
                Order::ByMonth => "by_month",
                Order::ByYear => "by_year",
                Order::BySender => "by_sender",
                Order::ByKind => "by_kind",
                Order::Flat => "flat",
            };
            text("order", &Some(order.to_owned()));
            ("change_structure", fields)
        }
        ChatIntent::Undo => ("undo", fields),
        ChatIntent::Search(query) => {
            text("sender", &query.sender);
            text("kind", &query.kind);
            text("text", &query.text);
            let numbers = [
                ("year_from", query.year_from.map(f64::from)),
                ("year_to", query.year_to.map(f64::from)),
                (
                    "amount_min",
                    query.amount_min_cents.map(|c| c as f64 / 100.0),
                ),
                (
                    "amount_max",
                    query.amount_max_cents.map(|c| c as f64 / 100.0),
                ),
            ];
            for (key, value) in numbers {
                if let Some(value) = value {
                    fields.push((key, Got::Number(value)));
                }
            }
            ("search", fields)
        }
        ChatIntent::AskContent {
            question_kind,
            sender,
            kind,
        } => {
            let word = match question_kind {
                QuestionKind::Sum => "sum",
                QuestionKind::Deadline => "deadline",
                QuestionKind::Number => "number",
                QuestionKind::Date => "date",
            };
            text("question_kind", &Some(word.to_owned()));
            text("sender", sender);
            text("kind", kind);
            ("ask_content", fields)
        }
        ChatIntent::Explain { file } => {
            text("file", &Some(file.clone()));
            ("explain", fields)
        }
        ChatIntent::BulkMove {
            filter_kind,
            filter_sender,
            filter_year,
            target,
        } => {
            text("filter_kind", filter_kind);
            text("filter_sender", filter_sender);
            text("target", &Some(target.clone()));
            if let Some(year) = filter_year {
                fields.push(("filter_year", Got::Number(f64::from(*year))));
            }
            ("bulk_move", fields)
        }
        ChatIntent::Answer { choice } => {
            let option = turn
                .question
                .as_ref()
                .and_then(|question| question.options.get(*choice))
                .cloned();
            text("choice", &option);
            ("answer", fields)
        }
        ChatIntent::Other => ("other", fields),
    }
}

/// (same intent, same intent and every expected field).
pub fn score(row: &EvalRow, intent: &ChatIntent) -> (bool, bool) {
    let (name, fields) = describe(intent, &row.turn());
    if name != row.intent {
        return (false, false);
    }
    let all_match = row.expected.iter().all(|(key, expect)| {
        let got = fields
            .iter()
            .find(|(field, _)| field == key)
            .map(|(_, got)| got);
        match (expect, got) {
            (Expect::Text(wanted), Some(Got::Text(got))) => {
                if is_exact_field(key) {
                    wanted.iter().any(|want| normalise(want) == normalise(got))
                } else {
                    wanted.iter().any(|want| text_matches(want, got))
                }
            }
            (Expect::Number(want), Some(Got::Number(got))) => (got - want).abs() < 0.005,
            _ => false,
        }
    });
    (true, all_match)
}

fn is_exact_field(name: &str) -> bool {
    matches!(
        name,
        "order" | "schedule" | "transfer" | "question_kind" | "choice"
    )
}

pub fn normalise(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'ä' => 'a',
            'ö' => 'o',
            'ü' => 'u',
            other => other,
        })
        .collect()
}

pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let current = row[j + 1];
            row[j + 1] = (previous + usize::from(ca != cb))
                .min(row[j] + 1)
                .min(current + 1);
            previous = current;
        }
    }
    row[b.len()]
}

pub fn text_matches(want: &str, got: &str) -> bool {
    let (want, got) = (normalise(want), normalise(got));
    if want.is_empty() || got.is_empty() {
        return false;
    }
    if want == got {
        return true;
    }
    let shortest = want.len().min(got.len());
    if shortest >= 3 && (want.contains(&got) || got.contains(&want)) {
        return true;
    }
    shortest >= 5 && edit_distance(&want, &got) <= 2
}
