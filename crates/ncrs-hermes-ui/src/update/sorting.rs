//! The page Einsortieren: the sources, their dialog and the history.

use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::operation;
use iced::Task;
use ncrs_config::config::{remove_sort_source, upsert_sort_source, Filing, FilingWriteError};
use ncrs_filing::sorting::is_on_own_volume;
use ncrs_i18n::{fill_template, Msg};
use ncrs_index::BatchId;
use ncrs_ui_kit::focus::release_focus;

use super::{hermes_file, lifecycle};
use crate::action::Action;
use crate::ctx::Ctx;
use crate::documents_facts::run_facts;
use crate::message::Message;
use crate::sorting_page::{own_volume_to_save, source_state, Loaded, SortingMessage, SortingPage};
use crate::source_dialog::{
    look_up_folder, FolderCheck, SourceDialog, SourceDialogKind, SourceField,
};
use crate::state::Hermes;
use crate::target_dialog::{TargetDialog, TargetsMessage};
use crate::ui::source_dialog::{EXCLUDE_FIELD_ID, FOLDER_FIELD_ID, INCLUDE_FIELD_ID};

/// Runs the history can list.
const RUNS_LISTED_AT_MOST: usize = 50;

fn sorting(message: SortingMessage) -> Message {
    Message::Sorting(message)
}

pub fn open(hermes: &mut Hermes, ctx: &Ctx<'_>, _actions: &mut Vec<Action>) -> Task<Message> {
    hermes.sorting = Some(SortingPage::default());
    load(hermes, ctx)
}

/// Reads the state of the sources and the history in the background.
pub fn load(hermes: &Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    let settings = hermes.filing_config.sources.clone();
    let access = hermes
        .filing
        .as_ref()
        .and_then(|module| module.sorting_access(Arc::clone(&ctx.trash.0)));
    let now = ctx.now;
    let read = move || {
        let states = settings
            .iter()
            .map(|setting| source_state(setting, access.as_ref()))
            .collect();
        let runs = match &access {
            Some(sorting_access) => sorting_access
                .recent_runs(RUNS_LISTED_AT_MOST)?
                .iter()
                .map(|run| run_facts(run, now))
                .collect(),
            None => Vec::new(),
        };
        Ok(Loaded { states, runs })
    };
    Task::perform(
        async move { tokio::task::spawn_blocking(read).await },
        |joined| {
            sorting(SortingMessage::Loaded(Box::new(
                joined.unwrap_or_else(|err| Err(err.to_string())),
            )))
        },
    )
}

/// Opens the dialog of the source at `folder` (a new one without), or of the
/// one-off filing of the folder.
pub fn open_dialog(
    hermes: &mut Hermes,
    folder: Option<PathBuf>,
    kind: SourceDialogKind,
) -> Task<Message> {
    hermes.source_dialog = Some(SourceDialog::new(
        kind,
        folder.as_deref(),
        &hermes.filing_config,
    ));
    Task::batch([
        check_folder(hermes, folder.unwrap_or_default()),
        operation::focus(FOLDER_FIELD_ID),
    ])
}

/// Looks for the folder of the dialog in the background: a dead mount can hang
/// that. Only the answer to the last look counts.
fn check_folder(hermes: &mut Hermes, folder: PathBuf) -> Task<Message> {
    let Some(dialog) = hermes.source_dialog.as_mut() else {
        return Task::none();
    };
    hermes.source_check += 1;
    let generation = hermes.source_check;
    dialog.folder_check = FolderCheck::Looking;
    if dialog.kind == SourceDialogKind::Once {
        // The plan that was asked for is the one of the folder before.
        dialog.saving = false;
    }
    Task::perform(
        async move { tokio::task::spawn_blocking(move || look_up_folder(&folder)).await },
        move |joined| {
            sorting(SortingMessage::FolderChecked {
                generation,
                found: joined.ok().flatten(),
            })
        },
    )
}

fn check_typed_folder(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    match hermes.source_dialog.as_ref() {
        Some(dialog) => {
            let folder = dialog.path(ctx.home);
            check_folder(hermes, folder)
        }
        None => Task::none(),
    }
}

/// A target that the dialog of a target just wrote is ticked in the source
/// dialog under it.
pub fn tick_new_targets(hermes: &mut Hermes, message: &TargetsMessage) {
    let Some(dialog) = hermes.source_dialog.as_mut() else {
        return;
    };
    match message {
        TargetsMessage::Add => dialog.remember_targets(&hermes.filing_config),
        TargetsMessage::Written(result) => {
            if let Ok(filing) = &**result {
                dialog.tick_new_targets(filing);
            }
        }
        _ => {}
    }
}

