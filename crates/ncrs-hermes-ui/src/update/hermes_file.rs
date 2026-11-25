//! What the file menu of Hermes does with the file under the cursor, and the
//! files read again.

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_config::config::{same_folder, Filing, FilingLocation, FilingWriteError};
use ncrs_core::INDEX_NOT_OPEN;
use ncrs_filing::filing::FilingModule;
use ncrs_i18n::{fill_template, Msg};
use ncrs_index::IncludeOutcome;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::filing_setup::{self, SetupProblem};
use crate::hermes_column::FolderRole;
use crate::message::{HermesFileMessage, HermesPage, Message, TargetsMessage};
use crate::source_dialog::SourceDialogKind;
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

/// More files than this in the watched sources are asked about first: Hermes
/// may move them out.
const FILES_WITHOUT_QUESTION: usize = 1;

fn lies_in_a_watched_source(filing: &Filing, path: &Path) -> bool {
    filing
        .sources
        .iter()
        .any(|setting| setting.source.watch && path.starts_with(&setting.source.path))
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
    let in_watched_sources = found
        .iter()
        .filter(|path| lies_in_a_watched_source(&hermes.filing_config, path))
        .count();
    if in_watched_sources > FILES_WITHOUT_QUESTION {
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

/// What the dialog of the targets is told to show the target at `folder`:
/// the one there is, or a new one.
fn target_dialog_steps(filing: &Filing, folder: &Path) -> Vec<TargetsMessage> {
    let known = filing
        .targets
        .iter()
        .position(|setting| same_folder(&setting.target.path, folder));
    match known {
        Some(index) => vec![TargetsMessage::Change(index)],
        None => vec![
            TargetsMessage::Add,
            TargetsMessage::Folder(folder.to_string_lossy().into_owned()),
        ],
    }
}

/// Opens the dialog of the target at `folder`, a new one when there is none.
fn open_target_dialog(
    hermes: &mut Hermes,
    folder: &Path,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let steps = target_dialog_steps(&hermes.filing_config, folder);
    Task::batch(
        steps
            .into_iter()
            .map(|step| super::targets::update(hermes, step, ctx, actions)),
    )
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
        HermesFileMessage::ProcessAgain
        | HermesFileMessage::FileOnce
        | HermesFileMessage::WatchSource
        | HermesFileMessage::AddTarget
        | HermesFileMessage::ToggleExclusion
        | HermesFileMessage::ShowInInspection => {}
    }
    // The first source and the first target are set up before there is any.
    let sets_up_sorting = matches!(
        message,
        HermesFileMessage::WatchSource | HermesFileMessage::AddTarget
    );
    let available = if sets_up_sorting {
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
        HermesFileMessage::ProcessAgain => {
            process_again(hermes, ctx.action_sources.clone(), ctx, actions)
        }
        HermesFileMessage::WatchSource if is_dir => super::sorting::open_dialog(
            hermes,
            Some(path.to_path_buf()),
            SourceDialogKind::Watched,
        ),
        HermesFileMessage::AddTarget if is_dir => open_target_dialog(hermes, path, ctx, actions),
        HermesFileMessage::ToggleExclusion => {
            let excluded = hermes.cursor_is_excluded(path, is_dir);
            toggle_hermes_exclusion(hermes, path.to_path_buf(), excluded, actions)
        }
        HermesFileMessage::ShowInInspection => {
            show_in_inspection(actions);
            Task::none()
        }
        // Opening the filing tab is the app's: it owns the tab.
        HermesFileMessage::FileOnce
        | HermesFileMessage::WatchSource
        | HermesFileMessage::AddTarget
        | HermesFileMessage::RoleSaved(_)
        | HermesFileMessage::ProcessAgainFound(_)
        | HermesFileMessage::TextlessFound(_)
        | HermesFileMessage::ConfirmProcessAgain
        | HermesFileMessage::CancelProcessAgain
        | HermesFileMessage::Included { .. } => Task::none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ncrs_config::config::{SourceSetting, TargetSetting};
    use ncrs_index::TargetId;
    use ncrs_intake::{LocationTransfer, OnNoText, OnSeveralTargets, SortSource, SortTarget};

    fn filing() -> Filing {
        let source = |path: &str, watch| SourceSetting {
            source: SortSource {
                path: PathBuf::from(path),
                include: String::new(),
                exclude: String::new(),
                targets: vec![TargetId::main()],
                watch,
                transfer: LocationTransfer::default(),
                no_text: OnNoText::default(),
                several: OnSeveralTargets::default(),
                own_volume: None,
            },
            paused: None,
        };
        Filing {
            sources: vec![source("/in", true), source("/once", false)],
            targets: vec![TargetSetting {
                target: SortTarget {
                    id: TargetId::main(),
                    name: "Docs".to_owned(),
                    path: PathBuf::from("/docs"),
                    own_volume: None,
                },
                paused: None,
            }],
            ..Filing::default()
        }
    }

    #[test]
    fn the_target_dialog_shows_the_target_at_the_folder_or_a_new_one() {
        assert_eq!(
            target_dialog_steps(&filing(), Path::new("/docs")),
            [TargetsMessage::Change(0)]
        );
        assert_eq!(
            target_dialog_steps(&filing(), Path::new("/docs/2024")),
            [
                TargetsMessage::Add,
                TargetsMessage::Folder("/docs/2024".to_owned())
            ]
        );
    }

    #[test]
    fn only_files_below_a_watched_source_may_be_filed_when_read_again() {
        let filing = filing();
        assert!(lies_in_a_watched_source(&filing, Path::new("/in/a.pdf")));
        assert!(!lies_in_a_watched_source(&filing, Path::new("/once/a.pdf")));
        assert!(!lies_in_a_watched_source(&filing, Path::new("/docs/a.pdf")));
    }

    #[test]
    fn the_roles_of_a_folder_follow_the_sources_and_targets() {
        let hermes = Hermes {
            filing_config: filing(),
            ..Hermes::default()
        };
        let source = hermes.cursor_roles(Path::new("/in"));
        assert!(source.is_source && !source.is_target);
        let target = hermes.cursor_roles(Path::new("/docs"));
        assert!(target.is_target && !target.is_source);
        let below = hermes.cursor_roles(Path::new("/in/sub"));
        assert!(!below.is_source && !below.is_target);
    }
}
