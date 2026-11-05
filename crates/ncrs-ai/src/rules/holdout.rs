//! Sentences written by hand, the way people talk: not made from the matrix
//! and not in the prompt. Each has the rule that the model should write.

#![allow(clippy::panic, clippy::unreachable)]

use ncrs_intake::FILING_PLACEHOLDERS;

use super::matrix::{Ask, Axis, Case, Expected, Language};
use super::parse::parse_generate;
use super::prompt::with_levels;

const HOLDOUT: &str = include_str!("holdout.txt");

pub fn holdout_cases() -> Vec<Case> {
    HOLDOUT
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.splitn(3, " | ");
            let (Some(language), Some(text), Some(answer)) =
                (parts.next(), parts.next(), parts.next())
            else {
                panic!("a holdout line is `language | sentence | answer`: {line}");
            };
            let spec = parse_generate(&with_levels(answer))
                .unwrap_or_else(|error| panic!("the answer of {text:?} is bad: {error}"));
            Case {
                axis: Axis::Holdout,
                language: if language == "en" {
                    Language::English
                } else {
                    Language::German
                },
                ask: Ask::Describe(text.to_owned()),
                placeholders: FILING_PLACEHOLDERS
                    .into_iter()
                    .filter(|name| spec.filing_pattern.contains(&format!("{{{name}}}")))
                    .collect(),
                conditions: spec.conditions.list.iter().map(|c| c.kind).collect(),
                expected: Expected::Rule(spec),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use ncrs_intake::rule_problem;

    use super::*;
    use crate::rules::matrix::{archive_kinds, fixtures};
    use crate::rules::oracle::filed_at;
    use crate::rules::prompt::system_prompt;

    #[test]
    fn there_are_forty_or_more_sentences_in_both_languages() {
        let cases = holdout_cases();
        assert!(cases.len() >= 40, "{}", cases.len());
        assert!(cases.iter().any(|c| c.language == Language::English));
        assert!(cases.iter().any(|c| c.language == Language::German));
    }

    #[test]
    fn every_expected_rule_is_valid_and_files_some_document() {
        let kinds = archive_kinds();
        for case in holdout_cases() {
            let Expected::Rule(spec) = &case.expected else {
                unreachable!()
            };
            assert!(rule_problem(spec).is_none(), "{}", case.asked());
            let filed = fixtures()
                .iter()
                .filter(|fixture| filed_at(spec, fixture, &kinds).is_some())
                .count();
            assert!(filed > 0, "no fixture matches: {}", case.asked());
        }
    }

    #[test]
    fn no_sentence_is_in_the_prompt() {
        let prompt = system_prompt();
        for case in holdout_cases() {
            assert!(!prompt.contains(&case.asked()), "{}", case.asked());
        }
    }
}
