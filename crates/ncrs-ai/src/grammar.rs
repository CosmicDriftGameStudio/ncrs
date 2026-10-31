//! The grammar (llama.cpp GBNF) the built-in model must write its reply in:
//! one JSON object with the open fields only, each as `{"value", "confidence"}`.
//! Every token the model can choose is then part of such a reply.

use ncrs_index::Field;
use ncrs_intake::FIELD_ORDER;

use crate::contract::MAX_VALUE_CHARS;

/// Whitespace is allowed but bounded, so the model cannot spin on it.
const SHARED_RULES: &str = r#"ws ::= [ \t\n]{0,2}
num ::= "0" ("." [0-9]{1,4})? | "1" ("." "0"{1,4})?
char ::= [^"\\\x7F\x00-\x1F] | "\\" (["\\/bfnrt] | "u" [0-9a-fA-F]{4})
"#;

/// Fields come in the order of `FIELD_ORDER`, each at most once and in any
/// subset, so the object may also be empty. Rule names are numbers because a
/// field name is not necessarily a legal rule name.
pub fn reply_grammar(open_fields: &[Field]) -> String {
    let fields: Vec<Field> = FIELD_ORDER
        .into_iter()
        .filter(|field| open_fields.contains(field))
        .collect();
    let count = fields.len();
    let mut grammar = String::from("root ::= \"{\" ws ");
    if count > 0 {
        grammar.push_str("first0 ");
    }
    grammar.push_str("\"}\"\n");
    for (index, field) in fields.iter().enumerate() {
        let name = field.as_str();
        let next = index + 1;
        grammar.push_str(&format!(
            "kv{index} ::= \"\\\"{name}\\\"\" ws \":\" ws \"{{\" ws \
             \"\\\"value\\\"\" ws \":\" ws string ws \",\" ws \
             \"\\\"confidence\\\"\" ws \":\" ws num ws \"}}\" ws\n\
             first{index} ::= kv{index} rest{next} | first{next}\n\
             rest{index} ::= \",\" ws kv{index} rest{next} | rest{next}\n"
        ));
    }
    if count > 0 {
        grammar.push_str(&format!("first{count} ::= \"\"\nrest{count} ::= \"\"\n"));
    }
    grammar.push_str(&format!(
        "string ::= \"\\\"\" char{{0,{MAX_VALUE_CHARS}}} \"\\\"\"\n"
    ));
    grammar.push_str(SHARED_RULES);
    grammar
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn the_grammar_names_the_open_fields_and_no_other() {
        let grammar = reply_grammar(&[Field::Number, Field::Date]);

        assert!(grammar.contains("\\\"number\\\""));
        assert!(grammar.contains("\\\"date\\\""));
        for other in [Field::Sender, Field::Kind, Field::Amount] {
            assert!(
                !grammar.contains(&format!("\\\"{}\\\"", other.as_str())),
                "{}",
                other.as_str()
            );
        }
    }

    #[test]
    fn the_fields_come_in_field_order_whatever_the_order_asked() {
        let grammar = reply_grammar(&[Field::Amount, Field::Sender]);

        let sender = grammar.find("\\\"sender\\\"").unwrap();
        let amount = grammar.find("\\\"amount\\\"").unwrap();
        assert!(sender < amount);
    }

    #[test]
    fn an_object_without_fields_is_allowed() {
        assert!(reply_grammar(&[Field::Sender]).contains("first1 ::= \"\""));
        assert!(reply_grammar(&[]).starts_with("root ::= \"{\" ws \"}\"\n"));
    }

    #[test]
    fn every_rule_that_is_used_is_defined() {
        let grammar = reply_grammar(&FIELD_ORDER);

        for rule in ["first5", "rest5", "kv4", "string", "num", "char", "ws"] {
            assert!(grammar.contains(&format!("{rule} ::=")), "{rule}");
        }
        assert!(grammar.contains(&format!("char{{0,{MAX_VALUE_CHARS}}}")));
    }
}
