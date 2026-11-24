//! What a message does to the filing tab.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use iced::Task;
use ncrs_ai::AiError;
use ncrs_filing::chat_work::{
    mark_files, plan_live, run_plan, tally, ChatAccess, ChatResult, ChatWork, LiveFile, LivePlan,
    LiveRun, LiveState, Override, PlanFailure, TrashItem,
};
use ncrs_i18n::Msg;
use ncrs_index::Transfer;
use ncrs_ui_kit::row_scroll::row_into_view;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::{FiledReport, Message};
use crate::scroll::{LEFT_ROW_ID, LEFT_SCROLL_ID, RIGHT_ROW_ID, RIGHT_SCROLL_ID};
use crate::state::{Session, State};
use crate::texts::{say, tags};

/// The id of the field the new name is typed in.
pub const RENAME_FIELD: &str = "filing-rename";

/// How deep the folders of the write folder are listed for the full tree.
const FOLDER_DEPTH: usize = 3;

type Step = (Task<Message>, Option<Action>);

fn nothing() -> Step {
    (Task::none(), None)
}

/// A new session for `source` into `target`; a running one is stopped.
pub fn start(state: &mut State, source: PathBuf, target: PathBuf, ctx: &Ctx) -> Task<Message> {
    crate::state::stop(state);
    let run = Arc::new(LiveRun::default());
    let mut session = Session::new(source.clone(), target.clone(), Arc::clone(&run));
    session.copy_only = ctx
        .access
        .as_ref()
        .is_some_and(|access| access.copies_from(&source));
    let (Some(access), Some(model)) = (ctx.access.clone(), ctx.model.clone()) else {
        let reason = ctx.lang.text(Msg::FilingViewErrorNotReady).to_owned();
        session.finished = Some(Err(reason.clone()));
        session.error = Some(reason);
        state.session = Some(session);
        return Task::none();
    };
    let id = session.id;
    state.session = Some(session);
    let tags = tags(ctx.lang);
    let lang = ctx.lang;
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                plan_live(&access, &source, &target, &*model, &tags, &run)
                    .map_err(|failure| failure_text(lang, &failure))
            })
            .await
            .unwrap_or_else(|err| Err(err.to_string()))
        },
        move |result| Message::PlanFinished(id, result),
    )
}

/// What the user reads when planning ends early. The technical reason of a
/// model failure is not shown.
pub fn failure_text(lang: ncrs_i18n::Language, failure: &PlanFailure) -> String {
    match failure {
        PlanFailure::Model(AiError::Unsupported) => {
            lang.text(Msg::FilingViewErrorFolderChoiceUnsupported)
        }
        PlanFailure::Model(AiError::ConsentRequired(_)) => {
            lang.text(Msg::FilingViewErrorFolderConsentMissing)
        }
        PlanFailure::Model(_) => lang.text(Msg::FilingViewErrorModelFailed),
        PlanFailure::Other(reason) => reason.as_str(),
    }
    .to_owned()
}