pub fn update(
    hermes: &mut Hermes,
    message: SortingMessage,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match message {
        SortingMessage::Loaded(result) => {
            match (*result, hermes.sorting.as_mut()) {
                (Ok(loaded), Some(page)) => {
                    page.states = loaded.states;
                    page.runs = loaded.runs;
                }
                (Err(reason), Some(_)) => actions.push(Action::ShowError(reason)),
                (_, None) => {}
            }
            Task::none()
        }
        SortingMessage::AddSource => open_dialog(hermes, None, SourceDialogKind::Watched),
        SortingMessage::ChangeSource(index) => {
            let folder = hermes
                .filing_config
                .sources
                .get(index)
                .map(|setting| setting.source.path.clone());
            open_dialog(hermes, folder, SourceDialogKind::Watched)
        }
        SortingMessage::RemoveSource(index) => remove(hermes, index, ctx, actions),
        SortingMessage::RepickSource(index) => {
            let folder = hermes
                .filing_config
                .sources
                .get(index)
                .map(|setting| setting.source.path.clone());
            if let (Some(_), Some(page)) = (&folder, hermes.sorting.as_mut()) {
                page.repick = folder;
                actions.push(Action::PickSourceFolder(None));
            }
            Task::none()
        }
        SortingMessage::OpenQuestions(index) => {
            if let Some(setting) = hermes.filing_config.sources.get(index) {
                actions.push(Action::OpenPlan {
                    source: setting.source.clone(),
                    once: false,
                });
            }
            Task::none()
        }
        SortingMessage::ToggleAllRuns => {
            if let Some(page) = hermes.sorting.as_mut() {
                page.all_runs = !page.all_runs;
            }
            Task::none()
        }
        SortingMessage::Undo(batch) => undo(hermes, batch, ctx),
        SortingMessage::Undone(result) => {
            match result {
                Ok(0) => {}
                Ok(left) => actions.push(Action::ShowNotice(fill_template(
                    ctx.lang.text(Msg::SettingsHermesHistoryUndoLeft),
                    &[("{left}", &left.to_string())],
                ))),
                Err(reason) => actions.push(Action::ShowError(fill_template(
                    ctx.lang.text(Msg::SettingsHermesHistoryUndoFailed),
                    &[("{reason}", &reason)],
                ))),
            }
            actions.push(Action::ReloadPanels);
            load(hermes, ctx)
        }
        SortingMessage::Written(result) => written(hermes, *result, ctx, actions),
        SortingMessage::Enter if keyboard_is_on_add_target(hermes) => {
            press_focused(hermes);
            Task::none()
        }
        SortingMessage::Enter | SortingMessage::Save => save(hermes, ctx, actions),
        SortingMessage::ShowPlan => show_plan(hermes, ctx),
        SortingMessage::Cancel => {
            hermes.source_dialog = None;
            Task::none()
        }
        SortingMessage::PickFolder => {
            if let Some(dialog) = &hermes.source_dialog {
                actions.push(Action::PickSourceFolder(match &dialog.folder_check {
                    FolderCheck::Found(path) => Some(path.clone()),
                    FolderCheck::Looking | FolderCheck::Missing => None,
                }));
            }
            Task::none()
        }
        SortingMessage::FolderPicked(picked) => {
            let repicked = hermes.sorting.as_mut().and_then(|page| page.repick.take());
            match (picked, repicked) {
                (Some(folder), Some(old)) => repick(hermes, old, folder, ctx, actions),
                (Some(folder), None) => {
                    folder_picked(hermes, folder);
                    check_typed_folder(hermes, ctx)
                }
                (None, _) => Task::none(),
            }
        }
        SortingMessage::FolderText(typed) => {
            if let Some(dialog) = hermes.source_dialog.as_mut() {
                dialog.folder = typed;
                dialog.focus = SourceField::Folder;
            }
            check_typed_folder(hermes, ctx)
        }
        SortingMessage::FolderChecked { generation, found } => {
            let is_last = generation == hermes.source_check;
            if let Some(dialog) = hermes.source_dialog.as_mut().filter(|_| is_last) {
                dialog.folder_check = found.map_or(FolderCheck::Missing, FolderCheck::Found);
            }
            Task::none()
        }
        SortingMessage::PlanSource { generation, source } => {
            let is_last = generation == hermes.source_check;
            let waits = hermes
                .source_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.saving);
            if is_last && waits {
                hermes.source_dialog = None;
                actions.push(Action::OpenPlan {
                    source: *source,
                    once: true,
                });
            }
            Task::none()
        }
        SortingMessage::FocusNext => move_focus(hermes, true),
        SortingMessage::FocusPrevious => move_focus(hermes, false),
        SortingMessage::Press => {
            press_focused(hermes);
            Task::none()
        }
        SortingMessage::ToggleTarget(id) => {
            let listed = &hermes.filing_config.targets;
            let at = listed.iter().position(|setting| setting.target.id == id);
            if let Some(dialog) = hermes.source_dialog.as_mut() {
                dialog.toggle_target(&id);
                dialog.focus = at.map_or(dialog.focus, SourceField::Target);
            }
            Task::none()
        }
        draft_edit => {
            if let Some(dialog) = hermes.source_dialog.as_mut() {
                edit_draft(dialog, draft_edit);
            }
            Task::none()
        }
    }
}

