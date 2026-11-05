//! Decides whether an answer is right by what it does, not by what it says:
//! a rule is right when it files every fixture document where the expected
//! rule does, and a pattern is right when it renders the same paths.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;

use ncrs_index::{DocumentFields, Kinds, Rule, RuleId, RuleOrigin, RuleSpec, SenderAliases};
use ncrs_intake::{plan, render_pattern_in, rule_problem, PlanInput, Planning};

use super::matrix::{archive_kinds, fixtures, Case, Expected, Fixture};
use super::parse::{parse_generate, parse_pattern};
use super::verify::{path_text, pattern_mismatches};

fn single_rule(spec: &RuleSpec) -> Rule {
    Rule {
        id: RuleId::unused_in(&[]),
        position: 1,
        origin: RuleOrigin::Hand,
        spec: spec.clone(),
    }
}

/// Where the rule files the document; `None` when it does not, whatever the
/// reason.
pub fn filed_at(spec: &RuleSpec, fixture: &Fixture, kinds: &Kinds) -> Option<PathBuf> {
    let aliases = SenderAliases::default();
    let input = PlanInput {
        text: &fixture.text,
        fields: &fixture.fields,
        path: Some(&fixture.path),
        sender_aliases: &aliases,
        manual_fields: &[],
        kinds,
    };
    let Planning::Plan(filing) = plan(&[single_rule(spec)], &input) else {
        return None;
    };
    Some(filing.relative_path)
}

pub fn check_generate(got: &RuleSpec, expected: &RuleSpec) -> Result<(), String> {
    if let Some(problem) = rule_problem(got) {
        return Err(format!("the rule is refused: {problem:?}"));
    }
    let kinds = archive_kinds();
    for (number, fixture) in fixtures().iter().enumerate() {
        let wanted = filed_at(expected, fixture, &kinds);
        let written = filed_at(got, fixture, &kinds);
        if wanted != written {
            return Err(format!(
                "document {} is filed at {written:?}, wanted {wanted:?}",
                number + 1
            ));
        }
    }
    Ok(())
}

/// The paths the expected pattern gives every fixture document, as files.
pub fn recognize_files(expected: &str, kinds: &Kinds) -> Vec<(String, DocumentFields)> {
    fixtures()
        .iter()
        .filter_map(|fixture| {
            let path = render_pattern_in(expected, &fixture.fields, kinds)?;
            Some((format!("{}.pdf", path_text(&path)), fixture.fields.clone()))
        })
        .collect()
}

pub fn check_recognize(got: &str, expected: &str) -> Result<(), String> {
    let kinds = archive_kinds();
    let files = recognize_files(expected, &kinds);
    match pattern_mismatches(got, &files, &kinds).first() {
        None => Ok(()),
        Some(mismatch) => Err(format!(
            "{} is written as {:?}",
            mismatch.file, mismatch.written
        )),
    }
}

/// The raw answer of the model against the case.
pub fn check_answer(case: &Case, raw: &str) -> Result<(), String> {
    match &case.expected {
        Expected::Rule(expected) => {
            let got = parse_generate(raw).map_err(|error| error.to_string())?;
            check_generate(&got, expected)
        }
        Expected::Pattern(expected) => {
            let got = parse_pattern(raw).map_err(|error| error.to_string())?;
            check_recognize(&got, expected)
        }
    }
}

#[cfg(test)]
mod tests {
    use ncrs_index::{Combinator, Condition, ConditionKind};
    use ncrs_intake::FILING_PLACEHOLDERS;

    use super::*;
    use crate::rules::levels::Levels;
    use crate::rules::matrix::{all_cases, rule, Axis};

    fn rule_of(conditions: Vec<Condition>, combinator: Combinator, pattern: &str) -> RuleSpec {
        rule(conditions, combinator, pattern)
    }

    fn telekom() -> String {
        "Telekom Deutschland GmbH".to_owned()
    }

    #[test]
    fn the_right_rule_passes_and_another_spelling_of_the_date_too() {
        let by_date = rule_of(
            vec![Condition::new(ConditionKind::Sender, telekom())],
            Combinator::AllOf,
            "{date}_{sender}",
        );
        let spelled_out = rule_of(
            vec![Condition::new(ConditionKind::Sender, telekom())],
            Combinator::AllOf,
            "{year}-{month}-{day}_{sender}",
        );
        assert_eq!(check_generate(&spelled_out, &by_date), Ok(()));
        assert_eq!(check_generate(&by_date, &by_date), Ok(()));
        assert_eq!(check_recognize("{year}-{month}-{day}", "{date}"), Ok(()));
    }

    #[test]
    fn a_wrong_placeholder_fails() {
        let right = rule_of(
            vec![Condition::new(ConditionKind::Sender, telekom())],
            Combinator::AllOf,
            "{year}/",
        );
        let wrong = rule_of(
            vec![Condition::new(ConditionKind::Sender, telekom())],
            Combinator::AllOf,
            "{month}/",
        );
        assert!(check_generate(&wrong, &right).is_err());
        assert!(check_recognize("{month}/", "{year}/").is_err());
        assert!(check_recognize("{nothing}/", "{year}/").is_err());
    }

