//! What the file menu of Hermes does with the file under the cursor, and the
//! files read again.

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_config::config::{Filing, FilingLocation, FilingWriteError, LocationTransfer};
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_i18n::{fill_template, Msg};
use ncrs_index::IncludeOutcome;
use ncrs_intake::LocationMode;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::filing_setup::{self, SetupProblem};
use crate::hermes_column::FolderRole;
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

/// Resolves the folders among `paths` to their documents on a blocking
/// thread; [`process_found_again`] gets the answer.
pub fn process_again(
    hermes: &Hermes,
    paths: Vec<PathBuf>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(finder) = hermes
        .filing
        .as_ref()
        .and_then(FilingModule::process_again_finder)
    else {
        actions.push(Action::ShowNotice(
            ctx.lang.text(Msg::StatusHermesNotProcessable).to_owned(),
        ));
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || finder.find(&paths)),
        |joined| {
            Message::HermesFile(HermesFileMessage::ProcessAgainFound(
                joined.unwrap_or_else(|err| Err(err.to_string())),
            ))
        },
    )
}

/// More files than this in the inboxes are asked about first: Hermes
/// may move them out.
const FILES_WITHOUT_QUESTION: usize = 1;

fn lies_in_an_inbox(hermes: &Hermes, path: &Path) -> bool {
    hermes
        .filing_config
        .locations
        .iter()
        .any(|location| location.mode == LocationMode::Inbox && path.starts_with(&location.path))
}

pub fn process_found_again(
    hermes: &mut Hermes,
    result: Result<Vec<PathBuf>, String>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    let found = match result {
        Ok(found) => found,
        Err(reason) => {
            actions.push(Action::ShowError(fill_template(
                ctx.lang.text(Msg::StatusHermesColumnReadFailed),
                &[("{reason}", &reason)],
            )));
            return;
        }
    };
    let in_inboxes = found
        .iter()
        .filter(|path| lies_in_an_inbox(hermes, path))
        .count();
    if in_inboxes > FILES_WITHOUT_QUESTION {
        hermes.confirm_again = Some(found);
        return;
    }
    queue_again(hermes, found, ctx, actions);
}

/// The answer yes to the question about many files.
pub fn confirm_process_again(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) {
    if let Some(found) = hermes.confirm_again.take() {
        queue_again(hermes, found, ctx, actions);
    }
}

