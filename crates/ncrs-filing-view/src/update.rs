//! What a message does to the filing tab.

use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;
use ncrs_filing::execute::TrashItem;
use ncrs_filing::sorting::{RowState, RunRequest};
use ncrs_i18n::Msg;
use ncrs_index::TargetId;
use ncrs_intake::{canonical_path, LocationTransfer, SortSource};
use ncrs_ui_kit::row_scroll::row_into_view;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::decide::{decide, Decided};
use crate::folders::{list_target_folders, read_existing};
use crate::left::link_of;
use crate::message::Message;
use crate::question::Question;
use crate::run::{carry_answer, carry_out, file_name, problems, Carried, Duplicates, RunProgress};
use crate::scroll::{LEFT_ROW_ID, LEFT_SCROLL_ID, RIGHT_ROW_ID, RIGHT_SCROLL_ID};
use crate::state::{Change, Outcome, Session, State};
use crate::texts::say;

/// The id of the field the new name is typed in.
pub const RENAME_FIELD: &str = "filing-rename";

type Step = (Task<Message>, Option<Action>);

fn nothing() -> Step {
    (Task::none(), None)
}

fn only(task: Task<Message>) -> Step {
    (task, None)
}

/// The target a rule of the file belongs to, where one rule has been tried.
fn tried_target(state: &RowState) -> Option<&TargetId> {
    match state {
        RowState::Place(place) | RowState::NameTaken { place, .. } => Some(&place.target),
        RowState::MissingValue { target, .. }
        | RowState::BadName { target, .. }
        | RowState::AlreadyThere { target, .. } => Some(target),
        RowState::Several(_)
        | RowState::NoRule
        | RowState::NoText
        | RowState::NotRead(_)
        | RowState::SetAside => None,
    }
}

/// A new session for `source`: the whole folder `once`, or the open questions
/// of a watched source. An open one is stopped, but one that is filing stays.
/// Nothing is filed by opening.
pub fn open(state: &mut State, source: SortSource, once: bool, ctx: &Ctx) -> Task<Message> {
    if state.session.as_ref().is_some_and(Session::is_filing) {
        return Task::none();
    }
    crate::state::stop(state);
    let mut session = Session::new(source, once);
    // What is known is shown at once; the rest is read after that.
    let task = decide_again(&mut session, false, ctx);
    state.session = Some(session);
    task
}

fn decide_again(session: &mut Session, read: bool, ctx: &Ctx) -> Task<Message> {
    let Some(access) = ctx.access.clone() else {
        session.error = Some(ctx.lang.text(Msg::IndexNotOpen).to_owned());
        return Task::none();
    };
    session.error = None;
    decide(session, access, read, ctx.lang)
}

/// Decides with what is known, now or once the work in hand is done.
fn decide_when_idle(session: &mut Session, ctx: &Ctx) -> Task<Message> {
    if session.planning || session.is_filing() {
        session.stale = true;
        return Task::none();
    }
    decide_again(session, false, ctx)
}

pub fn update(state: &mut State, message: Message, ctx: &Ctx) -> Step {
    match message {
        Message::Cancel => return cancel(state),
        // The files are where they are now, also when the tab has gone.
        Message::Carried(id, result) => return carried(state, id, result, ctx),
        Message::Answered(id, relative, result) => {
            return answered(state, id, &relative, result, ctx)
        }
        _ => {}
    }
    let Some(session) = state.session.as_mut() else {
        return nothing();
    };
    match message {
        Message::Planned {
            session: id,
            read,
            result,
        } => {
            if id == session.id {
                return only(planned(session, read, result, ctx));
            }
        }
        Message::RulesChanged => return only(decide_when_idle(session, ctx)),
        Message::Recheck => {
            if !session.planning && !session.is_filing() {
                session.outcome = None;
                forget_folders(session);
                return only(decide_again(session, session.once, ctx));
            }
        }
        Message::FoldersListed(id, folders) => {
            if session.id == id {
                session.listing = false;
                session.target_folders = folders;
                return only(read_existing(session));
            }
        }
        Message::ExistingRead(id, read) => {
            if session.id == id {
                for (key, names) in read {
                    session.reading_folders.remove(&key);
                    session.existing.insert(key, names);
                }
            }
        }
        Message::ToggleOnlyNew => {
            session.only_new = !session.only_new;
            if !session.only_new {
                return only(list_target_folders(session));
            }
        }
        Message::ToggleFolder(target, folder) => {
            let key = (target, folder);
            if !session.closed_folders.remove(&key) {
                session.closed_folders.insert(key);
            }
        }
        Message::Select(path) => select(session, path),
        Message::SelectNext => return only(move_selection(session, true)),
        Message::SelectPrevious => return only(move_selection(session, false)),
        Message::LinkNext => mark_link(session, true),
        Message::LinkPrevious => mark_link(session, false),
        // What follows changes what a run carries out.
        _ if session.running.is_some() => {}
        Message::File(transfer) => return only(file(session, transfer, ctx)),
        Message::ToggleTrashDupes => {
            session.trash_dupes =
                !session.trash_dupes && !session.copy_only() && session.trash_can_restore;
        }
        message => return edit(session, message, ctx),
    }
    nothing()
}

