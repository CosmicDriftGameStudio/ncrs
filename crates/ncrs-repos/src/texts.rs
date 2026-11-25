//! The words of a result, shared by the log and the rows.

use ncrs_i18n::{fill_template, Language, Msg};

use crate::bulk::{Outcome, SkipReason};

/// What a row says about how its repository ended; the sign comes from the kit.
pub fn outcome_text(outcome: &Outcome, lang: Language) -> String {
    let number = |msg: Msg, count: &str| fill_template(lang.text(msg), &[("{n}", count)]);
    match outcome {
        Outcome::Running => lang.text(Msg::ReposResultRunning).to_owned(),
        Outcome::Updated { behind } => number(Msg::ReposResultUpdated, &behind.to_string()),
        Outcome::AlreadyCurrent => lang.text(Msg::ReposResultCurrent).to_owned(),
        Outcome::Switched { from, to, behind } => fill_template(
            lang.text(Msg::ReposResultSwitched),
            &[("{from}", from), ("{to}", to), ("{n}", &behind.to_string())],
        ),
        Outcome::Skipped(SkipReason::Worktree) => lang.text(Msg::ReposResultWorktree).to_owned(),
        Outcome::Skipped(SkipReason::LocalChanges(count)) => {
            number(Msg::ReposResultLocalChanges, &count.to_string())
        }
        Outcome::Skipped(SkipReason::Diverged { ahead }) => {
            number(Msg::ReposResultAhead, &ahead.to_string())
        }
        Outcome::NoAccess { remote } => {
            fill_template(lang.text(Msg::ReposResultNoAccess), &[("{remote}", remote)])
        }
        Outcome::Failed { .. } => lang.text(Msg::ReposResultFailed).to_owned(),
    }
}

/// What the log says: the same, but a failure with the last line git said.
pub fn log_text(outcome: &Outcome, lang: Language) -> String {
    match outcome {
        Outcome::Failed { last_line } if !last_line.is_empty() => last_line.clone(),
        other => outcome_text(other, lang),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_result_has_words_in_both_languages() {
        let outcomes = [
            Outcome::Running,
            Outcome::Updated { behind: 3 },
            Outcome::AlreadyCurrent,
            Outcome::Switched {
                from: "dev".to_owned(),
                to: "main".to_owned(),
                behind: 2,
            },
            Outcome::Skipped(SkipReason::Worktree),
            Outcome::Skipped(SkipReason::LocalChanges(4)),
            Outcome::Skipped(SkipReason::Diverged { ahead: 2 }),
            Outcome::NoAccess {
                remote: "origin".to_owned(),
            },
            Outcome::Failed {
                last_line: String::new(),
            },
        ];
        for lang in [Language::English, Language::German] {
            for outcome in &outcomes {
                let text = outcome_text(outcome, lang);
                assert!(!text.is_empty());
                assert!(!text.contains('{'), "{text}");
            }
        }
    }

    #[test]
    fn a_failure_logs_what_git_said() {
        let failed = Outcome::Failed {
            last_line: "fatal: unable to access".to_owned(),
        };
        assert_eq!(
            log_text(&failed, Language::English),
            "fatal: unable to access"
        );
    }
}