pub fn update(state: &mut State, message: Message, ctx: &Ctx) -> Step {
    match message {
        Message::Cancel => return cancel(state, ctx),
        Message::Filed(result) => return filed(state, result),
        Message::File(transfer) => return file(state, transfer, false, ctx),
        Message::FileSure => return file(state, Transfer::Move, true, ctx),
        Message::Marked(_) => return nothing(),
        _ => {}
    }
    let Some(session) = state.session.as_mut() else {
        return nothing();
    };
    let mut task = Task::none();
    match message {
        Message::Tick => {
            session.take_snapshot();
            task = read_existing(session);
        }
        Message::PlanFinished(id, _) if id != session.id => {}
        Message::PlanFinished(_, result) => {
            session.take_snapshot();
            task = read_existing(session);
            if let Err(reason) = &result {
                session.error = Some(reason.clone());
            } else if session
                .plan
                .as_ref()
                .is_some_and(|plan| plan.folder_choice_unsupported)
            {
                session.error = Some(
                    ctx.lang
                        .text(Msg::FilingViewErrorFolderChoiceUnsupported)
                        .to_owned(),
                );
            }
            session.finished = Some(result);
        }
        Message::Select(path) => {
            session.renaming = None;
            session.selected = Some(path);
        }
        Message::SelectNext => return move_selection(session, true),
        Message::SelectPrevious => return move_selection(session, false),
        Message::StartRename => return start_rename(session),
        Message::RenameInput(typed) => {
            if session.renaming.is_some() {
                session.renaming = Some(typed);
            }
        }
        Message::RenameSubmit => submit_rename(session, ctx),
        Message::RenameCancel => session.renaming = None,
        Message::PickFolder => return pick_folder(session),
        Message::FolderPicked(folder) => task = folder_picked(session, folder, ctx),
        Message::Stay => {
            if let Some(selected) = session.selected.clone() {
                session.set_override(selected, Override::Stay);
                session.renaming = None;
                task = read_existing(session);
            }
        }
        Message::ToggleOnlyNew => {
            session.only_new = !session.only_new;
            if !session.only_new {
                task = list_target_folders(session);
            }
        }
        Message::FoldersListed(id, folders) => {
            if session.id == id {
                session.listing = false;
                session.target_folders = folders;
                task = read_existing(session);
            }
        }
        Message::ExistingRead(id, read) => {
            if session.id == id {
                for (folder, names) in read {
                    session.reading.remove(&folder);
                    session.existing.insert(folder, names);
                }
            }
        }
        Message::ToggleTrashDupes => {
            session.trash_dupes = !session.trash_dupes && !session.copy_only;
        }
        Message::ToggleFolder(folder) => {
            if !session.closed_folders.remove(&folder) {
                session.closed_folders.insert(folder);
            }
        }
        Message::Cancel
        | Message::Filed(_)
        | Message::File(_)
        | Message::FileSure
        | Message::Marked(_) => {}
    }
    (task, None)
}

/// The folders of `root`, relative to it, down to `FOLDER_DEPTH` levels.
fn list_folders(root: &Path) -> BTreeSet<PathBuf> {
    fn walk(root: &Path, relative: &Path, found: &mut BTreeSet<PathBuf>) {
        if relative.components().count() >= FOLDER_DEPTH {
            return;
        }
        let Ok(entries) = std::fs::read_dir(root.join(relative)) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let is_folder = entry.file_type().is_ok_and(|kind| kind.is_dir());
            if !is_folder || name.to_string_lossy().starts_with('.') {
                continue;
            }
            let child = relative.join(name);
            walk(root, &child, found);
            found.insert(child);
        }
    }
    let mut found = BTreeSet::new();
    walk(root, Path::new(""), &mut found);
    found
}

/// Lists the folders of the write folder off the UI thread.
fn list_target_folders(session: &mut Session) -> Task<Message> {
    if session.listing {
        return Task::none();
    }
    session.listing = true;
    let id = session.id;
    let target = session.target.clone();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || list_folders(&target))
                .await
                .unwrap_or_default()
        },
        move |folders| Message::FoldersListed(id, folders),
    )
}

/// Reads the files of every folder of the write folder the plan files into,
/// once per folder, off the UI thread.
fn read_existing(session: &mut Session) -> Task<Message> {
    let Some(plan) = session.shown_ref() else {
        return Task::none();
    };
    let mut wanted: BTreeSet<PathBuf> = plan
        .files
        .iter()
        .filter_map(|file| match &file.state {
            LiveState::Filed { folder, .. } => Some(folder.clone()),
            _ => None,
        })
        .collect();
    if !session.only_new {
        wanted.extend(session.target_folders.iter().cloned());
    }
    drop(plan);
    let missing: Vec<PathBuf> = wanted
        .into_iter()
        .filter(|folder| {
            !session.existing.contains_key(folder) && !session.reading.contains(folder)
        })
        .collect();
    if missing.is_empty() {
        return Task::none();
    }
    session.reading.extend(missing.iter().cloned());
    let id = session.id;
    let target = session.target.clone();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                missing
                    .into_iter()
                    .map(|folder| {
                        let names = names_in(&target.join(&folder));
                        (folder, names)
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default()
        },
        move |read| Message::ExistingRead(id, read),
    )
}

/// The names of the files in `folder`, sorted, without the hidden ones.
fn names_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names
}