/// The changes the user makes to single files.
fn edit(session: &mut Session, message: Message, ctx: &Ctx) -> Step {
    match message {
        Message::PressMarkedLink => return press_marked_link(session, ctx),
        Message::MakeRule(from) => {
            let action = session.row(&from).map(|row| Action::OpenRuleForm {
                target: tried_target(&row.state)
                    .or_else(|| session.source.targets.first())
                    .cloned(),
                document: row.document,
                path: row.path.clone(),
            });
            return (Task::none(), action);
        }
        Message::SuggestRule(from) => {
            let source = session.source.path.clone();
            let action = session.row(&from).map(|row| Action::SuggestRule {
                source,
                path: row.path.clone(),
            });
            return (Task::none(), action);
        }
        Message::Assign(from) => {
            select(session, from);
            return pick_folder(session);
        }
        Message::PickFolder => return pick_folder(session),
        Message::FolderPicked(folder) => return only(folder_picked(session, folder, ctx)),
        Message::Leave(from) => return only(change(session, from, Change::Stay, ctx)),
        Message::Stay => return only(stay(session, ctx)),
        Message::Ask(from) => ask(session, from, ctx),
        Message::StartRename => return only(start_rename(session)),
        Message::RenameInput(typed) => {
            if session.renaming.is_some() {
                session.renaming = Some(typed);
            }
        }
        Message::RenameSubmit => return only(submit_rename(session, ctx)),
        Message::RenameCancel => session.renaming = None,
        Message::QuestionAnswer(chosen) => return only(answer_question(session, &chosen, ctx)),
        Message::QuestionConfirm => {
            let chosen = session
                .question
                .as_ref()
                .map_or_else(Vec::new, Question::confirmed);
            return only(answer_question(session, &chosen, ctx));
        }
        Message::QuestionLeave => {
            if let Some(question) = session.question.take() {
                return only(change(session, question.relative, Change::Stay, ctx));
            }
        }
        message => {
            if let Some(question) = session.question.as_mut() {
                move_in_question(question, &message);
            }
        }
    }
    nothing()
}

fn move_in_question(question: &mut Question, message: &Message) {
    match message {
        Message::QuestionToggle(at) => {
            question.marked = *at;
            question.toggle(*at);
        }
        Message::QuestionToggleMarked => question.toggle(question.marked),
        Message::QuestionNext => question.move_mark(true),
        Message::QuestionPrevious => question.move_mark(false),
        _ => {}
    }
}

fn planned(
    session: &mut Session,
    read: bool,
    result: Result<Box<Decided>, String>,
    ctx: &Ctx,
) -> Task<Message> {
    session.planning = false;
    session.reading = false;
    let decided = match result {
        Ok(decided) => *decided,
        Err(reason) => {
            // A stopped plan has no reason.
            session.error = Some(reason).filter(|reason| !reason.is_empty());
            return Task::none();
        }
    };
    let first = !session.planned;
    session.planned = true;
    session.rows = decided.rows;
    session.waiting = decided.waiting;
    session.targets = decided.targets;
    session.rule_names = decided.rule_names;
    session.trash_can_restore = decided.trash_can_restore;
    drop_old_changes(session);
    let folders = read_existing(session);
    // A run that was started meanwhile asks again when it ends.
    let again = if session.is_filing() {
        Task::none()
    } else if first && session.once && !read {
        decide_again(session, true, ctx)
    } else if session.stale {
        decide_again(session, false, ctx)
    } else {
        Task::none()
    };
    Task::batch([folders, again])
}

