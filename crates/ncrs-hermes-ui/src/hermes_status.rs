//! What Hermes tells in one line: the state of the pipeline, how many files
//! are in work, open, waiting for the user or failed. The status bar and the
//! title chip both read it, so they never disagree.

use ncrs_pipeline::{RunnerState, Snapshot, Task};

use ncrs_i18n::fill_template;
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::format;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateWord {
    Running,
    Paused,
    /// Running, with nothing waiting and no worker busy on a step.
    Idle,
}

/// How the title chip is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Fine,
    Paused,
    Stuck,
    Failing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HermesStatus {
    pub word: StateWord,
    /// Workers busy on a step.
    pub working: usize,
    /// Files queued at a step.
    pub open: usize,
    /// Files of the sources that wait for an answer of the user.
    pub waiting: usize,
    /// Files that failed a step.
    pub failed: usize,
    pub pdf_library_missing: bool,
    pub stuck: bool,
}

/// Hermes is stuck when it is paused with files queued or with questions open:
/// it cannot get on without the user then.
fn is_stuck(word: StateWord, open: usize, waiting: usize) -> bool {
    word == StateWord::Paused && (open > 0 || waiting > 0)
}

/// The counter of the status bar and of the activity page.
pub fn waiting_text(count: usize, lang: Language) -> String {
    if count == 1 {
        return lang.text(Msg::HermesStatusWaitingOne).to_owned();
    }
    counted(Msg::HermesStatusWaitingOther, count, lang)
}

fn counted(template: Msg, count: usize, lang: Language) -> String {
    fill_template(
        lang.text(template),
        &[("{count}", &format::grouped(count as u64))],
    )
}

impl HermesStatus {
    /// `None` while the pipeline is off.
    pub fn of(snapshot: &Snapshot, waiting: usize, pdf_library_missing: bool) -> Option<Self> {
        if snapshot.state == RunnerState::Off {
            return None;
        }
        let working = snapshot
            .workers
            .iter()
            .filter(|worker| {
                worker
                    .busy
                    .as_ref()
                    .is_some_and(|busy| matches!(busy.task, Task::Step(_)))
            })
            .count();
        let open: usize = snapshot.steps.iter().map(|step| step.waiting).sum();
        let failed: usize = snapshot.steps.iter().map(|step| step.failed).sum();
        let word = if snapshot.state == RunnerState::Paused {
            StateWord::Paused
        } else if working == 0 && open == 0 {
            StateWord::Idle
        } else {
            StateWord::Running
        };
        Some(Self {
            word,
            working,
            open,
            waiting,
            failed,
            pdf_library_missing,
            stuck: is_stuck(word, open, waiting),
        })
    }

    /// The documents-per-minute rate only means something while files are
    /// in work or queued; it also counts documents routed to review, so an
    /// idle Hermes must not advertise it.
    pub fn is_busy(&self) -> bool {
        self.working > 0 || self.open > 0
    }

    pub fn is_failing(&self) -> bool {
        self.pdf_library_missing || self.failed > 0
    }

    pub fn tone(&self) -> Tone {
        if self.is_failing() {
            Tone::Failing
        } else if self.stuck {
            Tone::Stuck
        } else if self.word == StateWord::Paused {
            Tone::Paused
        } else {
            Tone::Fine
        }
    }