fn move_focus(hermes: &mut Hermes, forward: bool) -> Task<Message> {
    let targets = hermes.filing_config.targets.len();
    let Some(dialog) = hermes.source_dialog.as_mut() else {
        return Task::none();
    };
    dialog.move_focus(forward, targets);
    match dialog.focus {
        SourceField::Folder => operation::focus(FOLDER_FIELD_ID),
        SourceField::Include => operation::focus(INCLUDE_FIELD_ID),
        SourceField::Exclude => operation::focus(EXCLUDE_FIELD_ID),
        SourceField::Target(_)
        | SourceField::AddTarget
        | SourceField::When
        | SourceField::Files
        | SourceField::NoText
        | SourceField::Several => release_focus(),
    }
}

fn keyboard_is_on_add_target(hermes: &Hermes) -> bool {
    hermes
        .source_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.focus == SourceField::AddTarget)
}

fn press_focused(hermes: &mut Hermes) {
    let Some(dialog) = hermes.source_dialog.as_mut() else {
        return;
    };
    match dialog.focus {
        SourceField::Target(at) => {
            if let Some(setting) = hermes.filing_config.targets.get(at) {
                dialog.toggle_target(&setting.target.id);
            }
        }
        SourceField::AddTarget => {
            dialog.remember_targets(&hermes.filing_config);
            hermes.target_dialog = Some(TargetDialog::new(None));
        }
        _ => dialog.step_choice(),
    }
}

fn edit_draft(dialog: &mut SourceDialog, message: SortingMessage) {
    match message {
        SortingMessage::Include(text) => {
            dialog.include = text;
            dialog.focus = SourceField::Include;
        }
        SortingMessage::Exclude(text) => {
            dialog.exclude = text;
            dialog.focus = SourceField::Exclude;
        }
        SortingMessage::Focus(field) => dialog.focus = field,
        SortingMessage::SetWatch(watch) => {
            dialog.focus = SourceField::When;
            if dialog.kind == SourceDialogKind::Watched {
                dialog.watch = watch;
            }
        }
        SortingMessage::SetTransfer(transfer) => {
            dialog.transfer = transfer;
            dialog.focus = SourceField::Files;
        }
        SortingMessage::SetNoText(no_text) => {
            dialog.no_text = no_text;
            dialog.focus = SourceField::NoText;
        }
        SortingMessage::SetSeveral(several) => {
            dialog.several = several;
            dialog.focus = SourceField::Several;
        }
        SortingMessage::StepChoice => dialog.step_choice(),
        _ => {}
    }
}

/// A folder of an existing source brings that source's settings along.
fn folder_picked(hermes: &mut Hermes, folder: PathBuf) {
    let Some(dialog) = hermes.source_dialog.as_mut() else {
        return;
    };
    let fresh = SourceDialog::new(dialog.kind, Some(&folder), &hermes.filing_config);
    if fresh.replacing.is_some() {
        *dialog = SourceDialog {
            focus: dialog.focus,
            ..fresh
        };
    } else {
        dialog.folder = folder.to_string_lossy().into_owned();
    }
}

fn write_task(
    write: impl FnOnce() -> Result<ncrs_config::config::Config, FilingWriteError> + Send + 'static,
) -> Task<Message> {
    Task::perform(
        tokio::task::spawn_blocking(move || write().map(|config| config.filing)),
        |joined| {
            sorting(SortingMessage::Written(Box::new(joined.unwrap_or_else(
                |join| {
                    Err(FilingWriteError::Io {
                        path: PathBuf::new(),
                        reason: join.to_string(),
                    })
                },
            ))))
        },
    )
}