/// What no longer has a row or is in its place already.
fn drop_old_changes(session: &mut Session) {
    let rows = &session.rows;
    let open = |relative: &PathBuf| {
        rows.iter().any(|row| {
            row.relative == *relative && !matches!(row.state, RowState::AlreadyThere { .. })
        })
    };
    session.changes.retain(|relative, _| open(relative));
    if session.question.as_ref().is_some_and(|question| !open(&question.relative)) {
        session.question = None;
    }
    if !session
        .selected
        .as_ref()
        .is_some_and(|selected| rows.iter().any(|row| row.relative == *selected))
    {
        session.selected = None;
        session.renaming = None;
    }
    session.marked_link = None;
}

fn forget_folders(session: &mut Session) {
    session.existing.clear();
    session.reading_folders.clear();
}

fn select(session: &mut Session, path: PathBuf) {
    session.renaming = None;
    session.marked_link = None;
    session.selected = Some(path);
}

fn move_selection(session: &mut Session, forward: bool) -> Task<Message> {
    let at = session
        .selected
        .as_ref()
        .and_then(|selected| session.rows.iter().position(|row| row.relative == *selected));
    let next = match (at, forward) {
        (None, _) => 0,
        (Some(index), true) => (index + 1).min(session.rows.len().saturating_sub(1)),
        (Some(index), false) => index.saturating_sub(1),
    };
    if let Some(row) = session.rows.get(next) {
        let path = row.relative.clone();
        select(session, path);
    }
    Task::batch([
        row_into_view(LEFT_SCROLL_ID, LEFT_ROW_ID),
        row_into_view(RIGHT_SCROLL_ID, RIGHT_ROW_ID),
    ])
}

fn mark_link(session: &mut Session, forward: bool) {
    let links = session
        .selected_row()
        .map_or(0, |row| session.links(row).len());
    session.marked_link = match (session.marked_link, forward) {
        _ if links == 0 => None,
        (None, true) => Some(0),
        (None, false) => None,
        (Some(at), true) => Some((at + 1).min(links - 1)),
        (Some(at), false) => at.checked_sub(1),
    };
}

fn press_marked_link(session: &mut Session, ctx: &Ctx) -> Step {
    let pressed = session.marked_link.zip(session.selected_row()).and_then(|(at, row)| {
        let link = *session.links(row).get(at)?;
        Some(link_of(link, row).1)
    });
    match pressed {
        Some(message) => edit(session, message, ctx),
        None => nothing(),
    }
}

fn start_rename(session: &mut Session) -> Task<Message> {
    let place = session
        .selected_row()
        .filter(|_| session.once)
        .and_then(|row| session.one_place(row));
    let Some((_, relative)) = place else {
        return Task::none();
    };
    session.renaming = Some(file_name(&relative));
    iced::widget::operation::focus(RENAME_FIELD)
}

fn submit_rename(session: &mut Session, ctx: &Ctx) -> Task<Message> {
    let Some(typed) = session.renaming.as_ref() else {
        return Task::none();
    };
    let name = typed.trim().to_owned();
    let is_only_dots = name.chars().all(|letter| letter == '.');
    if is_only_dots || name.contains(std::path::is_separator) {
        session.error = Some(ctx.lang.text(Msg::FilingViewErrorName).to_owned());
        return Task::none();
    }
    let place = session.selected_row().and_then(|row| {
        let (target, relative) = session.one_place(row)?;
        Some((row.relative.clone(), (target, relative.with_file_name(&name))))
    });
    session.renaming = None;
    session.error = None;
    match place {
        Some((from, place)) => change(session, from, Change::Places(vec![place]), ctx),
        None => Task::none(),
    }
}

fn pick_folder(session: &Session) -> Step {
    let Some(row) = session.selected_row() else {
        return nothing();
    };
    let mut targets = session.source_targets().map(|(_, target)| target);
    let current = session.one_place(row).and_then(|(id, relative)| {
        let target = session.targets.iter().find(|target| target.id == id)?;
        let folder = target.path.join(relative.parent()?);
        Some(if folder.is_dir() {
            folder
        } else {
            target.path.clone()
        })
    });
    let start = current.or_else(|| targets.next().map(|target| target.path.clone()));
    (
        Task::none(),
        start.map(|start| Action::PickFolder { start }),
    )
}

