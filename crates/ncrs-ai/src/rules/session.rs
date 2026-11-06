//! What a rule session is asked: the fixed prefix, the task after it and the
//! grammar the answer is written in. The text is built here, apart from the
//! model, so a cache can hash exactly what the model reads.

use super::grammar::{generate_grammar, recognize_grammar};
use super::prompt::{END_OF_TURN, GENERATE_REMINDER, RECOGNIZE_REMINDER};

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
            task: format!("TASK generate\n{description}\n{GENERATE_REMINDER}"),
            grammar: generate_grammar(),
            max_new_tokens: MAX_RULE_TOKENS,
        }
    }

    /// `question` is asked above the paths, in the user's language. With a
    /// `folder_depth` the answer is forced to that many folder levels and the
    /// prompt says so.
    pub fn recognize(
        prefix: &str,
        question: &str,
        paths: &str,
        folder_depth: Option<usize>,
    ) -> Self {
        let depth_line = folder_depth.map_or_else(String::new, |depth| {
            format!("Every path has {depth} folder level(s) and a file name.\n")
        });
        Self {
            prefix: prefix.to_owned(),
            task: format!("TASK recognize\n{question}\n{depth_line}{paths}\n{RECOGNIZE_REMINDER}"),
            grammar: recognize_grammar(folder_depth),
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
            format!("PREFIX|TASK generate\nAlles nach Jahr\n{GENERATE_REMINDER}{END_OF_TURN}")
        );
        assert!(request.grammar.contains("conditions"));
    }

    #[test]
    fn a_recognize_task_lists_the_paths() {
        let paths = "PATH 1: a/b.pdf\nPATH 2: c/d.pdf";
        let request = RuleRequest::recognize("P", "Which?", paths, None);
        assert_eq!(
            request.task,
            format!(
                "TASK recognize\nWhich?\nPATH 1: a/b.pdf\nPATH 2: c/d.pdf\n{RECOGNIZE_REMINDER}"
            )
        );
        assert!(!request.grammar.contains("conditions"));
    }

    #[test]
    fn a_known_depth_is_told_and_forced() {
        let request = RuleRequest::recognize("P", "Which?", "PATH 1: a/b.pdf", Some(1));
        assert!(request
            .task
            .contains("Which?\nEvery path has 1 folder level(s) and a file name.\nPATH 1"));
        assert_eq!(request.grammar, recognize_grammar(Some(1)));
    }
}