    #[test]
    fn a_missing_negation_fails() {
        let right = rule_of(
            vec![Condition::not(ConditionKind::Sender, telekom())],
            Combinator::AllOf,
            "{sender}/",
        );
        let wrong = rule_of(
            vec![Condition::new(ConditionKind::Sender, telekom())],
            Combinator::AllOf,
            "{sender}/",
        );
        assert!(check_generate(&wrong, &right).is_err());
    }

    #[test]
    fn all_instead_of_any_fails() {
        let conditions = vec![
            Condition::new(ConditionKind::Sender, telekom()),
            Condition::new(ConditionKind::Kind, "Rechnung"),
        ];
        let any = rule_of(conditions.clone(), Combinator::AnyOf, "{sender}/");
        let all = rule_of(conditions, Combinator::AllOf, "{sender}/");
        assert!(check_generate(&all, &any).is_err());
        assert_eq!(check_generate(&any, &any), Ok(()));
    }

    #[test]
    fn a_rule_without_conditions_or_with_a_bad_pattern_is_refused() {
        let right = rule_of(
            vec![Condition::new(ConditionKind::Sender, telekom())],
            Combinator::AllOf,
            "{sender}/",
        );
        let none = rule_of(Vec::new(), Combinator::AllOf, "{sender}/");
        let bad = rule_of(
            right.conditions.list.clone(),
            Combinator::AllOf,
            "{nothing}/",
        );
        assert!(check_generate(&none, &right)
            .unwrap_err()
            .contains("refused"));
        assert!(check_generate(&bad, &right)
            .unwrap_err()
            .contains("refused"));
    }

    #[test]
    fn every_condition_case_files_some_fixtures_and_not_others() {
        let kinds = archive_kinds();
        let documents = fixtures();
        for case in all_cases()
            .iter()
            .filter(|case| case.axis == Axis::Condition)
        {
            let Expected::Rule(spec) = &case.expected else {
                panic!("a condition case expects a rule");
            };
            let filed = documents
                .iter()
                .filter(|fixture| filed_at(spec, fixture, &kinds).is_some())
                .count();
            assert!(
                filed > 0 && filed < documents.len(),
                "{} files {filed} of {}",
                case.expected_text(),
                documents.len()
            );
        }
    }

    #[test]
    fn the_folder_and_file_name_cases_file_every_fixture() {
        let kinds = archive_kinds();
        let documents = fixtures();
        for case in all_cases()
            .iter()
            .filter(|case| matches!(case.axis, Axis::Folder | Axis::FileName))
        {
            let Expected::Rule(spec) = &case.expected else {
                panic!("a filing case expects a rule");
            };
            for fixture in &documents {
                assert!(
                    filed_at(spec, fixture, &kinds).is_some(),
                    "{}",
                    case.expected_text()
                );
            }
        }
    }

    #[test]
    fn no_two_placeholders_render_alike() {
        for first in FILING_PLACEHOLDERS {
            for second in FILING_PLACEHOLDERS {
                if first != second {
                    assert!(
                        check_recognize(&format!("{{{first}}}"), &format!("{{{second}}}")).is_err(),
                        "{first} and {second}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_expected_answer_of_every_recognize_case_passes() {
        for case in all_cases()
            .iter()
            .filter(|case| case.axis == Axis::Recognize)
        {
            let Expected::Pattern(pattern) = &case.expected else {
                panic!("a recognize case expects a pattern");
            };
            assert_eq!(check_recognize(pattern, pattern), Ok(()), "{pattern}");
        }
    }

    fn first_case(axis: Axis) -> Case {
        all_cases()
            .into_iter()
            .find(|case| case.axis == axis)
            .unwrap()
    }

    fn levels_of(pattern: &str) -> String {
        Levels::from_pattern(pattern).unwrap().json_fields()
    }

    fn answer_of(spec: &RuleSpec, pattern: &str) -> String {
        let conditions: Vec<String> = spec
            .conditions
            .list
            .iter()
            .map(|condition| {
                format!(
                    r#"{{"kind":"{}","value":"{}","not":{}}}"#,
                    condition.kind.as_str(),
                    condition.value,
                    condition.negated
                )
            })
            .collect();
        format!(
            r#"{{"match":"all","conditions":[{}],{}}}"#,
            conditions.join(","),
            levels_of(pattern)
        )
    }

    #[test]
    fn a_raw_generate_answer_is_parsed_and_checked() {
        let case = first_case(Axis::Folder);
        let Expected::Rule(expected) = &case.expected else {
            panic!("a folder case expects a rule");
        };
        let right = answer_of(expected, &expected.filing_pattern);
        assert_eq!(check_answer(&case, &right), Ok(()));
        let other = if expected.filing_pattern == "{kind}/" {
            "{year}/"
        } else {
            "{kind}/"
        };
        assert!(check_answer(&case, &answer_of(expected, other)).is_err());
        assert!(check_answer(&case, "not json").is_err());
    }

    #[test]
    fn a_raw_recognize_answer_is_parsed_and_checked() {
        let case = first_case(Axis::Recognize);
        let Expected::Pattern(expected) = &case.expected else {
            panic!("a recognize case expects a pattern");
        };
        assert_eq!(
            check_answer(&case, &format!("{{{}}}", levels_of(expected))),
            Ok(())
        );
        assert!(check_answer(
            &case,
            &format!("{{{}}}", levels_of("{kind}/{kind}/{kind}/"))
        )
        .is_err());
        assert!(check_answer(&case, "").is_err());
    }
}