/// The target of the source that `folder` is in; of nested ones the inner one.
fn target_of(session: &Session, folder: &std::path::Path) -> Option<(TargetId, PathBuf)> {
    session
        .source_targets()
        .filter_map(|(_, target)| {
            let relative = folder.strip_prefix(&target.path).ok()?;
            Some((target.id.clone(), relative.to_path_buf()))
        })
        .min_by_key(|(_, relative)| relative.components().count())
}

fn folder_picked(session: &mut Session, folder: Option<PathBuf>, ctx: &Ctx) -> Task<Message> {
    let Some(picked) = folder else {
        return Task::none();
    };
    let picked = canonical_path(&picked).unwrap_or(picked);
    let Some(row) = session.selected_row() else {
        return Task::none();
    };
    let Some((target, below)) = target_of(session, &picked) else {
        session.error = Some(ctx.lang.text(Msg::FilingViewErrorOutside).to_owned());
        return Task::none();
    };
    let name = session
        .one_place(row)
        .map_or_else(|| file_name(&row.relative), |(_, relative)| file_name(&relative));
    let from = row.relative.clone();
    session.error = None;
    change(
        session,
        from,
        Change::Places(vec![(target, below.join(name))]),
        ctx,
    )
}

fn stay(session: &mut Session, ctx: &Ctx) -> Task<Message> {
    let Some(selected) = session.selected.clone() else {
        return Task::none();
    };
    if session.changes.get(&selected) == Some(&Change::Stay) {
        session.changes.remove(&selected);
        return read_existing(session);
    }
    change(session, selected, Change::Stay, ctx)
}

fn ask(session: &mut Session, from: PathBuf, ctx: &Ctx) {
    let places = match session.row(&from).map(|row| &row.state) {
        Some(RowState::Several(places)) => places.clone(),
        _ => return,
    };
    select(session, from.clone());
    session.question = Some(Question::new(session, from, &places, ctx.lang));
}

fn answer_question(session: &mut Session, chosen: &[usize], ctx: &Ctx) -> Task<Message> {
    let Some(places) = session
        .question
        .as_ref()
        .map(|question| question.places_at(chosen))
        .filter(|places| !places.is_empty())
    else {
        return Task::none();
    };
    match session.question.take() {
        Some(question) => change(session, question.relative, Change::Places(places), ctx),
        None => Task::none(),
    }
}

/// In a plan the decision waits for the run; the question of a watched source
/// is answered right away.
fn change(session: &mut Session, from: PathBuf, change: Change, ctx: &Ctx) -> Task<Message> {
    session.renaming = None;
    session.marked_link = None;
    session.outcome = None;
    if session.once {
        session.changes.insert(from, change);
        return read_existing(session);
    }
    if session.answering.contains(&from) {
        return Task::none();
    }
    let Some(document) = session.row(&from).and_then(|row| row.document) else {
        return Task::none();
    };
    let Some(access) = ctx.access.clone() else {
        session.error = Some(ctx.lang.text(Msg::IndexNotOpen).to_owned());
        return Task::none();
    };
    session.error = None;
    session.answering.insert(from.clone());
    let id = session.id;
    let lang = ctx.lang;
    let source = session.source.clone();
    let name = run_name(session, Msg::HermesChatRunName, ctx);
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                carry_answer(&access, &source, document, change, &name, lang)
            })
            .await
            .unwrap_or_else(|err| Err(err.to_string()))
            .map(Box::new)
        },
        move |result| Message::Answered(id, from.clone(), result),
    )
}

fn run_name(session: &Session, msg: Msg, ctx: &Ctx) -> String {
    say(ctx.lang, msg, &[("folder", &session.source_name())])
}

fn duplicates(session: &Session, name: String) -> Duplicates {
    let wanted = session.trash_dupes && !session.copy_only();
    let items = session
        .rows
        .iter()
        .filter(|_| wanted)
        .filter_map(|row| match (&row.state, row.hash) {
            (RowState::AlreadyThere { path, .. }, Some(hash)) => Some(TrashItem {
                path: row.path.clone(),
                expected_hash: hash,
                keeper: path.clone(),
            }),
            _ => None,
        })
        .collect();
    Duplicates { name, items }
}

