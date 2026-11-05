//! The grammar (llama.cpp GBNF) of the two answers: a whole rule, or the
//! pattern alone. The words in it come from the constants of the rule
//! language, so a new placeholder or condition kind reaches the model without
//! anyone editing a list here.

use ncrs_index::ConditionKind;
use ncrs_intake::FILING_PLACEHOLDERS;

use super::vocabulary::{condition_word, COMBINATORS};

/// A pattern is one to eight segments. A segment is literal text, or
/// placeholders with literal text between them. Every text has exactly one
/// parse (literal text never contains `{`, and two literals never touch):
/// llama.cpp follows every parse of an ambiguous grammar at once, and a
/// grammar that splits literal runs in many ways stalls it.
/// The literal text keeps off the characters a pattern or a file name cannot
/// carry.
const PATTERN_RULES: &str = r#"pattern ::= "\"" segment ("/" segment){0,7} "/"? "\""
segment ::= literal | (literal? placeholder)+ literal?
placeholder ::= @PLACEHOLDERS@
literal ::= literalchar+
literalchar ::= [^/{}\\"<>:*?|\x00-\x1F\x7F]
"#;

const GENERATE_RULES: &str = r#"root ::= "{\"match\":" combinator ",\"conditions\":[" (condition ("," condition){0,5})? "],\"pattern\":" pattern "}"
combinator ::= @COMBINATORS@
condition ::= "{\"kind\":" kindword ",\"value\":" value ",\"not\":" boolean "}"
kindword ::= @KINDS@
value ::= "\"" valuechar{1,80} "\""
valuechar ::= [^"\\\x00-\x1F\x7F] | "\\" ["\\/bfnrt]
boolean ::= "true" | "false"
"#;

const RECOGNIZE_RULES: &str = r#"root ::= "{\"pattern\":" pattern "}"
"#;

fn alternatives<'word>(words: impl IntoIterator<Item = &'word str>) -> String {
    words
        .into_iter()
        .map(|word| format!("\"\\\"{word}\\\"\""))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn pattern_rules() -> String {
    let placeholders = FILING_PLACEHOLDERS
        .iter()
        .map(|name| format!("\"{{{name}}}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    PATTERN_RULES.replace("@PLACEHOLDERS@", &placeholders)
}

/// The whole rule: `{"match":"all","conditions":[...],"pattern":"..."}`.
pub fn generate_grammar() -> String {
    let kinds = alternatives(ConditionKind::ALL.into_iter().map(condition_word));
    let combinators = alternatives(
        COMBINATORS
            .into_iter()
            .map(|combinator| combinator.as_str()),
    );
    let rules = GENERATE_RULES
        .replace("@COMBINATORS@", &combinators)
        .replace("@KINDS@", &kinds);
    format!("{rules}{}", pattern_rules())
}

/// The pattern alone: `{"pattern":"..."}`.
pub fn recognize_grammar() -> String {
    format!("{RECOGNIZE_RULES}{}", pattern_rules())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_come_from_the_constants() {
        let generate = generate_grammar();
        for kind in ConditionKind::ALL {
            assert!(generate.contains(&format!("\\\"{}\\\"", condition_word(kind))));
        }
        for combinator in COMBINATORS {
            assert!(generate.contains(&format!("\\\"{}\\\"", combinator.as_str())));
        }
        for grammar in [generate, recognize_grammar()] {
            for name in FILING_PLACEHOLDERS {
                assert!(grammar.contains(&format!("\"{{{name}}}\"")), "{name}");
            }
            assert!(!grammar.contains('@'));
        }
    }

    #[test]
    fn each_grammar_has_one_root() {
        for grammar in [generate_grammar(), recognize_grammar()] {
            let roots = grammar
                .lines()
                .filter(|line| line.starts_with("root ::="))
                .count();
            assert_eq!(roots, 1);
        }
    }
}
