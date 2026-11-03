//! The reply grammar of one turn: the intents as JSON objects, and, when
//! Hermes asked a question, an answer that is one of its options.

use super::turn::ChatTurn;

const BASE_RULES: &str = include_str!("grammar.gbnf");
const INTENT_ROOTS: &str =
    "tidy | change | undo | search | ask | explain | bulk | smalltalk | leave | other";

/// An option the grammar can spell without escaping.
fn is_literal_safe(option: &str) -> bool {
    !option.is_empty()
        && !option
            .chars()
            .any(|character| character == '"' || character == '\\' || character.is_control())
}

pub fn chat_grammar(turn: &ChatTurn) -> String {
    let choices: Vec<String> = turn
        .question
        .iter()
        .flat_map(|question| &question.options)
        .filter(|option| is_literal_safe(option))
        .map(|option| format!("\"\\\"{option}\\\"\""))
        .collect();
    if choices.is_empty() {
        return format!("root ::= {INTENT_ROOTS}\n{BASE_RULES}");
    }
    format!(
        "root ::= {INTENT_ROOTS} | answer\nchoice ::= {}\n\
         answer ::= \"{{\\\"intent\\\":\\\"answer\\\",\\\"choice\\\":\" choice \"}}\"\n{BASE_RULES}",
        choices.join(" | ")
    )
}
