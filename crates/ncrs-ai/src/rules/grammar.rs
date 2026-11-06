//! The grammar (llama.cpp GBNF) of the two answers: a whole rule, or the
//! levels alone. The words in it come from the constants of the rule
//! language, so a new placeholder or condition kind reaches the model without
//! anyone editing a list here.

use ncrs_index::ConditionKind;
use ncrs_intake::FILING_PLACEHOLDERS;

use super::levels::SEPARATORS;
use super::vocabulary::{condition_word, COMBINATORS};

/// Up to seven folder levels and a file name level, each of one to four
/// parts: a fixed text or a placeholder name. Every text has exactly one parse
/// (a fixed part starts with `{`, a placeholder with `"`): llama.cpp follows
/// every parse of an ambiguous grammar at once and stalls on one that splits
/// text in many ways.
/// The fixed text keeps off the characters a pattern or a file name cannot
/// carry.
const FREE_LEVELS: &str = r#"levels ::= "\"ordner\":[" (level ("," level){0,6})? "],\"datei\":" (level | "null")
"#;

const LEVEL_RULES: &str = r#"level ::= "{\"teile\":[" part ("," part){0,3} "],\"trenner\":" separator "}"
part ::= fixed | placeholder
fixed ::= "{\"fest\":\"" fixedchar{1,40} "\"}"
fixedchar ::= [^/{}\\"<>:*?|\x00-\x1F\x7F]
placeholder ::= @PLACEHOLDERS@
separator ::= @SEPARATORS@
"#;

const GENERATE_RULES: &str = r#"root ::= "{\"match\":" combinator ",\"conditions\":[" (condition ("," condition){0,5})? "]," levels "}"
combinator ::= @COMBINATORS@
condition ::= "{\"kind\":" kindword ",\"value\":" value ",\"not\":" boolean "}"
kindword ::= @KINDS@
value ::= "\"" valuechar{1,80} "\""
valuechar ::= [^"\\\x00-\x1F\x7F] | "\\" ["\\/bfnrt]
boolean ::= "true" | "false"
"#;

const RECOGNIZE_RULES: &str = r#"root ::= "{" levels "}"
"#;

fn alternatives<'word>(words: impl IntoIterator<Item = &'word str>) -> String {
    words
        .into_iter()
        .map(|word| format!("\"\\\"{word}\\\"\""))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn level_rules() -> String {
    let placeholders = alternatives(FILING_PLACEHOLDERS);
    LEVEL_RULES
        .replace("@PLACEHOLDERS@", &placeholders)
        .replace("@SEPARATORS@", &alternatives(SEPARATORS))
}

/// The whole rule: `{"match":"all","conditions":[...],"ordner":[...],"datei":...}`.
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
    format!("{rules}{FREE_LEVELS}{}", level_rules())
}

/// The levels alone: `{"ordner":[...],"datei":...}`. With `folder_depth` the
/// answer has exactly that many folder levels and a file name level.
pub fn recognize_grammar(folder_depth: Option<usize>) -> String {
    let levels = match folder_depth {
        None => FREE_LEVELS.to_owned(),
        Some(depth) => {
            let folders = vec!["level"; depth].join(" \",\" ");
            format!("levels ::= \"\\\"ordner\\\":[\" {folders} \"],\\\"datei\\\":\" (level | \"null\")\n")
        }
    };
    format!("{RECOGNIZE_RULES}{levels}{}", level_rules())
}

/// The levels after a change, and whether the user is done:
/// `{"ordner":[...],"datei":...,"fertig":false}`.
pub fn revise_grammar() -> String {
    format!(
        "root ::= \"{{\" levels \",\\\"fertig\\\":\" (\"true\" | \"false\") \"}}\"\n{FREE_LEVELS}{}",
        level_rules()
    )
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
        for grammar in [generate, recognize_grammar(None)] {
            for name in FILING_PLACEHOLDERS {
                assert!(grammar.contains(&format!("\"\\\"{name}\\\"\"")), "{name}");
            }
            for separator in SEPARATORS {
                assert!(
                    grammar.contains(&format!("\"\\\"{separator}\\\"\"")),
                    "{separator:?}"
                );
            }
            assert!(!grammar.contains('@'));
        }
    }

    #[test]
    fn each_grammar_has_one_root() {
        for grammar in [generate_grammar(), recognize_grammar(None)] {
            let roots = grammar
                .lines()
                .filter(|line| line.starts_with("root ::="))
                .count();
            assert_eq!(roots, 1);
        }
    }

    #[test]
    fn a_known_depth_allows_exactly_that_many_folder_levels() {
        let levels_rule = |depth| {
            recognize_grammar(depth)
                .lines()
                .find(|line| line.starts_with("levels ::="))
                .unwrap_or_default()
                .to_owned()
        };
        assert_eq!(
            levels_rule(Some(2)),
            r#"levels ::= "\"ordner\":[" level "," level "],\"datei\":" (level | "null")"#
        );
        assert_eq!(
            levels_rule(Some(1)),
            r#"levels ::= "\"ordner\":[" level "],\"datei\":" (level | "null")"#
        );
        assert!(levels_rule(None).contains("{0,6}"));
    }
}
