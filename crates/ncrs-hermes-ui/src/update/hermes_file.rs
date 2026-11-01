//! What the file menu of Hermes does with the file under the cursor, and the
//! files read again.

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_config::config::{Filing, FilingWriteError};
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_i18n::{fill_template, Msg};
use ncrs_index::IncludeOutcome;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::filing_setup::{self, SetupProblem};
use crate::hermes_column::{FolderRole, HermesFileState};
use crate::message::{HermesFileMessage, HermesPage, Message};
use crate::state::Hermes;
use crate::ui::filing_setup as filing_setup_ui;

fn file_name_or_path(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Hermes reads these files again; the ones the pipeline takes are watched
/// until their reading ends.
pub fn read_paths_again(
    hermes: &mut Hermes,
    paths: Vec<PathBuf>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    let since = hermes
        .live_pipeline()
        .map_or(0, |runner| runner.snapshot().changed_sequence);
    let queued: Vec<PathBuf> = paths
        .into_iter()
        .filter(|path| {
            hermes
                .live_pipeline()
                .is_some_and(|runner| runner.process_again(path))
        })
        .collect();
    if queued.is_empty() {
        return;
    }
    for path in &queued {
        hermes.invalidate_hermes_path(path);
        hermes.reading_again.ask(path.clone(), since);
    }
    let text = if queued.len() == 1 {
        ctx.lang.text(Msg::StatusReviewReadAgain).to_owned()
    } else {
        fill_template(
            ctx.lang.text(Msg::StatusReviewsReadAgain),
            &[("{count}", &queued.len().to_string())],
        )
    };
    actions.push(Action::ShowNotice(text));
}

pub fn process_again(hermes: &mut Hermes, path: &Path, ctx: &Ctx<'_>, actions: &mut Vec<Action>) {
    let since = hermes
        .live_pipeline()
        .map_or(0, |runner| runner.snapshot().changed_sequence);
    let queued = hermes.hermes_column.state_of(path) != Some(HermesFileState::Excluded)
        && hermes
            .live_pipeline()
            .is_some_and(|runner| runner.process_again(path));
    let text = if queued {
        hermes.invalidate_hermes_path(path);
        hermes.reading_again.ask(path.to_path_buf(), since);
        fill_template(
            ctx.lang.text(Msg::StatusHermesProcessingAgain),
            &[("{name}", &file_name_or_path(path))],
        )
    } else {
        ctx.lang.text(Msg::StatusHermesNotProcessable).to_owned()
    };
    actions.push(Action::ShowNotice(text));
}

pub fn set_folder_role(
    hermes: &Hermes,
    folder: &Path,
    role: FolderRole,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(config_path) = ctx.config_path.map(Path::to_path_buf) else {
        actions.push(Action::ShowNotice(
            ctx.lang.text(Msg::FilingSetupNoConfigPath).to_owned(),
        ));
        return Task::none();
    };
    let chosen =
        match filing_setup::locations_with_role(&hermes.filing_config.locations, folder, role) {
            Ok(chosen) => chosen,
            Err(refused) => {
                let text = match refused.problem {
                    SetupProblem::NoDocumentFolder => ctx
                        .lang
                        .text(Msg::StatusHermesRoleNeedsFilingLocation)
                        .to_owned(),
                    problem => filing_setup_ui::problem_sentence(
                        problem,
                        &refused.chosen,
                        ctx.lang,
                        ctx.home,
                    ),
                };
                actions.push(Action::ShowError(text));
                return Task::none();
            }
        };
    let written_path = config_path.clone();
    Task::perform(
        tokio::task::spawn_blocking(move || filing_setup::write_locations(&written_path, &chosen)),
        move |joined| {
            Message::HermesFile(HermesFileMessage::RoleSaved(joined.unwrap_or_else(
                |join| {
                    Err(FilingWriteError::Io {
                        path: config_path,
                        reason: join.to_string(),
                    })
                },
            )))
        },
    )
}

pub fn role_save_failed(
    hermes: &Hermes,
    error: &FilingWriteError,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    let reason = filing_setup_ui::write_failure_sentence(
        error,
        &hermes.filing_config.locations,
        ctx.lang,
        ctx.home,
    );
    let text = fill_template(
        ctx.lang.text(Msg::StatusHermesRoleFailed),
        &[("{reason}", &reason)],
    );
    actions.push(Action::ShowError(text));
}

/// `excluded` is whether the file or folder under the cursor is excluded now:
/// it is included again, otherwise excluded.
pub fn toggle_hermes_exclusion(
    hermes: &mut Hermes,
    path: PathBuf,
    excluded: bool,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    if !excluded {
        return super::quick_fix::exclude_from_hermes(hermes, vec![path], actions);
    }
    let Some(access) = hermes.filing.as_ref().and_then(FilingModule::review_access) else {
        actions.push(Action::ShowError(INDEX_NOT_OPEN.to_owned()));
        return Task::none();
    };
    let included = path.clone();
    Task::perform(
        tokio::task::spawn_blocking(move || access.include(&path)),
        move |joined| {
            Message::HermesFile(HermesFileMessage::Included {
                path: included.clone(),
                result: joined.unwrap_or_else(|err| Err(err.to_string())),
            })
        },
    )
}

pub fn hermes_included(
    hermes: &mut Hermes,
    path: &Path,
    result: Result<IncludeOutcome, String>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    let text = match result {
        Err(detail) => {
            actions.push(Action::ShowError(detail));
            return;
        }
        Ok(IncludeOutcome::ExcludedByAncestor(folder)) => fill_template(
            ctx.lang.text(Msg::StatusHermesIncludedByFolder),
            &[("{folder}", &folder.display().to_string())],
        ),
        Ok(IncludeOutcome::Included | IncludeOutcome::NotExcluded) => {
            hermes.invalidate_hermes_path(path);
            if let Some(runner) = hermes.live_pipeline() {
                runner.rescan_around(path);
            }
            fill_template(
                ctx.lang.text(Msg::StatusHermesIncluded),
                &[("{name}", &file_name_or_path(path))],
            )
        }
    };
    actions.push(Action::ShowNotice(text));
}

/// The file with the new role is written: the module follows it, and the
/// column asks again.
pub fn role_saved(
    hermes: &mut Hermes,
    result: Result<Filing, FilingWriteError>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match result {
        Ok(filing) => {
            let (applies, task) = super::lifecycle::apply_saved_filing(hermes, filing, ctx);
            hermes.hermes_column.invalidate_all();
            hermes.hermes_reads.clear();
            actions.push(Action::ShowNotice(super::lifecycle::saved_filing_notice(
                applies, ctx,
            )));
            task
        }
        Err(error) => {
            role_save_failed(hermes, &error, ctx, actions);
            Task::none()
        }
    }
}

/// Opens the Hermes page on the inspection, at the file's error if the page
/// lists one for it.
pub fn show_in_inspection(hermes: &Hermes, path: &Path, ctx: &Ctx<'_>, actions: &mut Vec<Action>) {
    actions.push(if super::open_page(hermes, ctx).is_some() {
        Action::GoToHermesPage(HermesPage::Inspection)
    } else {
        Action::OpenFilingStatus
    });
    actions.push(Action::SelectErrorOf(path.to_path_buf()));
}
