//! The patterns Hermes proposes for a document no rule and no learned archive
//! pattern covers. They only ever fill the pattern field of a review; nothing
//! is filed by them.

use ncrs_index::DocumentFields;

use crate::filing_plan::{pattern_is_valid, render_pattern};

/// The built-in patterns, richest first: the first one the fields can fill
/// is the proposal.
pub const DEFAULT_PATTERNS: [&str; 3] = [
    "{year}/{year}{month}{day}_{sender}_{number}",
    "{year}/{year}{month}{day}_{sender}",
    "{year}/{year}{month}{day}_{number}",
];

/// Where a proposed pattern comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternSource {
    /// A rule, or a pattern learned from an archive.
    RuleOrProfile,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChosenPattern {
    pub pattern: String,
    pub source: PatternSource,
}

/// The default pattern for these fields: the `custom` one when it is valid
/// (and then the only one), else the first built-in one the fields can fill.
/// `None` when none fits, for example without a date.
pub fn default_pattern_for(custom: Option<&str>, fields: &DocumentFields) -> Option<String> {
    let valid_custom = custom
        .map(str::trim)
        .filter(|pattern| pattern_is_valid(pattern));
    let fills = |pattern: &&str| render_pattern(pattern, fields).is_some();
    match valid_custom {
        Some(pattern) => Some(pattern).filter(fills),
        None => DEFAULT_PATTERNS.iter().copied().find(fills),
    }
    .map(str::to_owned)
}

/// The pattern a review starts with: the one of a rule or a learned profile
/// if there is one, else the default pattern.
pub fn choose_pattern(
    proposed: Option<&str>,
    custom_default: Option<&str>,
    fields: &DocumentFields,
) -> Option<ChosenPattern> {
    if let Some(pattern) = proposed {
        return Some(ChosenPattern {
            pattern: pattern.to_owned(),
            source: PatternSource::RuleOrProfile,
        });
    }
    default_pattern_for(custom_default, fields).map(|pattern| ChosenPattern {
        pattern,
        source: PatternSource::Default,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::path::PathBuf;

    use chrono::NaiveDate;

    use super::*;

    fn frost(sender: bool, number: bool, date: bool) -> DocumentFields {
        DocumentFields {
            sender: sender.then(|| "Frost".to_owned()),
            number: number.then(|| "3318".to_owned()),
            date: date.then(|| NaiveDate::from_ymd_opt(2021, 5, 18).unwrap()),
            ..DocumentFields::default()
        }
    }

    fn filed_as(fields: &DocumentFields, custom: Option<&str>) -> Option<PathBuf> {
        let chosen = choose_pattern(None, custom, fields)?;
        render_pattern(&chosen.pattern, fields)
    }

    #[test]
    fn every_built_in_pattern_is_valid() {
        for pattern in DEFAULT_PATTERNS {
            assert!(pattern_is_valid(pattern), "{pattern}");
        }
    }

    #[test]
    fn a_rule_or_profile_pattern_comes_before_the_default() {
        let chosen = choose_pattern(Some("{kind}/{date}"), None, &frost(true, true, true));

        assert_eq!(
            chosen,
            Some(ChosenPattern {
                pattern: "{kind}/{date}".to_owned(),
                source: PatternSource::RuleOrProfile,
            })
        );
    }

    #[test]
    fn without_a_rule_the_default_fills_in_and_falls_back_field_by_field() {
        assert_eq!(
            filed_as(&frost(true, true, true), None),
            Some(PathBuf::from("2021/20210518_Frost_3318"))
        );
        assert_eq!(
            filed_as(&frost(true, false, true), None),
            Some(PathBuf::from("2021/20210518_Frost"))
        );
        assert_eq!(
            filed_as(&frost(false, true, true), None),
            Some(PathBuf::from("2021/20210518_3318"))
        );
        assert_eq!(
            choose_pattern(None, None, &frost(true, true, false)),
            None,
            "no date, no guess"
        );
        assert_eq!(choose_pattern(None, None, &frost(false, false, true)), None);
        assert_eq!(
            choose_pattern(None, None, &frost(true, true, true)).map(|chosen| chosen.source),
            Some(PatternSource::Default)
        );
    }

    #[test]
    fn a_custom_default_is_the_only_one() {
        let custom = Some("{year}/{sender}");

        assert_eq!(
            filed_as(&frost(true, true, true), custom),
            Some(PathBuf::from("2021/Frost"))
        );
        assert_eq!(
            choose_pattern(None, custom, &frost(false, true, true)),
            None,
            "no fallback to the built-in patterns"
        );
    }

    #[test]
    fn an_invalid_custom_default_is_ignored() {
        assert_eq!(
            filed_as(&frost(true, true, true), Some("{nope}/x")),
            Some(PathBuf::from("2021/20210518_Frost_3318"))
        );
    }
}