fn move_selection(session: &mut Session, forward: bool) -> Step {
    let Some(plan) = &session.plan else {
        return nothing();
    };
    let at = session
        .selected
        .as_ref()
        .and_then(|selected| plan.files.iter().position(|file| file.from == *selected));
    let next = match (at, forward) {
        (None, _) => 0,
        (Some(index), true) => (index + 1).min(plan.files.len().saturating_sub(1)),
        (Some(index), false) => index.saturating_sub(1),
    };
    if let Some(file) = plan.files.get(next) {
        session.renaming = None;
        session.selected = Some(file.from.clone());
    }
    let scroll = Task::batch([
        row_into_view(LEFT_SCROLL_ID, LEFT_ROW_ID),
        row_into_view(RIGHT_SCROLL_ID, RIGHT_ROW_ID),
    ]);
    (scroll, None)
}

fn selected_file(session: &Session) -> Option<LiveFile> {
    session.file(session.selected.as_ref()?)
}

fn start_rename(session: &mut Session) -> Step {
    let Some(LiveState::Filed { name, .. }) = selected_file(session).map(|file| file.state) else {
        return nothing();
    };
    session.renaming = Some(name);
    (iced::widget::operation::focus(RENAME_FIELD), None)
}

fn submit_rename(session: &mut Session, ctx: &Ctx) {
    let (Some(typed), Some(selected)) = (session.renaming.as_ref(), session.selected.clone())
    else {
        return;
    };
    let name = typed.trim().to_owned();
    if name.is_empty() || name == "." || name == ".." || name.contains(std::path::is_separator) {
        session.error = Some(ctx.lang.text(Msg::FilingViewErrorName).to_owned());
        return;
    }
    session.error = None;
    session.set_override(selected, Override::Name(name));
    session.renaming = None;
}

fn pick_folder(session: &Session) -> Step {
    let Some(file) = selected_file(session) else {
        return nothing();
    };
    let current = match file.state {
        LiveState::Filed { folder, .. } => session.target.join(folder),
        _ => session.target.clone(),
    };
    let start = if current.is_dir() {
        current
    } else {
        session.target.clone()
    };
    (Task::none(), Some(Action::PickFolder { start }))
}

fn folder_picked(session: &mut Session, folder: Option<PathBuf>, ctx: &Ctx) -> Task<Message> {
    let (Some(picked), Some(selected)) = (folder, session.selected.clone()) else {
        return Task::none();
    };
    match picked.strip_prefix(&session.target) {
        Ok(relative) => {
            session.error = None;
            session.set_override(selected, Override::Folder(relative.to_path_buf()));
            read_existing(session)
        }
        Err(_) => {
            session.error = Some(say(
                ctx.lang,
                Msg::FilingViewErrorOutside,
                &[("target", &session.target.display().to_string())],
            ));
            Task::none()
        }
    }
}

fn cancel(state: &mut State, ctx: &Ctx) -> Step {
    // A run that is carrying files out reports to the chat when it ends; the
    // tab stays until then.
    if state.session.as_ref().is_some_and(|session| session.filing) {
        return nothing();
    }
    let Some(session) = state.session.take() else {
        return (Task::none(), Some(Action::Close));
    };
    session.run.stop.store(true, Ordering::Relaxed);
    let (Some(access), Some(plan)) = (ctx.access.clone(), session.shown_plan()) else {
        return (Task::none(), Some(Action::Close));
    };
    let tags = tags(ctx.lang);
    let task = Task::perform(
        async move {
            tokio::task::spawn_blocking(move || mark_files(&access, &plan, &tags))
                .await
                .unwrap_or_else(|err| Err(err.to_string()))
        },
        Message::Marked,
    );
    (task, Some(Action::Close))
}

fn file(state: &mut State, transfer: Transfer, sure_only: bool, ctx: &Ctx) -> Step {
    let Some(session) = state.session.as_mut() else {
        return nothing();
    };
    if session.filing || !session.plan_is_done() {
        return nothing();
    }
    if session.copy_only && transfer == Transfer::Move {
        return nothing();
    }
    if session.shown_ref().is_none_or(|plan| {
        let counts = tally(&plan);
        let carried = if sure_only {
            counts.filed - counts.unsure
        } else {
            counts.filed
        };
        carried == 0
    }) {
        return nothing();
    }
    let (Some(access), Some(plan)) = (ctx.access.clone(), session.shown_plan()) else {
        session.error = Some(ctx.lang.text(Msg::FilingViewErrorNotReady).to_owned());
        return nothing();
    };
    session.filing = true;
    session.error = None;
    let unexpected = ctx.lang.text(Msg::FilingViewErrorFilingFailed).to_owned();
    let folder = session.source_name();
    let run_name = say(ctx.lang, Msg::HermesChatRunName, &[("folder", &folder)]);
    let trash_name = say(
        ctx.lang,
        Msg::HermesChatTrashRunName,
        &[("folder", &folder)],
    );
    let tags = tags(ctx.lang);
    let trash = session.trash_dupes;
    let task = Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                carry_out(
                    &access,
                    &plan,
                    transfer,
                    sure_only,
                    (run_name, trash_name),
                    trash,
                    (&tags, &unexpected),
                )
            })
            .await
            .unwrap_or_else(|err| Err(err.to_string()))
        },
        Message::Filed,
    );
    (task, None)
}

