use iced::Task;
use ncrs_filing::filing::{FilingMessage, FilingModule, Work, WorkOutcome};
use ncrs_filing::filing_work::FilingSummary;
use ncrs_i18n::{fill_template, Msg};
use ncrs_jobs::{Job, JobKind};
use std::path::PathBuf;

use crate::action::{Action, StartProgressJob};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::Hermes;

/// One job files every planned document. Waits for a running job instead of
/// queueing behind it, and is asked for again on the next tick.
pub fn run_filing(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) {
    if ctx.jobs_busy {
        hermes.filing_due = true;
        return;
    }
    hermes.filing_due = false;
    let Some(access) = ctx.filing.and_then(FilingModule::filing_access) else {
        return;
    };
    actions.push(Action::StartJobWithProgress(StartProgressJob {
        job: Job {
            kind: JobKind::File,
            path: PathBuf::new(),
            total: None,
        },
        build: Box::new(move |progress_tx| {
            Task::perform(
                tokio::task::spawn_blocking(move || {
                    let mut progress = ncrs_fs::transfer::Reporting::new(&progress_tx);
                    access.file_planned(&mut progress)
                }),
                |joined| {
                    Message::DocumentsFiled(joined.unwrap_or_else(|join| Err(join.to_string())))
                },
            )
        }),
    }));
}

/// Runs one blocking step of the filing module off the UI thread.
pub fn run_filing_work(work: Work) -> Task<Message> {
    run_filing_work_after(None, work)
}

/// Like [`run_filing_work`], with the pipeline dropped first on the same
/// blocking thread: joining its threads blocks, and the work needs the index
/// free of them.
pub fn run_filing_work_after(pipeline: Option<ncrs_pipeline::Runner>, work: Work) -> Task<Message> {
    let generation = work.generation();
    Task::perform(
        tokio::task::spawn_blocking(move || {
            drop(pipeline);
            work.run()
        }),
        move |joined| {
            Message::Filing(joined.unwrap_or_else(|join| FilingMessage::WorkDone {
                generation,
                outcome: WorkOutcome::Crashed(join.to_string()),
            }))
        },
    )
}

/// The filing job is over.
pub fn documents_filed(
    result: Result<FilingSummary, String>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    actions.extend([Action::FinishJob, Action::ClearJobProgress]);
    match result {
        Ok(summary) if summary.filed + summary.reviewed > 0 => {
            let text = fill_template(
                ctx.lang.text(Msg::StatusDocumentsFiled),
                &[
                    ("{filed}", &summary.filed.to_string()),
                    ("{reviewed}", &summary.reviewed.to_string()),
                ],
            );
            actions.push(Action::ShowNotice(text));
        }
        Ok(_) => {}
        Err(detail) => actions.push(Action::ShowError(detail)),
    }
}
