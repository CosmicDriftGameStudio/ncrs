use iced::Task;
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_filing::replan_work::{lies_in_inbox, ReplanItem, ReplanScope, ReplanSummary};
use ncrs_i18n::{fill_template, Msg};
use ncrs_intake::LocationMode;
use ncrs_jobs::{Job, JobKind};
use std::path::PathBuf;

use super::{review_list, Outcome};
use crate::action::{Action, StartProgressJob};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::Hermes;

/// Plans the documents below `folder` again, if it lies in an inbox location.
/// Anywhere else nothing happens but the message.
pub fn reprocess_folder(
    hermes: &mut Hermes,
    folder: PathBuf,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let inbox_roots: Vec<PathBuf> = hermes
        .filing_config
        .locations
        .iter()
        .filter(|location| location.mode == LocationMode::Inbox)
        .map(|location| location.path.clone())
        .collect();
    if !lies_in_inbox(&folder, &inbox_roots) {
        let text = ctx.lang.text(Msg::StatusReprocessOutsideInbox).to_owned();
        actions.push(Action::ShowNotice(text));
        return Task::none();
    }
    review_list::start_replan(hermes, ReplanScope::Folder(folder), ctx, actions)
}

/// The user confirmed the preview: the previewed documents are filed by one
/// job, as soon as the queue is free.
pub fn confirm_replan(
    hermes: &mut Hermes,
    scope: ReplanScope,
    items: Vec<ReplanItem>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    let folder = match scope {
        ReplanScope::Folder(folder) => folder,
        ReplanScope::OpenCases => PathBuf::new(),
    };
    hermes.pending_refile = Some((folder, items));
    if ctx.jobs_busy {
        let text = ctx.lang.text(Msg::StatusRefileWaiting).to_owned();
        actions.push(Action::ShowNotice(text));
        return;
    }
    run_refile(hermes, ctx, actions);
}

/// Starts the confirmed refiling if no other job runs; asked for again on the
/// next tick otherwise, so it never queues behind another job.
pub fn run_refile(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) {
    if ctx.jobs_busy {
        return;
    }
    let Some((folder, items)) = hermes.pending_refile.take() else {
        return;
    };
    let Some(access) = hermes.filing.as_ref().and_then(FilingModule::replan_access) else {
        actions.push(Action::ShowError(INDEX_NOT_OPEN.to_owned()));
        return;
    };
    actions.push(Action::StartJobWithProgress(StartProgressJob {
        job: Job {
            kind: JobKind::File,
            path: folder,
            total: Some(items.iter().filter(|item| item.would_be_filed()).count()),
        },
        build: Box::new(move |progress_tx| {
            Task::perform(
                tokio::task::spawn_blocking(move || {
                    let mut progress = ncrs_fs::transfer::Reporting::new(&progress_tx);
                    access.file_previewed(&items, &mut progress)
                }),
                |joined| {
                    Message::DocumentsRefiled(joined.unwrap_or_else(|join| Err(join.to_string())))
                },
            )
        }),
    }));
}

/// The job that filed the previewed documents is over.
pub fn documents_refiled(
    hermes: &mut Hermes,
    result: Result<ReplanSummary, String>,
    ctx: &Ctx<'_>,
) -> Outcome {
    let mut actions = vec![Action::FinishJob, Action::ClearJobProgress];
    let reload = if hermes.review_list.is_some() {
        review_list::run_review_load(hermes)
    } else {
        Task::none()
    };
    match result {
        Ok(summary) => {
            let text = fill_template(
                ctx.lang.text(Msg::StatusRefileDone),
                &[
                    ("{filed}", &summary.filed.to_string()),
                    ("{reviewed}", &summary.reviewed.to_string()),
                    ("{changed}", &summary.changed.to_string()),
                    ("{requeued}", &summary.requeued.to_string()),
                ],
            );
            actions.push(Action::ShowNotice(text));
        }
        Err(detail) => actions.push(Action::ShowError(detail)),
    }
    (reload, actions)
}