fn file(session: &mut Session, transfer: LocationTransfer, ctx: &Ctx) -> Task<Message> {
    if !session.can_file() {
        return Task::none();
    }
    let Some(access) = ctx.access.clone() else {
        session.error = Some(ctx.lang.text(Msg::IndexNotOpen).to_owned());
        return Task::none();
    };
    let request = RunRequest {
        name: run_name(session, Msg::HermesChatRunName, ctx),
        source: session.source.clone(),
        entries: session.entries(),
        transfer,
    };
    let duplicates = duplicates(session, run_name(session, Msg::HermesChatTrashRunName, ctx));
    let progress = Arc::new(RunProgress::new(request.entries.len()));
    session.running = Some(Arc::clone(&progress));
    session.renaming = None;
    session.marked_link = None;
    session.question = None;
    session.outcome = None;
    session.error = None;
    let id = session.id;
    let lang = ctx.lang;
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                carry_out(&access, &request, &duplicates, progress, lang)
            })
            .await
            .unwrap_or_else(|err| Err(err.to_string()))
            .map(Box::new)
        },
        move |result| Message::Carried(id, result),
    )
}

/// The files that still lie in the source without a place: all rows but the
/// filed ones, and a duplicate that is trashed or copied is not one either.
fn left_after(session: &Session, carried: &Carried) -> usize {
    let present = session
        .rows
        .iter()
        .filter(|row| matches!(row.state, RowState::AlreadyThere { .. }))
        .count();
    let settled = match carried.transfer {
        LocationTransfer::Copy => present,
        LocationTransfer::Move => carried.trashed,
    };
    session
        .rows
        .len()
        .saturating_sub(carried.report.done() + settled)
}

fn filed_action(source: PathBuf, carried: &Carried, left: usize) -> Option<Action> {
    let batch = carried.report.run;
    (batch.is_some() || carried.trashed > 0).then(|| Action::Filed {
        source,
        transfer: carried.transfer,
        filed: carried.report.done(),
        left,
        batch,
    })
}

fn carried(
    state: &mut State,
    id: u64,
    result: Result<Box<Carried>, String>,
    ctx: &Ctx,
) -> Step {
    let Some(session) = state.session.as_mut().filter(|session| session.id == id) else {
        return nothing();
    };
    session.running = None;
    let carried = match result {
        Ok(carried) => carried,
        Err(reason) => {
            let task = decide_again(session, false, ctx);
            session.error = Some(reason);
            return only(task);
        }
    };
    let left = left_after(session, &carried);
    for entry in &carried.report.entries {
        if entry.is_done() {
            if let Some(row) = session.rows.iter().find(|row| row.path == entry.path) {
                session.changes.remove(&row.relative);
            }
        }
    }
    session.outcome = Some(Outcome {
        filed: carried.report.done(),
        left,
        trashed: carried.trashed,
        problems: problems(ctx.lang, &carried.report),
    });
    forget_folders(session);
    let task = decide_again(session, false, ctx);
    session.error = carried.trash_problem.clone();
    let action = filed_action(session.source.path.clone(), &carried, left);
    (task, action)
}

fn answered(
    state: &mut State,
    id: u64,
    relative: &std::path::Path,
    result: Result<Box<Carried>, String>,
    ctx: &Ctx,
) -> Step {
    let Some(session) = state.session.as_mut().filter(|session| session.id == id) else {
        return nothing();
    };
    session.answering.remove(relative);
    let carried = match result {
        Ok(carried) => carried,
        Err(reason) => {
            let task = decide_when_idle(session, ctx);
            session.error = Some(reason);
            return only(task);
        }
    };
    let left = session.rows.len().saturating_sub(1);
    let found = problems(ctx.lang, &carried.report);
    if !found.is_empty() {
        session.outcome = Some(Outcome {
            problems: found,
            left: session.rows.len(),
            ..Outcome::default()
        });
    }
    forget_folders(session);
    let action = filed_action(session.source.path.clone(), &carried, left);
    (decide_when_idle(session, ctx), action)
}

/// A run is stopped first and the tab stays to show what it did; an answer
/// that is being carried out is waited for.
fn cancel(state: &mut State) -> Step {
    if let Some(session) = &state.session {
        if let Some(running) = &session.running {
            running.stop();
            return nothing();
        }
        if session.is_filing() {
            return nothing();
        }
    }
    crate::state::stop(state);
    state.session = None;
    (Task::none(), Some(Action::Close))
}
