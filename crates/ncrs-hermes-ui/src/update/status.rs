//! The reads that feed the status page and the title and status bar.

use std::time::SystemTime;

use iced::Task;
use ncrs_filing::filing::FilingModule;
use ncrs_pipeline::{Load, Runner, RunnerState, Snapshot};

use super::column::{refresh_hermes_column, settle_reading_again};
use super::{watching, Outcome};
use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{FilingStatusMessage, HermesPage, Message};
use crate::state::Hermes;

/// Reads the counts of Hermes on the blocking pool: the intake may hold the
/// index for as long as it hashes a large file.
pub fn request_hermes_counts(hermes: &mut Hermes) -> Task<Message> {
    let Some(reader) = hermes
        .filing
        .as_ref()
        .and_then(FilingModule::hermes_counts_reader)
    else {
        return Task::none();
    };
    hermes.filing_status.counts_requested();
    Task::perform(
        tokio::task::spawn_blocking(move || reader.read()),
        |joined| {
            Message::FilingStatus(FilingStatusMessage::HermesCounts(
                joined.unwrap_or_else(|err| Err(err.to_string())),
            ))
        },
    )
}

impl Hermes {
    /// Takes in the latest snapshot of the pipeline and of the module.
    pub fn refresh_filing_snapshot(&mut self, now: SystemTime) {
        let snapshot = self
            .pipeline
            .as_ref()
            .map_or_else(Snapshot::off, Runner::snapshot);
        self.filing_status.set_snapshot(snapshot, now);
    }

    /// The pipeline, unless it is stopped.
    pub fn live_pipeline(&self) -> Option<&Runner> {
        self.pipeline
            .as_ref()
            .filter(|runner| runner.state() != RunnerState::Off)
    }

    /// The pause button: a paused runner is not resumed while the Hermes
    /// switch is off, because resuming would call the model again.
    pub fn toggle_pause(&mut self) {
        let hermes_on = self.filing_config.hermes_on();
        let Some(runner) = self.live_pipeline() else {
            return;
        };
        match runner.snapshot().state {
            RunnerState::Paused if hermes_on => runner.resume(),
            RunnerState::Running | RunnerState::Idle => runner.pause(),
            RunnerState::Paused | RunnerState::Off => return,
        }
        self.paused_by_switch_off = false;
    }

    /// The switch went off and the module stays: stop its runner now.
    pub fn pause_for_switch_off(&mut self) {
        let Some(runner) = self.live_pipeline() else {
            return;
        };
        if runner.snapshot().state != RunnerState::Paused {
            runner.pause();
            self.paused_by_switch_off = true;
        }
    }

    /// The switch went on and the module stays: resume what the switch paused.
    pub fn resume_after_switch_on(&mut self) {
        if !std::mem::take(&mut self.paused_by_switch_off) {
            return;
        }
        if let Some(runner) = self.live_pipeline() {
            runner.resume();
        }
    }

    pub fn set_pipeline_load(&mut self, load: Load, now: SystemTime) {
        let Some(runner) = self
            .pipeline
            .as_ref()
            .filter(|runner| runner.state() != RunnerState::Off)
        else {
            return;
        };
        runner.set_load(load);
        if let Some(module) = self.filing.as_mut() {
            module.set_load(load);
        }
        self.refresh_filing_snapshot(now);
    }
}

/// The Read errors page is shown: the snapshot is taken at once.
pub fn open_filing_status_page(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    hermes.filing_status.open_page();
    hermes.refresh_filing_snapshot(ctx.now);
    (Task::none(), vec![Action::CloseContextMenu])
}

pub fn update_filing_status(
    hermes: &mut Hermes,
    message: FilingStatusMessage,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = Vec::new();
    let task = filing_status_step(hermes, message, ctx, &mut actions);
    (task, actions)
}

