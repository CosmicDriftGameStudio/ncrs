//! The reads that feed the status page and the title and status bar.

use std::num::NonZeroUsize;
use std::time::SystemTime;

use iced::Task;
use ncrs_filing::filing::FilingModule;
use ncrs_pipeline::{Load, Runner, RunnerState, Snapshot};

use super::column::{refresh_hermes_column, settle_reading_again};
use super::{filing_job, open_page, refile, Outcome};
use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{FilingStatusMessage, HermesPage, Message};
use crate::state::Hermes;

/// Reads the figures on the blocking pool: the intake may hold the index for
/// as long as it hashes a large file.
pub fn request_index_figures(hermes: &mut Hermes) -> Task<Message> {
    let Some(reader) = hermes
        .filing
        .as_ref()
        .and_then(FilingModule::index_figures_reader)
    else {
        return Task::none();
    };
    hermes.filing_status.figures_requested();
    Task::perform(
        tokio::task::spawn_blocking(move || reader.read()),
        |joined| {
            Message::FilingStatus(FilingStatusMessage::IndexFigures(
                joined.unwrap_or_else(|err| Err(err.to_string())),
            ))
        },
    )
}

/// Reads the counts of Hermes on the blocking pool, like the index figures.
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
        self.filing_status.set_ai(
            self.filing
                .as_ref()
                .map(FilingModule::ai_status)
                .unwrap_or_default(),
        );
    }

    /// The pipeline, unless it is stopped.
    pub fn live_pipeline(&self) -> Option<&Runner> {
        self.pipeline
            .as_ref()
            .filter(|runner| runner.snapshot().state != RunnerState::Off)
    }

    pub fn set_pipeline_load(&mut self, load: Load, now: SystemTime) {
        let Some(runner) = self
            .pipeline
            .as_ref()
            .filter(|runner| runner.snapshot().state != RunnerState::Off)
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

/// The status page opens: the app has checked that nothing covers it.
pub fn open_filing_status_page(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Outcome {
    hermes.filing_status.open_page();
    hermes.refresh_filing_snapshot(ctx.now);
    (
        request_index_figures(hermes),
        vec![Action::CloseContextMenu],
    )
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
            actions.push(Action::OpenHermesOverview);
            Task::none()
        }
        FilingStatusMessage::Close => {
            hermes.filing_status.close_page();
            Task::none()
        }
        FilingStatusMessage::Toggle => {
            match open_page(hermes, ctx) {
                Some(HermesPage::Inspection) => hermes.filing_status.close_page(),
                Some(_) => actions.push(Action::GoToHermesPage(HermesPage::Inspection)),
                None => actions.push(Action::OpenFilingStatus),
            }
            Task::none()
        }
        FilingStatusMessage::Restarted(generation) => {
            actions.push(Action::FilingRestarted(generation));
            Task::none()
        }
        FilingStatusMessage::Tick => tick(hermes, ctx, actions),
        FilingStatusMessage::TogglePause => {
            if let Some(runner) = hermes.live_pipeline() {
                match runner.snapshot().state {
                    RunnerState::Paused => runner.resume(),
                    RunnerState::Running | RunnerState::Idle => runner.pause(),
                    RunnerState::Off => {}
                }
                hermes.refresh_filing_snapshot(ctx.now);
            }
            Task::none()
        }
        FilingStatusMessage::StepWorkers(delta) => {
            let Some(runner) = hermes.live_pipeline() else {
                return Task::none();
            };
            let current = match hermes.filing.as_ref().map(FilingModule::load) {
                Some(Load::Workers(count)) => count.get(),
                _ => runner.snapshot().worker_target,
            };
            let most = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
            let wanted = current
                .saturating_add_signed(isize::from(delta))
                .clamp(1, most);
            if let Some(count) = NonZeroUsize::new(wanted) {
                hermes.set_pipeline_load(Load::Workers(count), ctx.now);
            }
            Task::none()
        }
        FilingStatusMessage::HermesCounts(counts) => {
            hermes.filing_status.counts_arrived(counts);
            Task::none()
        }
        FilingStatusMessage::IndexFigures(figures) => {
            hermes.filing_status.figures_arrived(figures);
            Task::none()
        }
        FilingStatusMessage::ErrorActivated(path) => {
            hermes.filing_status.close_page();
            actions.push(Action::RevealFile(path));
            Task::none()
        }
        FilingStatusMessage::MoveError(delta) => {
            hermes.filing_status.move_error_selection(delta);
            Task::none()
        }
        FilingStatusMessage::SelectError(position) => {
            hermes.filing_status.select_error(position);
            Task::none()
        }
        FilingStatusMessage::ToggleErrorChecked(path) => {
            hermes.filing_status.toggle_error_checked(&path);
            Task::none()
        }
        FilingStatusMessage::ToggleSelectedErrorChecked => {
            hermes.filing_status.toggle_selected_error_checked();
            Task::none()
        }
        FilingStatusMessage::ProcessAgain => {
            actions.push(Action::ReadPathsAgain(
                hermes.filing_status.files_to_process_again(),
            ));
            Task::none()
        }
        FilingStatusMessage::ToggleSteps => {
            hermes.filing_status.toggle_steps();
            Task::none()
        }
    }
}

/// The once-a-second look at the pipeline. The filing job comes before the
/// refiling, which waits for the queue the filing job may have just taken.
fn tick(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    hermes.refresh_filing_snapshot(ctx.now);
    refresh_hermes_column(hermes, ctx);
    let settled = settle_reading_again(hermes, ctx, actions);
    let plans_made = hermes
        .live_pipeline()
        .map_or(0, |runner| runner.snapshot().plans_made);
    let first_new_action = actions.len();
    if plans_made > hermes.plans_filed_for || hermes.filing_due {
        hermes.plans_filed_for = plans_made;
        filing_job::run_filing(hermes, ctx, actions);
    }
    let filing_job_started = actions
        .iter()
        .skip(first_new_action)
        .any(|action| matches!(action, Action::StartJobWithProgress(_)));
    let after_filing = Ctx {
        jobs_busy: ctx.jobs_busy || filing_job_started,
        ..ctx.clone()
    };
    refile::run_refile(hermes, &after_filing, actions);
    let counts = if hermes.live_pipeline().is_some() && hermes.filing_status.tick_wants_counts() {
        request_hermes_counts(hermes)
    } else {
        Task::none()
    };
    let figures = if hermes.filing_status.tick_wants_figures() {
        request_index_figures(hermes)
    } else {
        Task::none()
    };
    Task::batch([counts, settled, figures])
}
