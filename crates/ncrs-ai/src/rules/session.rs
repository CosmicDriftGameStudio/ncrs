//! What a rule session is asked: the fixed prefix, the task after it and the
//! grammar the answer is written in. The text is built here, apart from the
//! model, so a cache can hash exactly what the model reads.

use super::grammar::{generate_grammar, recognize_grammar};
use super::prompt::END_OF_TURN;

/// A rule is a short JSON object: up to six conditions and a pattern.
const MAX_RULE_TOKENS: usize = 400;
const MAX_PATTERN_TOKENS: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRequest {
    /// Decoded once per session.
    pub prefix: String,
    /// The user's words after the prefix; special tokens in it stay text.
    pub task: String,
    pub grammar: String,
    pub max_new_tokens: usize,
}

impl RuleRequest {
    pub fn generate(prefix: &str, description: &str) -> Self {
        Self {
            prefix: prefix.to_owned(),
            task: format!("TASK generate\n{description}"),
            grammar: generate_grammar(),
            max_new_tokens: MAX_RULE_TOKENS,
        }
    }

    /// `question` is asked above the paths, in the user's language.
    pub fn recognize(prefix: &str, question: &str, paths: &[String]) -> Self {
        Self {
            prefix: prefix.to_owned(),
            task: format!("TASK recognize\n{question}\n{}", paths.join("\n")),
            grammar: recognize_grammar(),
            max_new_tokens: MAX_PATTERN_TOKENS,
        }
    }

    /// The text the model reads from the first to the last token.
    pub fn full_prompt(&self) -> String {
        format!("{}{}{END_OF_TURN}", self.prefix, self.task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_is_prefix_task_and_the_start_of_the_answer() {
        let request = RuleRequest::generate("PREFIX|", "Alles nach Jahr");
        assert_eq!(
            request.full_prompt(),
            format!("PREFIX|TASK generate\nAlles nach Jahr{END_OF_TURN}")
        );
        assert!(request.grammar.contains("conditions"));
    }

    #[test]
    fn a_recognize_task_lists_the_paths() {
        let paths = vec!["a/b.pdf".to_owned(), "c/d.pdf".to_owned()];
        let request = RuleRequest::recognize("P", "Which?", &paths);
        assert_eq!(request.task, "TASK recognize\nWhich?\na/b.pdf\nc/d.pdf");
        assert!(!request.grammar.contains("conditions"));
    }
}