fn filing_status_step(
    hermes: &mut Hermes,
    message: FilingStatusMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        FilingStatusMessage::Open => {
            actions.push(Action::ShowPage(HermesPage::Activity));
            Task::none()
        }
        FilingStatusMessage::Restarted(generation) => {
            actions.push(Action::FilingRestarted(generation));
            Task::none()
        }
        FilingStatusMessage::Tick => tick(hermes, ctx, actions),
        FilingStatusMessage::TogglePause => {
            hermes.toggle_pause();
            hermes.refresh_filing_snapshot(ctx.now);
            Task::none()
        }
        FilingStatusMessage::HermesCounts(counts) => {
            hermes.filing_status.counts_arrived(counts);
            Task::none()
        }
        FilingStatusMessage::ErrorActivated(path) => {
            actions.push(Action::RevealFile(path));
            Task::none()
        }
        FilingStatusMessage::ProcessOneAgain(path) => {
            actions.push(Action::ReadPathsAgain(vec![path]));
            Task::none()
        }
        FilingStatusMessage::ProcessAllAgain => {
            let files: Vec<_> = hermes
                .filing_status
                .errors_newest_first()
                .map(|error| error.file.clone())
                .collect();
            if !files.is_empty() {
                actions.push(Action::ReadPathsAgain(files));
            }
            Task::none()
        }
        FilingStatusMessage::LocationsResolved(resolved) => {
            super::lifecycle::take_up_resolved_locations(hermes, resolved);
            Task::none()
        }
        FilingStatusMessage::LocationsForgotten(result) => {
            if let Err(reason) = result {
                actions.push(Action::ShowError(reason));
            }
            Task::none()
        }
    }
}

/// The once-a-second look at the pipeline.
fn tick(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    actions.push(Action::RefreshDocumentsFacts);
    hermes.refresh_filing_snapshot(ctx.now);
    refresh_hermes_column(hermes, ctx);
    let settled = settle_reading_again(hermes, ctx, actions);
    actions.push(Action::EnsureChatModel);
    if ctx.chat_open {
        actions.push(Action::CheckChatWaiting);
    }
    let idle_pipeline = hermes
        .live_pipeline()
        .map(|runner| runner.snapshot())
        .filter(|snapshot| snapshot.state == RunnerState::Idle);
    if let Some(snapshot) = idle_pipeline {
        actions.push(Action::ReadingSettled(snapshot.left_pipeline));
    }
    watching::run_watching(hermes, ctx, actions);
    let counts = if hermes.live_pipeline().is_some() && hermes.filing_status.tick_wants_counts() {
        request_hermes_counts(hermes)
    } else {
        Task::none()
    };
    Task::batch([counts, settled])
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::Arc;

    use ncrs_config::config::Filing;
    use ncrs_pipeline::{Settings, SystemClock};

    use super::*;

    fn hermes_on_a_running_pipeline(switch_on: bool) -> Hermes {
        Hermes {
            filing_config: Filing {
                enabled: switch_on,
                first_run: false,
                ..Filing::default()
            },
            pipeline: Some(Runner::start(
                Settings::new(Load::Gentle, NonZeroUsize::new(2).unwrap()),
                Vec::new(),
                Vec::new(),
                Arc::new(SystemClock),
            )),
            ..Hermes::default()
        }
    }

    fn state(hermes: &Hermes) -> RunnerState {
        hermes.pipeline.as_ref().unwrap().snapshot().state
    }

    #[test]
    fn a_paused_runner_stays_paused_while_the_switch_is_off() {
        let mut hermes = hermes_on_a_running_pipeline(false);
        hermes.pause_for_switch_off();
        assert_eq!(state(&hermes), RunnerState::Paused);

        hermes.toggle_pause();

        assert_eq!(state(&hermes), RunnerState::Paused);
    }

    #[test]
    fn switching_on_resumes_only_a_pause_the_switch_made() {
        let mut hermes = hermes_on_a_running_pipeline(false);
        hermes.pause_for_switch_off();
        hermes.filing_config.enabled = true;
        hermes.resume_after_switch_on();
        assert_ne!(state(&hermes), RunnerState::Paused);

        let mut by_user = hermes_on_a_running_pipeline(true);
        by_user.toggle_pause();
        by_user.filing_config.enabled = false;
        by_user.pause_for_switch_off();
        by_user.filing_config.enabled = true;
        by_user.resume_after_switch_on();
        assert_eq!(state(&by_user), RunnerState::Paused);
    }
}