fn save(hermes: &mut Hermes, ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Task<Message> {
    let Some(dialog) = hermes.source_dialog.as_ref() else {
        return Task::none();
    };
    if dialog.kind == SourceDialogKind::Once {
        return show_plan(hermes, ctx);
    }
    if !dialog.can_be_saved(&hermes.filing_config) {
        return Task::none();
    }
    let Some(config_path) = config_path(ctx, actions) else {
        return Task::none();
    };
    let draft = dialog.clone();
    let home = ctx.home.to_path_buf();
    if let Some(saving_dialog) = hermes.source_dialog.as_mut() {
        saving_dialog.saving = true;
    }
    write_task(move || {
        let mut source = draft.source(&home);
        source.path = ncrs_intake::canonical_path(&source.path).unwrap_or(source.path);
        upsert_sort_source(&config_path, &source, draft.replacing.as_deref())
    })
}

fn show_plan(hermes: &mut Hermes, ctx: &Ctx<'_>) -> Task<Message> {
    let generation = hermes.source_check;
    let Some(dialog) = hermes.source_dialog.as_mut() else {
        return Task::none();
    };
    if !dialog.can_be_saved(&hermes.filing_config) {
        return Task::none();
    }
    dialog.saving = true;
    let draft = dialog.clone();
    let home = ctx.home.to_path_buf();
    Task::perform(
        tokio::task::spawn_blocking(move || Box::new(draft.source(&home))),
        move |joined| match joined {
            Ok(source) => sorting(SortingMessage::PlanSource { generation, source }),
            Err(_) => sorting(SortingMessage::Cancel),
        },
    )
}

fn remove(
    hermes: &Hermes,
    index: usize,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(folder) = hermes
        .filing_config
        .sources
        .get(index)
        .map(|setting| setting.source.path.clone())
    else {
        return Task::none();
    };
    let Some(config_path) = config_path(ctx, actions) else {
        return Task::none();
    };
    write_task(move || remove_sort_source(&config_path, &folder))
}

/// The entry of the source at `old` with another folder; everything else of it
/// stays.
fn repick(
    hermes: &Hermes,
    old: PathBuf,
    folder: PathBuf,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    let Some(mut source) = hermes
        .filing_config
        .sources
        .iter()
        .find(|setting| setting.source.path == old)
        .map(|setting| setting.source.clone())
    else {
        return Task::none();
    };
    let Some(config_path) = config_path(ctx, actions) else {
        return Task::none();
    };
    write_task(move || {
        source.path = ncrs_intake::canonical_path(&folder).unwrap_or(folder);
        source.own_volume = own_volume_to_save(
            Some((&old, source.own_volume)),
            &source.path,
            is_on_own_volume(&source.path),
        );
        upsert_sort_source(&config_path, &source, Some(&old))
    })
}

fn config_path(ctx: &Ctx<'_>, actions: &mut Vec<Action>) -> Option<PathBuf> {
    if ctx.config_path.is_none() {
        actions.push(Action::ShowError(
            ctx.lang.text(Msg::FilingSetupNoConfigPath).to_owned(),
        ));
    }
    ctx.config_path.map(PathBuf::from)
}

fn written(
    hermes: &mut Hermes,
    result: Result<Filing, FilingWriteError>,
    ctx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) -> Task<Message> {
    match result {
        Ok(filing) => {
            hermes.source_dialog = None;
            let (applies, applying) = lifecycle::apply_saved_filing(hermes, filing, ctx);
            actions.push(Action::ShowNotice(lifecycle::saved_filing_notice(
                applies, ctx,
            )));
            actions.push(Action::RulesChanged);
            Task::batch([applying, load(hermes, ctx)])
        }
        Err(error) => {
            if let Some(dialog) = hermes.source_dialog.as_mut() {
                dialog.saving = false;
            }
            hermes_file::role_save_failed(hermes, &error, ctx, actions);
            Task::none()
        }
    }
}

fn undo(hermes: &Hermes, batch: i64, ctx: &Ctx<'_>) -> Task<Message> {
    let Some(access) = hermes
        .filing
        .as_ref()
        .and_then(|module| module.sorting_access(Arc::clone(&ctx.trash.0)))
    else {
        return Task::none();
    };
    Task::perform(
        tokio::task::spawn_blocking(move || {
            access
                .undo(BatchId::from_number(batch))
                .map(|report| report.left)
        }),
        |joined| {
            sorting(SortingMessage::Undone(
                joined.unwrap_or_else(|err| Err(err.to_string())),
            ))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use ncrs_config::config::TargetSetting;
    use ncrs_i18n::Language;
    use ncrs_index::{Kinds, TargetId};
    use ncrs_intake::SortTarget;

    use crate::ctx::Bin;

    fn filing_with(ids: &[&str]) -> Filing {
        Filing {
            targets: ids
                .iter()
                .map(|name| TargetSetting {
                    target: SortTarget {
                        id: TargetId::new(name).expect("id"),
                        name: (*name).to_owned(),
                        path: Path::new("/targets").join(name),
                        own_volume: None,
                    },
                    paused: None,
                })
                .collect(),
            ..Filing::default()
        }
    }

    fn hermes_with_dialog(ids: &[&str]) -> Hermes {
        let filing_config = filing_with(ids);
        let source_dialog = Some(SourceDialog::new(
            SourceDialogKind::Watched,
            None,
            &filing_config,
        ));
        Hermes {
            filing_config,
            source_dialog,
            ..Hermes::default()
        }
    }

    fn step(hermes: &mut Hermes, message: SortingMessage) -> Vec<Action> {
        let kinds = Kinds::default();
        let tags = ncrs_config::file_list_tags::FileListTags::default();
        let nowhere = PathBuf::from("/nowhere");
        let panel = |side| crate::ctx::PanelFacts {
            side,
            directory: &nowhere,
            request_id: 0,
            readable: true,
            visible: Vec::new(),
        };
        let ctx = Ctx {
            lang: Language::English,
            kinds: &kinds,
            file_list_tags: &tags,
            start_over_dialog_open: false,
            hermes_is_restarting: false,
            search_open: false,
            jobs_busy: false,
            watching_allowed: true,
            trash: Bin(ncrs_fs::system_trash(None)),
            filing_tab_source: None,
            config_path: None,
            home: &nowhere,
            filing_status_shortcut: None,
            active_panel_path: &nowhere,
            other_panel_path: &nowhere,
            now: std::time::SystemTime::now(),
            panels: [
                panel(ncrs_core::PanelSide::Left),
                panel(ncrs_core::PanelSide::Right),
            ],
            cursor_row: None,
            overlay_open: false,
            action_sources: Vec::new(),
        };
        let mut actions = Vec::new();
        drop(update(hermes, message, &ctx, &mut actions));
        actions
    }

    fn ticked(hermes: &Hermes) -> Vec<String> {
        hermes
            .source_dialog
            .iter()
            .flat_map(|dialog| &dialog.targets)
            .map(|id| id.as_str().to_owned())
            .collect()
    }

    #[test]
    fn a_new_target_is_ticked_when_the_config_was_read_again_before() {
        let mut hermes = hermes_with_dialog(&["main"]);
        tick_new_targets(&mut hermes, &TargetsMessage::Add);

        for _ in 0..2 {
            hermes.filing_config = filing_with(&["main", "t2"]);
        }
        let written = TargetsMessage::Written(Box::new(Ok(filing_with(&["main", "t2"]))));
        tick_new_targets(&mut hermes, &written);
        tick_new_targets(&mut hermes, &written);

        assert_eq!(ticked(&hermes), ["t2"]);
    }

    #[test]
    fn a_target_written_without_the_dialog_of_a_new_one_is_not_ticked() {
        let mut hermes = hermes_with_dialog(&["main"]);

        tick_new_targets(
            &mut hermes,
            &TargetsMessage::Written(Box::new(Ok(filing_with(&["main", "t2"])))),
        );

        assert!(ticked(&hermes).is_empty());
    }

    #[test]
    fn enter_on_the_button_for_a_new_target_opens_its_dialog() {
        let mut hermes = hermes_with_dialog(&["main"]);
        step(&mut hermes, SortingMessage::Focus(SourceField::AddTarget));

        step(&mut hermes, SortingMessage::Enter);

        assert!(hermes.target_dialog.is_some());
        assert!(hermes
            .source_dialog
            .as_ref()
            .is_some_and(|dialog| !dialog.saving));
    }

    #[test]
    fn enter_on_another_field_opens_no_target_dialog() {
        let mut hermes = hermes_with_dialog(&["main"]);

        step(&mut hermes, SortingMessage::Enter);

        assert!(hermes.target_dialog.is_none());
    }
}