    /// The line of the status bar: the state word and the parts that are not
    /// zero; one notice instead when the PDF library is missing.
    pub fn line(&self, lang: Language) -> String {
        if self.pdf_library_missing {
            return lang.text(Msg::HermesStatusPdfMissing).to_owned();
        }
        let word = lang.text(match self.word {
            StateWord::Running => Msg::HermesStatusRunning,
            StateWord::Paused => Msg::HermesStatusPaused,
            StateWord::Idle => Msg::HermesStatusIdle,
        });
        let part =
            |count: usize, template: Msg| (count > 0).then(|| counted(template, count, lang));
        [
            Some(word.to_owned()),
            part(self.working, Msg::HermesStatusWorking),
            part(self.open, Msg::HermesStatusOpen),
            (self.waiting > 0).then(|| waiting_text(self.waiting, lang)),
            part(self.failed, Msg::HermesStatusFailed),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use ncrs_pipeline::{Busy, StepSnapshot, WorkerSnapshot};

    use super::*;

    fn snapshot(state: RunnerState, waiting: usize, failed: usize, busy: usize) -> Snapshot {
        let mut snapshot = Snapshot::off();
        snapshot.state = state;
        snapshot.steps = vec![StepSnapshot {
            name: "stamp",
            waiting,
            done: 0,
            failed,
            unchanged: 0,
        }];
        snapshot.workers = (0..busy)
            .map(|id| WorkerSnapshot {
                id,
                busy: Some(Busy {
                    file: PathBuf::from("a.pdf"),
                    task: Task::Step("stamp"),
                    started_at: SystemTime::UNIX_EPOCH,
                }),
            })
            .collect();
        snapshot
    }

    fn line(status: HermesStatus, lang: Language) -> String {
        status.line(lang)
    }

    #[test]
    fn nothing_is_said_while_the_pipeline_is_off() {
        assert!(HermesStatus::of(&Snapshot::off(), 0, false).is_none());
    }

    #[test]
    fn a_running_pipeline_names_what_is_not_zero() {
        let status = HermesStatus::of(&snapshot(RunnerState::Running, 5, 0, 2), 3, false).unwrap();
        assert_eq!(
            line(status, Language::English),
            "running \u{b7} 2 working \u{b7} 5 open \u{b7} 3 wait for you"
        );
        assert_eq!(
            line(status, Language::German),
            "l\u{e4}uft \u{b7} 2 in Arbeit \u{b7} 5 offen \u{b7} 3 warten auf dich"
        );
        assert_eq!(status.tone(), Tone::Fine);
    }

    #[test]
    fn nothing_waiting_and_no_worker_busy_is_idle() {
        let status = HermesStatus::of(&snapshot(RunnerState::Running, 0, 0, 0), 0, false).unwrap();
        assert_eq!(status.word, StateWord::Idle);
        assert_eq!(line(status, Language::English), "idle");
        assert_eq!(line(status, Language::German), "ruht");
    }

    #[test]
    fn errors_show_up_and_turn_the_chip_red() {
        let status = HermesStatus::of(&snapshot(RunnerState::Running, 0, 4, 0), 0, false).unwrap();
        assert_eq!(line(status, Language::English), "idle \u{b7} 4 failed");
        assert_eq!(status.tone(), Tone::Failing);
    }

    #[test]
    fn a_missing_pdf_library_is_one_notice_and_red() {
        let status = HermesStatus::of(&snapshot(RunnerState::Running, 5, 0, 2), 3, true).unwrap();
        assert_eq!(
            line(status, Language::English),
            "PDF library missing, PDFs are not read"
        );
        assert_eq!(
            line(status, Language::German),
            "PDF-Bibliothek fehlt, PDFs werden nicht gelesen"
        );
        assert_eq!(status.tone(), Tone::Failing);
    }

    #[test]
    fn paused_with_files_waiting_is_stuck_and_paused_alone_is_not() {
        let queued = HermesStatus::of(&snapshot(RunnerState::Paused, 3, 0, 0), 0, false).unwrap();
        assert_eq!(line(queued, Language::English), "paused \u{b7} 3 open");
        assert_eq!(queued.tone(), Tone::Stuck);
        let empty = HermesStatus::of(&snapshot(RunnerState::Paused, 0, 0, 0), 0, false).unwrap();
        assert_eq!(empty.tone(), Tone::Paused);
    }

    #[test]
    fn questions_are_stuck_only_while_hermes_is_paused() {
        let paused = HermesStatus::of(&snapshot(RunnerState::Paused, 0, 0, 0), 2, false).unwrap();
        assert_eq!(paused.tone(), Tone::Stuck);
        let idle = HermesStatus::of(&snapshot(RunnerState::Running, 0, 0, 0), 2, false).unwrap();
        assert_eq!(idle.tone(), Tone::Fine);
    }

    #[test]
    fn one_question_is_told_in_the_singular() {
        assert_eq!(waiting_text(1, Language::German), "1 wartet auf dich");
        assert_eq!(waiting_text(1, Language::English), "1 waits for you");
    }

    #[test]
    fn big_numbers_are_grouped() {
        let status =
            HermesStatus::of(&snapshot(RunnerState::Running, 0, 0, 0), 2421, false).unwrap();
        assert_eq!(
            line(status, Language::German),
            "ruht \u{b7} 2\u{a0}421 warten auf dich"
        );
    }
}
