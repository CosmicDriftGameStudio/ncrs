//! The one check of a rule before it is stored: the rule form and the Hermes
//! chat both run it, so a rule that one refuses the other refuses too.

use ncrs_index::{compile_bounded_regex, iban_is_valid, Condition, ConditionKind, RuleSpec};

use crate::filing_plan::pattern_is_valid;

/// What is wrong with one condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionProblem {
    Empty,
    /// The reason the regular expression does not compile, on one line.
    InvalidRegex(String),
    /// The checksum of the IBAN does not match.
    InvalidIban,
}

/// Why a rule cannot be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleProblem {
    NoCondition,
    Condition(ConditionProblem),
    NoFilingTarget,
    InvalidPattern,
}

/// The first problem of the condition, `None` for one that can be stored.
pub fn condition_problem(condition: &Condition) -> Option<ConditionProblem> {
    let value = condition.value.trim();
    if value.is_empty() {
        return Some(ConditionProblem::Empty);
    }
    match condition.kind {
        ConditionKind::Regex => compile_bounded_regex(value)
            .err()
            .map(|err| ConditionProblem::InvalidRegex(reason_of(&err.to_string()))),
        ConditionKind::Iban => (!iban_is_valid(value)).then_some(ConditionProblem::InvalidIban),
        _ => None,
    }
}

/// The first problem of the rule, `None` for one that can be stored: at
/// least one condition, every condition fine, and a filing target the
/// pattern can render.
pub fn rule_problem(spec: &RuleSpec) -> Option<RuleProblem> {
    if spec.conditions.list.is_empty() {
        return Some(RuleProblem::NoCondition);
    }
    if let Some(problem) = spec.conditions.list.iter().find_map(condition_problem) {
        return Some(RuleProblem::Condition(problem));
    }
    if spec.filing_pattern.trim_matches('/').is_empty() {
        return Some(RuleProblem::NoFilingTarget);
    }
    let empty_segment = spec.filing_pattern.contains("//");
    (empty_segment || !pattern_is_valid(&spec.filing_pattern))
        .then_some(RuleProblem::InvalidPattern)
}

/// The multi-line message of the regex crate names the reason on its last
/// line: `error: unclosed group`.
fn reason_of(message: &str) -> String {
    let last = message
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim();
    last.strip_prefix("error:")
        .unwrap_or(last)
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use ncrs_index::{Condition, ConditionKind, RuleConditions, RuleSpec};

    use super::*;

    fn spec(conditions: Vec<Condition>, pattern: &str) -> RuleSpec {
        RuleSpec {
            name: None,
            conditions: RuleConditions::all_of(conditions),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: None,
            tags: std::collections::BTreeSet::new(),
            filing_pattern: pattern.to_owned(),
            confirmed: true,
            enabled: true,
        }
    }

    fn sender(value: &str) -> Condition {
        Condition::new(ConditionKind::Sender, value)
    }

    #[test]
    fn a_rule_with_a_condition_and_a_pattern_is_fine() {
        assert_eq!(
            rule_problem(&spec(vec![sender("Alfa")], "{year}/{sender}")),
            None
        );
    }

    #[test]
    fn a_rule_needs_a_condition() {
        assert_eq!(
            rule_problem(&spec(Vec::new(), "{year}")),
            Some(RuleProblem::NoCondition)
        );
    }

    #[test]
    fn a_blank_value_is_a_problem_of_its_condition() {
        assert_eq!(
            rule_problem(&spec(vec![sender("  ")], "{year}")),
            Some(RuleProblem::Condition(ConditionProblem::Empty))
        );
    }

    #[test]
    fn a_regex_that_does_not_compile_names_its_reason() {
        let regex = Condition::new(ConditionKind::Regex, "(open");
        let problem = rule_problem(&spec(vec![regex], "{year}"));
        assert!(
            matches!(
                &problem,
                Some(RuleProblem::Condition(ConditionProblem::InvalidRegex(reason)))
                    if !reason.is_empty() && !reason.contains('\n')
            ),
            "{problem:?}"
        );
    }

    #[test]
    fn an_iban_with_a_wrong_checksum_is_refused() {
        let iban = Condition::new(ConditionKind::Iban, "DE00 0000 0000 0000 0000 00");
        assert_eq!(
            rule_problem(&spec(vec![iban], "{year}")),
            Some(RuleProblem::Condition(ConditionProblem::InvalidIban))
        );
        let good = Condition::new(ConditionKind::Iban, "DE89 3704 0044 0532 0130 00");
        assert_eq!(rule_problem(&spec(vec![good], "{year}")), None);
    }

    #[test]
    fn an_empty_target_and_an_unknown_block_are_told_apart() {
        for empty in ["", "/"] {
            assert_eq!(
                rule_problem(&spec(vec![sender("Alfa")], empty)),
                Some(RuleProblem::NoFilingTarget)
            );
        }
        assert_eq!(
            rule_problem(&spec(vec![sender("Alfa")], "{nonsense}/")),
            Some(RuleProblem::InvalidPattern)
        );
    }

    #[test]
    fn an_empty_folder_level_is_refused() {
        assert_eq!(
            rule_problem(&spec(vec![sender("Alfa")], "Bank//")),
            Some(RuleProblem::InvalidPattern)
        );
        assert_eq!(rule_problem(&spec(vec![sender("Alfa")], "Bank/")), None);
    }
}
