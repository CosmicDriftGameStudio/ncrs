//! The sentences for what is wrong with a list of read folders and for a
//! failed write of the config.

use std::path::Path;

use crate::filing_setup::{shown_path, SetupProblem};
use ncrs_config::config::{ConfigError, FilingLocation, FilingRuleViolation, FilingWriteError};
use ncrs_i18n::fill_template;
use ncrs_i18n::{Language, Msg};

/// The language and the home folder the sentences are worded with.
#[derive(Clone, Copy)]
struct Wording<'a> {
    lang: Language,
    home: &'a Path,
}

/// The sentence for a problem found in the list of folders.
pub fn problem_sentence(
    problem: SetupProblem,
    chosen: &[FilingLocation],
    lang: Language,
    home: &Path,
) -> String {
    let wording = Wording { lang, home };
    match problem {
        SetupProblem::NoDocumentFolder => lang
            .text(Msg::FilingSetupProblemNoDocumentFolder)
            .to_owned(),
        SetupProblem::ChooseFilingLocation => lang
            .text(Msg::FilingSetupProblemChooseFilingLocation)
            .to_owned(),
        SetupProblem::Rule(violation) => rule_text(violation, chosen, wording),
    }
}

fn rule_text(
    violation: FilingRuleViolation,
    chosen: &[FilingLocation],
    wording: Wording<'_>,
) -> String {
    let lang = wording.lang;
    let path_at = |index: usize| {
        chosen
            .get(index)
            .map(|location| shown_path(&location.path, wording.home))
            .unwrap_or_default()
    };
    match violation {
        FilingRuleViolation::SecondTarget { .. } => {
            lang.text(Msg::FilingSetupRuleSecondTarget).to_owned()
        }
        FilingRuleViolation::InboxAsTarget { .. } => {
            lang.text(Msg::FilingSetupRuleInboxTarget).to_owned()
        }
        FilingRuleViolation::Overlap { earlier, later } => fill_template(
            lang.text(Msg::FilingSetupRuleOverlap),
            &[
                ("{first}", &path_at(earlier)),
                ("{second}", &path_at(later)),
            ],
        ),
    }
}

/// The sentence for a failed write of the locations.
pub fn write_failure_sentence(
    error: &FilingWriteError,
    chosen: &[FilingLocation],
    lang: Language,
    home: &Path,
) -> String {
    let wording = Wording { lang, home };
    let shown = |path: &Path| shown_path(path, home);
    match error {
        // Saving waits for a clean list, so this only happens when the
        // canonical paths reveal an overlap the shown ones hid.
        FilingWriteError::Rule(violation) => rule_text(*violation, chosen, wording),
        FilingWriteError::PathNotText(path) => fill_template(
            lang.text(Msg::FilingSetupPathNotText),
            &[("{path}", &shown(path))],
        ),
        FilingWriteError::Config(config_error) => config_error_text(config_error, wording),
        FilingWriteError::Io { path, reason } => fill_template(
            lang.text(Msg::FilingSetupWriteFailed),
            &[("{path}", &shown(path)), ("{reason}", reason)],
        ),
    }
}

fn config_error_text(error: &ConfigError, wording: Wording<'_>) -> String {
    let path = shown_path(error.path(), wording.home);
    match error.line() {
        Some(line) => fill_template(
            wording.lang.text(Msg::FilingSetupConfigBrokenAtLine),
            &[
                ("{path}", &path),
                ("{line}", &line.to_string()),
                ("{reason}", error.reason()),
            ],
        ),
        None => fill_template(
            wording.lang.text(Msg::FilingSetupConfigBroken),
            &[("{path}", &path), ("{reason}", error.reason())],
        ),
    }
}