fn queue_again(hermes: &mut Hermes, found: Vec<PathBuf>, ctx: &Ctx<'_>, actions: &mut Vec<Action>) {
    let since = hermes
        .live_pipeline()
        .map_or(0, |runner| runner.snapshot().changed_sequence);
    let queued: Vec<PathBuf> = found
        .into_iter()
        .filter(|path| {
            hermes
                .live_pipeline()
                .is_some_and(|runner| runner.process_again(path))
        })
        .collect();
    for path in &queued {
        hermes.invalidate_hermes_path(path);
        hermes.reading_again.ask(path.clone(), since);
    }
    let text = match queued.as_slice() {
        [] => ctx.lang.text(Msg::StatusHermesNotProcessable).to_owned(),
        [only] => fill_template(
            ctx.lang.text(Msg::StatusHermesProcessingAgain),
            &[("{name}", &file_name_or_path(only))],
        ),
        many => fill_template(
            ctx.lang.text(Msg::StatusHermesProcessingAgainMany),
            &[("{count}", &many.len().to_string())],
        ),
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
    match locations_with_role_or_say(hermes, folder, role, ctx, actions) {
        Some(chosen) => save_locations(config_path, chosen),
        None => Task::none(),
    }
}

fn locations_with_role_or_say(
    hermes: &Hermes,
    folder: &Path,
    role: FolderRole,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Option<Vec<FilingLocation>> {
    match filing_setup::locations_with_role(&hermes.filing_config.locations, folder, role) {
        Ok(chosen) => Some(chosen),
        Err(refused) => {
            let text = match refused.problem {
                SetupProblem::NoDocumentFolder => ctx
                    .lang
                    .text(Msg::StatusHermesRoleNeedsFilingLocation)
                    .to_owned(),
                problem => {
                    filing_setup_ui::problem_sentence(problem, &refused.chosen, ctx.lang, ctx.home)
                }
            };
            actions.push(Action::ShowError(text));
            None
        }
    }
}

/// `folder` files its new files by itself with `transfer`, or, with `None`,
/// no longer. Switching on makes a folder that is no read folder one, in the
/// same write.
pub fn set_continuous_filing(
    hermes: &Hermes,
    folder: &Path,
    transfer: Option<LocationTransfer>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(config_path) = ctx.config_path.map(Path::to_path_buf) else {
        actions.push(Action::ShowNotice(
            ctx.lang.text(Msg::FilingSetupNoConfigPath).to_owned(),
        ));
        return Task::none();
    };
    let configured = &hermes.filing_config.locations;
    let is_read_folder = configured
        .iter()
        .any(|location| location.path == folder && location.mode == LocationMode::Inbox);
    let base = if transfer.is_none() || is_read_folder {
        configured.clone()
    } else {
        let Some(with_role) =
            locations_with_role_or_say(hermes, folder, FolderRole::Inbox, ctx, actions)
        else {
            return Task::none();
        };
        with_role
    };
    match filing_setup::locations_with_continuous(&base, folder, transfer) {
        Some(chosen) => save_locations(config_path, chosen),
        None => Task::none(),
    }
}

fn save_locations(config_path: PathBuf, chosen: Vec<FilingLocation>) -> Task<Message> {
    let written_path = config_path.clone();
    Task::perform(
        tokio::task::spawn_blocking(move || filing_setup::write_locations(&written_path, &chosen)),
        move |joined| {
            Message::HermesFile(HermesFileMessage::RoleSaved(Box::new(
                joined.unwrap_or_else(|join| {
                    Err(FilingWriteError::Io {
                        path: config_path,
                        reason: join.to_string(),
                    })
                }),
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
            hermes.hermes_failed_reads.clear();
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

/// Opens the settings on the Read errors page.
pub fn show_in_inspection(actions: &mut Vec<Action>) {
    actions.push(Action::ShowPage(HermesPage::Inspection));
}

/// A Hermes file command for the cursor row, or what came back of one.
pub fn update(
    hermes: &mut Hermes,
    message: HermesFileMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        HermesFileMessage::RoleSaved(result) => return role_saved(hermes, *result, ctx, actions),
        HermesFileMessage::ProcessAgainFound(result) => {
            process_found_again(hermes, result, ctx, actions);
            return Task::none();
        }
        HermesFileMessage::TextlessFound(result) => {
            match result {
                Ok(found) => read_paths_again(hermes, found, ctx, actions),
                Err(reason) => actions.push(Action::ShowError(fill_template(
                    ctx.lang.text(Msg::StatusHermesColumnReadFailed),
                    &[("{reason}", &reason)],
                ))),
            }
            return Task::none();
        }
        HermesFileMessage::ConfirmProcessAgain => {
            confirm_process_again(hermes, ctx, actions);
            return Task::none();
        }
        HermesFileMessage::CancelProcessAgain => {
            hermes.confirm_again = None;
            return Task::none();
        }
        HermesFileMessage::Included { path, result } => {
            hermes_included(hermes, &path, result, ctx, actions);
            return Task::none();
        }
        HermesFileMessage::HandToInbox
        | HermesFileMessage::ProcessAgain
        | HermesFileMessage::SetRole(_)
        | HermesFileMessage::FileOnce
        | HermesFileMessage::FileContinuously
        | HermesFileMessage::StopContinuous
        | HermesFileMessage::ToggleExclusion
        | HermesFileMessage::ShowInInspection => {}
    }
    let available = if matches!(message, HermesFileMessage::SetRole(_)) {
        hermes.is_ready()
    } else {
        hermes.is_active()
    };
    if ctx.overlay_open || ctx.search_open || !available {
        return Task::none();
    }
    let Some((path, is_dir)) = ctx.cursor_row else {
        return Task::none();
    };
    match message {
        HermesFileMessage::HandToInbox => {
            match hermes.inbox_location() {
                Some(inbox) => actions.push(Action::MoveToInbox(inbox.to_path_buf())),
                None => actions.push(Action::ShowNotice(
                    ctx.lang.text(Msg::StatusHermesNoInbox).to_owned(),
                )),
            }
            Task::none()
        }
        HermesFileMessage::ProcessAgain => {
            process_again(hermes, ctx.action_sources.clone(), ctx, actions)
        }
        HermesFileMessage::SetRole(role) if is_dir => {
            set_folder_role(hermes, path, role, ctx, actions)
        }
        HermesFileMessage::StopContinuous if is_dir => {
            set_continuous_filing(hermes, path, None, ctx, actions)
        }
        HermesFileMessage::ToggleExclusion => {
            let excluded = hermes.cursor_is_excluded(path, is_dir);
            toggle_hermes_exclusion(hermes, path.to_path_buf(), excluded, actions)
        }
        HermesFileMessage::ShowInInspection => {
            show_in_inspection(actions);
            Task::none()
        }
        // Opening the filing tab is the app's: it owns the tab.
        HermesFileMessage::SetRole(_)
        | HermesFileMessage::StopContinuous
        | HermesFileMessage::FileOnce
        | HermesFileMessage::FileContinuously
        | HermesFileMessage::RoleSaved(_)
        | HermesFileMessage::ProcessAgainFound(_)
        | HermesFileMessage::TextlessFound(_)
        | HermesFileMessage::ConfirmProcessAgain
        | HermesFileMessage::CancelProcessAgain
        | HermesFileMessage::Included { .. } => Task::none(),
    }
}