/// Tags the files that stay, files the rest and, when asked, trashes the
/// duplicates in the source.
fn carry_out(
    access: &ChatAccess,
    plan: &LivePlan,
    transfer: Transfer,
    sure_only: bool,
    names: (String, String),
    trash: bool,
    (tags, unexpected): (&ncrs_filing::chat_work::LiveTags, &str),
) -> Result<FiledReport, String> {
    let (run_name, trash_name) = names;
    let _running = access.begin_work();
    mark_files(access, plan, tags)?;
    let ChatResult::Filed(report) = access.run(ChatWork::File {
        run: run_plan(&carried_plan(plan, sure_only), transfer, run_name),
    }) else {
        return Err(unexpected.to_owned());
    };
    let carried = report?;
    let mut trashed = 0;
    if trash {
        let items: Vec<TrashItem> = plan
            .files
            .iter()
            .filter_map(|file| match (&file.state, file.hash) {
                (LiveState::AlreadyThere { at }, Some(hash)) => Some(TrashItem {
                    path: plan.source_root.join(&file.from),
                    expected_hash: hash,
                    keeper: plan.target_root.join(at),
                }),
                _ => None,
            })
            .collect();
        if !items.is_empty() {
            let ChatResult::Trashed(result) = access.run(ChatWork::TrashDuplicates {
                name: trash_name,
                root: plan.source_root.clone(),
                items,
            }) else {
                return Err(unexpected.to_owned());
            };
            trashed = result?.trashed;
        }
    }
    let left = plan
        .files
        .iter()
        .filter(|file| match &file.state {
            LiveState::Filed { unsure: true, .. } if sure_only => true,
            LiveState::Filed { folder, name, .. } => {
                carried.skipped > 0 && not_carried(plan, transfer, &file.from, &folder.join(name))
            }
            LiveState::AlreadyThere { .. } => !trash,
            _ => true,
        })
        .map(|file| file.from.clone())
        .collect();
    Ok(FiledReport {
        transfer,
        done: carried.done,
        trashed,
        left,
        batch: carried.batch,
    })
}

/// The plan as it is carried out: without the unsure files when only the sure
/// ones go.
pub fn carried_plan(plan: &LivePlan, sure_only: bool) -> LivePlan {
    let mut carried = plan.clone();
    if sure_only {
        carried
            .files
            .retain(|file| !matches!(file.state, LiveState::Filed { unsure: true, .. }));
    }
    carried
}

/// Whether a file the plan filed is still undone after a run that skipped some:
/// it is still in the source when moved, and missing from the target when
/// copied.
fn not_carried(plan: &LivePlan, transfer: Transfer, from: &Path, to: &Path) -> bool {
    match transfer {
        Transfer::Move => plan.source_root.join(from).exists(),
        Transfer::Copy | Transfer::Trash => !plan.target_root.join(to).exists(),
    }
}

fn filed(state: &mut State, result: Result<FiledReport, String>) -> Step {
    let Some(session) = state.session.as_mut() else {
        return nothing();
    };
    match result {
        Ok(report) => {
            let source = session.source.clone();
            let then_continuous = session.then_continuous;
            state.session = None;
            (
                Task::none(),
                Some(Action::Finished {
                    source,
                    transfer: report.transfer,
                    filed: report.done,
                    left: report.left,
                    batch: report.batch,
                    then_continuous,
                }),
            )
        }
        Err(reason) => {
            session.filing = false;
            session.error = Some(reason);
            nothing()
        }
    }
}

/// Whether a plan can be carried out now, for the buttons.
pub fn can_file(session: &Session) -> bool {
    session.plan_is_done() && !session.filing
}
