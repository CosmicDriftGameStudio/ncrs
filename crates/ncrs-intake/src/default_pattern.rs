//! The patterns Hermes proposes for a document no rule and no learned archive
//! pattern covers. They only ever fill the pattern field of a review; nothing
//! is filed by them.

use ncrs_index::DocumentFields;

use crate::filing_plan::{pattern_is_valid, render_pattern};

/// The built-in patterns, richest first: the first one the fields can fill
/// is the proposal.
pub const DEFAULT_PATTERNS: [&str; 3] = [
    "{recipient}/{year}/{sender}/",
    "{year}/{sender}/",
    "{year}/",
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

/// `pattern` without the retired `{tax_context}` block and the separator beside
/// it; a pattern that is empty afterwards becomes `{year}/`. The
/// index migration to schema 20 applies the same replacements in SQL.
pub fn pattern_without_tax_context(pattern: &str) -> String {
    let stripped = [
        "{tax_context}/",
        "/{tax_context}",
        "{tax_context}_",
        "_{tax_context}",
        "{tax_context}-",
        "-{tax_context}",
        "{tax_context}",
    ]
    .iter()
    .fold(pattern.to_owned(), |text, block| text.replace(block, ""));
    if matches!(stripped.as_str(), "" | "/") {
        FALLBACK_PATTERN.to_owned()
    } else {
        stripped
    }
}

/// What an emptied pattern becomes: the year folder, named automatically.
pub const FALLBACK_PATTERN: &str = "{year}/";

/// A pattern saved by an older version, in the shape the current version
/// files by: one without a file name part gets a trailing "/" (the file name
/// is then made automatically). The index migration to schema 20 does the
/// same in SQL for stored rules.
pub fn upgraded_pattern(pattern: &str) -> String {
    let stripped = pattern_without_tax_context(pattern);
    let last_part = stripped.rsplit('/').next().unwrap_or_default();
    let names_the_file = ["{day}", "{date}", "{number}"]
        .iter()
        .any(|placeholder| last_part.contains(placeholder));
    if last_part.is_empty() || names_the_file {
        stripped
    } else {
        format!("{stripped}/")
    }
}

/// The default pattern for these fields: the `custom` one when it is valid
/// and the fields can fill it, else the first built-in one they can fill.
/// `None` when none fits, for example without a date.
pub fn default_pattern_for(custom: Option<&str>, fields: &DocumentFields) -> Option<String> {
    let valid_custom = custom
        .map(str::trim)
        .filter(|pattern| pattern_is_valid(pattern));
    let fills = |pattern: &&str| render_pattern(pattern, fields).is_some();
    valid_custom
        .into_iter()
        .chain(DEFAULT_PATTERNS)
        .find(fills)
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

    fn recipient_frost(recipient: bool, sender: bool, date: bool) -> DocumentFields {
        DocumentFields {
            recipient: recipient.then(|| "Anna".to_owned()),
            ..frost(sender, true, date)
        }
    }

    #[test]
    fn the_retired_tax_context_block_leaves_with_its_separator() {
        for (before, after) in [
            ("{tax_context}/{year}", "{year}"),
            ("{year}/{tax_context}/{sender}", "{year}/{sender}"),
            ("{tax_context}_{year}", "{year}"),
            ("{tax_context}", FALLBACK_PATTERN),
            ("{year}/{sender}", "{year}/{sender}"),
        ] {
            assert_eq!(pattern_without_tax_context(before), after, "{before}");
        }
    }

    #[test]
    fn an_old_pattern_without_a_file_name_part_gets_a_trailing_slash() {
        for (before, after) in [
            ("{year}/{sender}", "{year}/{sender}/"),
            ("{year}/{sender}/", "{year}/{sender}/"),
            ("{year}/{number}", "{year}/{number}"),
            (
                "{year}/{year}{month}{day}_{sender}",
                "{year}/{year}{month}{day}_{sender}",
            ),
            ("{tax_context}/{year}", "{year}/"),
            ("{tax_context}", "{year}/"),
        ] {
            assert_eq!(upgraded_pattern(before), after, "{before}");
        }
    }

    #[test]
    fn every_built_in_pattern_is_valid_and_files_in_folders_only() {
        for pattern in DEFAULT_PATTERNS {
            assert!(pattern_is_valid(pattern), "{pattern}");
            assert!(pattern.ends_with('/'), "{pattern}");
        }
        assert!(!pattern_is_valid(""));
        assert!(!pattern_is_valid("/"));
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
    fn without_a_rule_the_default_fills_in_and_falls_back_folder_by_folder() {
        assert_eq!(
            filed_as(&recipient_frost(true, true, true), None),
            Some(PathBuf::from("Anna/2021/Frost/2021-05-18_Frost_3318"))
        );
        assert_eq!(
            filed_as(&recipient_frost(false, true, true), None),
            Some(PathBuf::from("2021/Frost/2021-05-18_Frost_3318"))
        );
        assert_eq!(
            filed_as(&recipient_frost(false, false, true), None),
            Some(PathBuf::from("2021/2021-05-18_3318"))
        );
        assert_eq!(
            choose_pattern(None, None, &frost(true, true, false)),
            None,
            "no date, no guess"
        );
        assert_eq!(
            choose_pattern(None, None, &frost(true, true, true)).map(|chosen| chosen.source),
            Some(PatternSource::Default)
        );
    }

    #[test]
    fn a_custom_default_comes_first_and_the_built_in_ones_follow() {
        let custom = Some("{year}/{recipient}/");

        assert_eq!(
            filed_as(
                &recipient_frost(false, true, true),
                Some("{year}/{sender}/")
            ),
            Some(PathBuf::from("2021/Frost/2021-05-18_Frost_3318"))
        );
        assert_eq!(
            filed_as(&frost(true, true, true), custom),
            Some(PathBuf::from("2021/Frost/2021-05-18_Frost_3318")),
            "no recipient: the built-in pattern takes over"
        );
    }

    #[test]
    fn an_invalid_custom_default_is_ignored() {
        assert_eq!(
            filed_as(&recipient_frost(false, true, true), Some("{nope}/x")),
            Some(PathBuf::from("2021/Frost/2021-05-18_Frost_3318"))
        );
    }
}
