//! The actions of #338: each message starts one call to git on the blocking
//! pool, and the answer reloads what the tab shows.

#![allow(
    clippy::wildcard_enum_match_arm,
    reason = "this module takes a group of messages out of the big match in `update`; the rest never arrives here"
)]

use iced::widget::{operation, text_editor};
use iced::Task;
use ncrs_git::{ActionError, ChangedFile, CommitMessage, ConflictChoice, DiffHunk, UndoSummary};
use ncrs_i18n::{fill_template, Language, Msg};

use crate::action::Action;
use crate::actions::{
    Actions, ChangeSide, Conflict, ConflictAction, Dialog, LocalChanges, MenuAction, MenuTarget,
    MergeInProgress, NewBranch, Notice, Question, RenameBranch, Side, SyncBar, Unavailable,
    WipFocus,
};
use crate::ctx::Ctx;
use crate::job::{Done, Finished, Job};
use crate::load;
use crate::message::Message;
use crate::snapshot::{self, IntegrationSnapshot, Snapshot};
use crate::state::{Area, State};
use crate::update::{ensure_detail, failed, menu_target_of, quiet, Outcome};
use crate::view::{NAME_ID, RENAME_ID};
use crate::wip;

pub fn act(state: &mut State, message: Message, ctx: &Ctx) -> Outcome {
    match message {
        Message::ActionsLoaded(generation, result) => loaded(state, ctx, generation, *result),
        Message::JobDone(finished) => finished_job(state, ctx, *finished),
        Message::SelectChange(side, index) => select_change(state, side, index),
        Message::Stage(index) => file_job(state, ChangeSide::Unstaged, index, |path| {
            Job::Stage(vec![path])
        }),
        Message::Unstage(index) => file_job(state, ChangeSide::Staged, index, |path| {
            Job::Unstage(vec![path])
        }),
        Message::DiscardFile(index) => {
            let file = state
                .actions
                .local
                .as_ref()
                .and_then(|local| local.unstaged.get(index))
                .map(|file| file.path.clone());
            ask(state, file.map(|path| Question::DiscardFile { path }))
        }
        Message::StageAll => start(state, Job::StageAll),
        Message::UnstageAll => start(state, Job::UnstageAll),
        Message::DiscardAll => {
            let count = state
                .actions
                .local
                .as_ref()
                .map_or(0, |local| local.unstaged.len());
            ask(state, (count > 0).then_some(Question::DiscardAll { count }))
        }
        Message::StageHunk(index) => hunk_job(state, ChangeSide::Unstaged, index, |path, hunk| {
            Job::StageHunk { path, hunk }
        }),
        Message::UnstageHunk(index) => hunk_job(state, ChangeSide::Staged, index, |path, hunk| {
            Job::UnstageHunk { path, hunk }
        }),
        Message::DiscardHunk(index) => {
            hunk_job(state, ChangeSide::Unstaged, index, |path, hunk| {
                Job::DiscardHunk { path, hunk }
            })
        }
        Message::CommitTitleChanged(title) => {
            if let Some(local) = state.actions.local.as_mut() {
                local.title = title;
            }
            wip::typed_in(state, WipFocus::Title);
            quiet(Task::none())
        }
        Message::CommitBodyEdited(edit) => {
            if let Some(local) = state.actions.local.as_mut() {
                local.body.perform(edit);
            }
            wip::typed_in(state, WipFocus::Body);
            quiet(Task::none())
        }
        Message::ToggleWipSection(side) => {
            if let Some(local) = state.actions.local.as_mut() {
                if !local.folded.remove(&side) {
                    local.folded.insert(side);
                }
            }
            quiet(Task::none())
        }
        Message::ToggleAmend => {
            wip::toggle_amend(state);
            quiet(Task::none())
        }
        Message::TogglePushAfter => {
            wip::toggle_push_after(state);
            quiet(Task::none())
        }
        Message::ToggleStageSelected => wip::toggle_stage_selected(state),
        Message::FocusCommitTitle => quiet(wip::focus_title(state)),
        Message::WipFocused(focus) => quiet(wip::focused(state, focus)),
        Message::WipFieldPressed(focus) => {
            state.area = Area::Detail;
            wip::typed_in(state, focus);
            quiet(Task::none())
        }
        Message::IgnoreFile(index) => wip::ignore_file(state, index),
        Message::IgnoreSelected => wip::ignore_selected(state),
        Message::IgnoreChoiceChanged(choice) => {
            wip::choose_ignore(state, choice);
            quiet(Task::none())
        }
        Message::OpenSelectedInEditor => wip::open_selected_in_editor(state),
        Message::ConflictShortcut(ConflictAction::OpenEditor) if state.actions.merge.is_none() => {
            wip::open_selected_in_editor(state)
        }
        Message::Commit => commit(state, false),
        Message::CommitAndPush => commit(state, true),
        Message::Fetch => start(state, Job::Fetch),
        Message::Pull | Message::PullFirst => start(state, Job::Pull(None)),
        Message::PullWith(strategy) => start(state, Job::Pull(Some(strategy))),
        Message::Push => start(state, Job::Push),
        Message::Stash => {
            let stash_name = fill_template(
                ctx.lang.text(Msg::GitViewStashMessage),
                &[("{branch}", &current_branch(state))],
            );
            start(
                state,
                Job::StashPush {
                    message: stash_name,
                },
            )
        }
        Message::Undo => match state.actions.undo.clone() {
            Some(record) => start(state, Job::Undo(record)),
            None => quiet(Task::none()),
        },
        Message::MenuPressed(action) => menu_pressed(state, action),
        Message::NewBranchNameChanged(name) => {
            if let Some(draft) = state.actions.new_branch.as_mut() {
                draft.problem = name
                    .contains(char::is_whitespace)
                    .then(|| ctx.lang.text(Msg::GitViewNewBranchSpaces).to_owned());
                draft.name = name;
            }
            quiet(Task::none())
        }
        Message::ToggleNewBranchCheckout => {
            if let Some(draft) = state.actions.new_branch.as_mut() {
                draft.checkout = !draft.checkout;
            }
            quiet(Task::none())
        }
        Message::CreateBranch => create_branch(state),
        Message::CloseDialog => {
            state.actions.new_branch = None;
            state.actions.dialog = None;
            state.actions.menu = None;
            quiet(Task::none())
        }
        Message::RenameNameChanged(name) => {
            if let Some(Dialog::Rename(rename)) = state.actions.dialog.as_mut() {
                rename.name = name;
            }
            quiet(Task::none())
        }
        Message::ConfirmDialog => confirm_dialog(state),
        Message::MergeAbort => start(state, Job::AbortIntegration),
        Message::MergeContinue => start(state, Job::ContinueIntegration),
        Message::SelectConflict(index) => {
            if select_conflict(state, index) {
                return quiet(load::actions(state));
            }
            quiet(Task::none())
        }
        Message::TakeBlock(block, side) => {
            let rebasing = state.actions.rebasing;
            let selected = state
                .actions
                .merge
                .as_ref()
                .and_then(|merge| merge.conflicts.get(merge.selected));
            match selected {
                Some(conflict) if !conflict.resolved => {
                    let job = Job::ResolveBlock {
                        path: conflict.path.clone(),
                        block,
                        choice: choice_of(side, rebasing),
                    };
                    start(state, job)
                }
                _ => quiet(Task::none()),
            }
        }
        Message::ConflictPressed(index, action) => {
            state.actions.menu = None;
            select_conflict(state, index);
            conflict_action(state, action)
        }
        Message::ConflictShortcut(action) => conflict_action(state, action),
        Message::DeleteBranchAnyway => match &state.actions.notice {
            Some(Notice::BranchNotMerged { branch }) => {
                let job = Job::DeleteBranch {
                    name: branch.clone(),
                    force: true,
                };
                start(state, job)
            }
            _ => quiet(Task::none()),
        },
        Message::Retry => match state.actions.retry.take() {
            Some(job) => start(state, job),
            None => quiet(Task::none()),
        },
        Message::ToggleNoticeDetails => {
            if let Some(
                Notice::AuthFailed { details_open, .. }
                | Notice::NetworkFailed { details_open, .. }
                | Notice::Other { details_open, .. },
            ) = state.actions.notice.as_mut()
            {
                *details_open = !*details_open;
            }
            quiet(Task::none())
        }
        Message::DismissNotice => {
            state.actions.notice = None;
            state.actions.retry = None;
            quiet(Task::none())
        }
        _ => quiet(Task::none()),
    }
}

/// Starts `job` unless another one runs or there are no actions.
pub fn start(state: &mut State, job: Job) -> Outcome {
    if state.actions.busy.is_some() || state.actions.unavailable.is_some() {
        return quiet(Task::none());
    }
    let Some(root) = state.root.clone() else {
        return quiet(Task::none());
    };
    state.actions.busy = Some(job.operation());
    state.actions.notice = None;
    quiet(load::job(root, job))
}

fn files_of(local: &LocalChanges, side: ChangeSide) -> &[ChangedFile] {
    match side {
        ChangeSide::Unstaged => &local.unstaged,
        ChangeSide::Staged => &local.staged,
    }
}

fn file_job(
    state: &mut State,
    side: ChangeSide,
    index: usize,
    make: impl FnOnce(String) -> Job,
) -> Outcome {
    let path = state
        .actions
        .local
        .as_ref()
        .and_then(|local| files_of(local, side).get(index))
        .map(|file| file.path.clone());
    match path {
        Some(found) => start(state, make(found)),
        None => quiet(Task::none()),
    }
}

fn start_any(state: &mut State, job: Option<Job>) -> Outcome {
    match job {
        Some(found) => start(state, found),
        None => quiet(Task::none()),
    }
}

/// The path and hunk of the chosen file, if it is on `side`.
fn hunk_of(state: &State, side: ChangeSide, hunk: usize) -> Option<(String, DiffHunk)> {
    let local = state.actions.local.as_ref()?;
    let (chosen, index) = local.selected?;
    // The hunks of a diff that ignores whitespace are not the ones git applies.
    if chosen != side || local.ignore_whitespace {
        return None;
    }
    let path = files_of(local, side).get(index)?.path.clone();
    Some((path, local.diff.as_ref()?.hunks.get(hunk)?.clone()))
}

fn hunk_job(
    state: &mut State,
    side: ChangeSide,
    number: usize,
    make: impl FnOnce(String, DiffHunk) -> Job,
) -> Outcome {
    match hunk_of(state, side, number) {
        Some((path, hunk)) => start(state, make(path, hunk)),
        None => quiet(Task::none()),
    }
}

fn select_change(state: &mut State, side: ChangeSide, index: usize) -> Outcome {
    let Some(local) = state.actions.local.as_mut() else {
        return quiet(Task::none());
    };
    if files_of(local, side).get(index).is_none() {
        return quiet(Task::none());
    }
    local.selected = Some((side, index));
    local.diff = None;
    quiet(load::actions(state))
}

/// Commits the staged files; `push` or the box "push afterwards" pushes next.
pub fn commit(state: &mut State, push: bool) -> Outcome {
    let Some(local) = state.actions.local.as_ref() else {
        return quiet(Task::none());
    };
    if local.title.trim().is_empty() && !local.amend {
        state.actions.notice = Some(Notice::CommitTitleMissing);
        return quiet(Task::none());
    }
    let message = CommitMessage {
        title: local.title.clone(),
        body: local.body.text(),
    };
    let amend = local.amend;
    let job = if push || local.push_after {
        Job::CommitAndPush { message, amend }
    } else {
        Job::Commit { message, amend }
    };
    start(state, job)
}

pub fn current_branch(state: &State) -> String {
    state
        .overview
        .as_ref()
        .and_then(|overview| overview.local.iter().find(|branch| branch.is_current))
        .map(|branch| branch.name.clone())
        .unwrap_or_default()
}

/// The row a menu action is about: the open menu's, else the one under the cursor.
fn menu_subject(state: &State) -> Option<MenuTarget> {
    state.actions.menu.clone().map(|open| open.target).or_else(|| {
        state
            .sidebar_items()
            .get(state.sidebar_cursor)
            .and_then(|item| menu_target_of(state, *item))
    })
}

fn branch_of(state: &State, target: &MenuTarget) -> Option<String> {
    let overview = state.overview.as_ref()?;
    match target {
        MenuTarget::LocalBranch(index) => {
            overview.local.get(*index).map(|branch| branch.name.clone())
        }
        MenuTarget::RemoteBranch(index) => overview.remote.get(*index).cloned(),
        MenuTarget::Stash(_) | MenuTarget::Conflict(_) | MenuTarget::Commit(_) => None,
    }
}

/// Asks for the name of a new branch that starts at `from`.
pub fn begin_new_branch(state: &mut State, from: String) -> Outcome {
    if state.actions.unavailable.is_some() {
        return quiet(Task::none());
    }
    state.actions.new_branch = Some(NewBranch {
        name: String::new(),
        from,
        checkout: true,
        problem: None,
    });
    quiet(operation::focus(NAME_ID))
}

fn menu_pressed(state: &mut State, action: MenuAction) -> Outcome {
    if let Some(on_commit) = crate::commit_menu::of_shortcut(state, action) {
        return crate::commit_menu::act(state, on_commit);
    }
    let target = menu_subject(state);
    state.actions.menu = None;
    if action == MenuAction::NewBranchFrom {
        let from = target
            .and_then(|found| branch_of(state, &found))
            .unwrap_or_else(|| current_branch(state));
        return begin_new_branch(state, from);
    }
    let Some(chosen) = target else {
        return quiet(Task::none());
    };
    let branch = branch_of(state, &chosen);
    let stash = match &chosen {
        MenuTarget::Stash(id) => state
            .actions
            .stash_ids
            .contains(id)
            .then(|| id.clone()),
        _ => None,
    };
    // The one key for "delete" means what the row it is pressed on can lose.
    let target_action = match (action, &chosen) {
        (MenuAction::Delete, MenuTarget::RemoteBranch(_)) => MenuAction::DeleteOnRemote,
        (MenuAction::Delete, MenuTarget::Stash(_)) => MenuAction::StashDrop,
        (other, _) => other,
    };
    let question = match (target_action, &chosen) {
        (MenuAction::Delete, MenuTarget::LocalBranch(_)) => {
            branch.clone().map(|name| Question::DeleteBranch { name })
        }
        (MenuAction::DeleteOnRemote, MenuTarget::RemoteBranch(_)) => {
            branch.as_deref().and_then(|full| {
                let (remote_name, branch_name) = full.split_once('/')?;
                Some(Question::DeleteOnRemote {
                    remote: remote_name.to_owned(),
                    branch: branch_name.to_owned(),
                })
            })
        }
        (MenuAction::StashDrop, MenuTarget::Stash(id)) => {
            stash.clone().map(|sha| Question::DropStash {
                sha,
                message: state
                    .actions
                    .stash_ids
                    .iter()
                    .position(|known| known == id)
                    .and_then(|index| state.actions.stashes.get(index))
                    .map(|entry| entry.message.clone())
                    .unwrap_or_default(),
            })
        }
        _ => None,
    };
    if question.is_some() {
        return ask(state, question);
    }
    if target_action == MenuAction::Rename {
        if let (MenuTarget::LocalBranch(_), Some(from)) = (&chosen, branch.clone()) {
            state.actions.dialog = Some(Dialog::Rename(RenameBranch {
                name: from.clone(),
                from,
            }));
            return quiet(operation::focus(RENAME_ID).chain(operation::select_all(RENAME_ID)));
        }
        return quiet(Task::none());
    }
    let job = match (target_action, &chosen) {
        (MenuAction::Checkout, MenuTarget::LocalBranch(_)) => branch.map(Job::Checkout),
        (MenuAction::CheckoutAsLocal, MenuTarget::RemoteBranch(_)) => {
            branch.map(Job::CheckoutRemote)
        }
        (MenuAction::Push, MenuTarget::LocalBranch(_)) => branch.map(Job::PushBranch),
        (MenuAction::MergeInto, MenuTarget::LocalBranch(_) | MenuTarget::RemoteBranch(_)) => {
            branch.map(Job::Merge)
        }
        (MenuAction::RebaseOnto, MenuTarget::LocalBranch(_) | MenuTarget::RemoteBranch(_)) => {
            branch.map(Job::Rebase)
        }
        (MenuAction::StashApply, MenuTarget::Stash(_)) => stash.map(Job::StashApply),
        (MenuAction::StashApplyAndDrop, MenuTarget::Stash(_)) => stash.map(Job::StashPop),
        _ => None,
    };
    start_any(state, job)
}

/// Opens the question unless another call to git runs or there are no actions.
pub fn ask(state: &mut State, question: Option<Question>) -> Outcome {
    if state.actions.busy.is_none() && state.actions.unavailable.is_none() {
        if let Some(asked) = question {
            state.actions.dialog = Some(Dialog::Ask(asked));
        }
    }
    quiet(Task::none())
}

/// The answer of a dialog: a question runs its job, a new name renames.
fn confirm_dialog(state: &mut State) -> Outcome {
    if state.actions.busy.is_some() {
        return quiet(Task::none());
    }
    let job = match state.actions.dialog.take() {
        Some(Dialog::Ask(question)) => question.job(),
        Some(Dialog::Rename(rename)) => {
            let to = rename.name.trim().to_owned();
            if to.is_empty() || to == rename.from {
                return quiet(Task::none());
            }
            Job::RenameBranch {
                from: rename.from,
                to,
            }
        }
        Some(Dialog::Tag(_)) | None => return quiet(Task::none()),
    };
    start(state, job)
}

fn create_branch(state: &mut State) -> Outcome {
    let Some(draft) = state.actions.new_branch.as_ref() else {
        return quiet(Task::none());
    };
    let name = draft.name.trim().to_owned();
    if name.is_empty() || draft.problem.is_some() {
        return quiet(Task::none());
    }
    let job = Job::CreateBranch {
        name,
        from: draft.from.clone(),
        checkout: draft.checkout,
    };
    start(state, job)
}

/// Git's "ours" is the branch rebased onto, so in a rebase "mine" is "theirs".
fn choice_of(side: Side, rebasing: bool) -> ConflictChoice {
    match (side, rebasing) {
        (Side::Mine, false) | (Side::Theirs, true) => ConflictChoice::Ours,
        (Side::Theirs, false) | (Side::Mine, true) => ConflictChoice::Theirs,
        (Side::Both, false) => ConflictChoice::Both,
        (Side::Both, true) => ConflictChoice::TheirsThenOurs,
    }
}

/// Chooses a file in conflict; whether it was a new choice.
fn select_conflict(state: &mut State, index: usize) -> bool {
    match state.actions.merge.as_mut() {
        Some(merge) if index < merge.conflicts.len() && merge.selected != index => {
            merge.selected = index;
            true
        }
        _ => false,
    }
}

fn conflict_action(state: &mut State, action: ConflictAction) -> Outcome {
    let rebasing = state.actions.rebasing;
    let Some(conflict) = state
        .actions
        .merge
        .as_ref()
        .and_then(|merge| merge.conflicts.get(merge.selected))
    else {
        return quiet(Task::none());
    };
    let path = conflict.path.clone();
    if action == ConflictAction::OpenEditor {
        let file = state.root.as_ref().map(|root| root.join(&path));
        return (Task::none(), file.map(Action::OpenInEditor));
    }
    let job = match action {
        ConflictAction::Take(side) if !conflict.resolved => Some(Job::ResolveFile {
            path,
            choice: choice_of(side, rebasing),
        }),
        ConflictAction::MarkResolved if !conflict.resolved => Some(Job::MarkResolved(path)),
        ConflictAction::Reset if conflict.resolved => Some(Job::ResetFile(path)),
        ConflictAction::Take(_)
        | ConflictAction::MarkResolved
        | ConflictAction::Reset
        | ConflictAction::OpenEditor => None,
    };
    start_any(state, job)
}

fn loaded(
    state: &mut State,
    ctx: &Ctx,
    generation: u64,
    result: Result<Snapshot, ActionError>,
) -> Outcome {
    if !state.loads.actions.is_current(generation) {
        return quiet(Task::none());
    }
    match result {
        Ok(snapshot) => apply(state, ctx, snapshot),
        Err(ActionError::GitNotFound) => switch_off(state, Unavailable::GitMissing),
        Err(ActionError::DisabledInSandbox) => switch_off(state, Unavailable::Sandbox),
        Err(error) => return failed(state, ctx, &error.to_string()),
    }
    quiet(ensure_detail(state))
}

fn switch_off(state: &mut State, reason: Unavailable) {
    let actions = &mut state.actions;
    actions.unavailable = Some(reason);
    actions.local = None;
    actions.sync = None;
    actions.merge = None;
    actions.notice = None;
    actions.new_branch = None;
    actions.dialog = None;
    actions.menu = None;
}

fn apply(state: &mut State, ctx: &Ctx, snapshot: Snapshot) {
    let actions = &mut state.actions;
    actions.unavailable = None;
    actions
        .local
        .get_or_insert_with(LocalChanges::default)
        .take(&snapshot);
    actions.stashes = snapshot.stashes.iter().map(snapshot::stash_row).collect();
    actions.stash_ids = snapshot
        .stashes
        .iter()
        .map(|stash| stash.sha.clone())
        .collect();
    apply_integration(actions, snapshot.integration);
    actions.sync.get_or_insert_with(SyncBar::default);
    refresh_sync(state, ctx);
    state.sidebar_cursor = state
        .sidebar_cursor
        .min(state.sidebar_items().len().saturating_sub(1));
}

fn apply_integration(actions: &mut Actions, found: Option<IntegrationSnapshot>) {
    let Some(integration) = found else {
        actions.merge = None;
        actions.rebasing = false;
        actions.seen_conflicts.clear();
        return;
    };
    for open in &integration.conflicts {
        if !actions.seen_conflicts.contains(&open.path) {
            actions.seen_conflicts.push(open.path.clone());
        }
    }
    let conflicts: Vec<Conflict> = actions
        .seen_conflicts
        .iter()
        .map(
            |path| match integration.conflicts.iter().find(|open| &open.path == path) {
                Some(open) => Conflict {
                    path: path.clone(),
                    places: open.places,
                    edited: open.edited,
                    resolved: false,
                },
                None => Conflict {
                    path: path.clone(),
                    places: 0,
                    edited: false,
                    resolved: true,
                },
            },
        )
        .collect();
    let before = actions.merge.as_ref().map_or(0, |merge| merge.selected);
    let shown = integration
        .preview
        .as_ref()
        .and_then(|(path, _)| actions.seen_conflicts.iter().position(|seen| seen == path));
    let selected = shown.unwrap_or_else(|| before.min(conflicts.len().saturating_sub(1)));
    let rebasing = snapshot::is_rebase(&integration.detail);
    let preview = integration
        .preview
        .map(|(_, lines)| snapshot::view_lines(lines, rebasing))
        .unwrap_or_default();
    actions.rebasing = rebasing;
    actions.merge = Some(MergeInProgress {
        kind: integration.detail.kind,
        from: integration.detail.other,
        into: integration.detail.current,
        conflicts,
        selected,
        preview,
    });
}

/// Puts ahead, behind and the undo into the toolbar, once there is one.
pub fn refresh_sync(state: &mut State, ctx: &Ctx) {
    let (ahead, behind) = state
        .overview
        .as_ref()
        .and_then(|overview| overview.local.iter().find(|branch| branch.is_current))
        .map_or((0, 0), |branch| (branch.ahead, branch.behind));
    let undo = state
        .actions
        .undo
        .as_ref()
        .map(|record| undo_text(ctx.lang, &record.summary));
    if let Some(sync) = state.actions.sync.as_mut() {
        *sync = SyncBar {
            ahead,
            behind,
            undo,
        };
    }
}

/// What the undo takes back, for the tooltip.
fn undo_text(lang: Language, summary: &UndoSummary) -> String {
    let counted =
        |msg: Msg, files: usize| fill_template(lang.text(msg), &[("{count}", &files.to_string())]);
    match summary {
        UndoSummary::Stage { files: 0 } => lang.text(Msg::GitViewUndoStagedAll).to_owned(),
        UndoSummary::Stage { files: 1 } => lang.text(Msg::GitViewUndoStagedOne).to_owned(),
        UndoSummary::Stage { files } => counted(Msg::GitViewUndoStagedMany, *files),
        UndoSummary::Unstage { files: 0 } => lang.text(Msg::GitViewUndoUnstagedAll).to_owned(),
        UndoSummary::Unstage { files: 1 } => lang.text(Msg::GitViewUndoUnstagedOne).to_owned(),
        UndoSummary::Unstage { files } => counted(Msg::GitViewUndoUnstagedMany, *files),
        UndoSummary::Discard { files: 1 } => lang.text(Msg::GitViewUndoDiscardedOne).to_owned(),
        UndoSummary::Discard { files } => counted(Msg::GitViewUndoDiscardedMany, *files),
        UndoSummary::Commit { title } => fill_template(
            lang.text(Msg::GitViewUndoCommitted),
            &[("{title}", title.trim())],
        ),
        UndoSummary::DeleteBranch { name } => fill_template(
            lang.text(Msg::GitViewUndoDeletedBranch),
            &[("{name}", name)],
        ),
        UndoSummary::Ignore { pattern } => fill_template(
            lang.text(Msg::GitViewUndoIgnored),
            &[("{pattern}", pattern)],
        ),
        UndoSummary::CreateTag { name } => {
            fill_template(lang.text(Msg::GitViewUndoCreatedTag), &[("{name}", name)])
        }
        UndoSummary::CherryPick { title } => fill_template(
            lang.text(Msg::GitViewUndoCherryPicked),
            &[("{title}", title.trim())],
        ),
        UndoSummary::Revert { title } => fill_template(
            lang.text(Msg::GitViewUndoReverted),
            &[("{title}", title.trim())],
        ),
        UndoSummary::Reset { branch, sha } => fill_template(
            lang.text(Msg::GitViewUndoReset),
            &[("{branch}", branch), ("{sha}", sha)],
        ),
        UndoSummary::CleanUp { branches } => fill_template(
            lang.text(Msg::GitViewUndoCleanedUp),
            &[("{count}", &branches.to_string())],
        ),
    }
}

fn finished_job(state: &mut State, ctx: &Ctx, finished: Finished) -> Outcome {
    state.actions.busy = None;
    let Finished { job, result } = finished;
    match result {
        Ok(Done::CommittedButNotPushed { undo, error }) => {
            if undo.is_some() {
                state.actions.undo = undo;
            }
            after_success(state, &job);
            report(state, ctx, Job::Push, error);
        }
        Ok(done) => {
            match done {
                Done::Undoable(Some(record)) => state.actions.undo = Some(record),
                Done::Undone => state.actions.undo = None,
                Done::Undoable(None) | Done::Plain | Done::CommittedButNotPushed { .. } => {}
            }
            state.actions.retry = None;
            after_success(state, &job);
        }
        Err(error) => report(state, ctx, job, error),
    }
    refresh_sync(state, ctx);
    quiet(load::everything(state))
}

fn after_success(state: &mut State, job: &Job) {
    match job {
        Job::Commit { .. } | Job::CommitAndPush { .. } => {
            if let Some(local) = state.actions.local.as_mut() {
                local.title.clear();
                local.body = text_editor::Content::new();
                local.amend = false;
            }
        }
        Job::CreateBranch { .. } => state.actions.new_branch = None,
        Job::CreateTag { .. } => state.actions.dialog = None,
        _ => {}
    }
}

pub(crate) fn report(state: &mut State, ctx: &Ctx, job: Job, error: ActionError) {
    match &error {
        ActionError::GitNotFound => return switch_off(state, Unavailable::GitMissing),
        ActionError::DisabledInSandbox => return switch_off(state, Unavailable::Sandbox),
        ActionError::InvalidName { name } => {
            if let Some(draft) = state.actions.new_branch.as_mut() {
                draft.problem = Some(fill_template(
                    ctx.lang.text(Msg::GitViewInvalidNameText),
                    &[("{name}", name)],
                ));
                return;
            }
        }
        _ => {}
    }
    if matches!(job, Job::CreateBranch { .. }) {
        state.actions.new_branch = None;
    }
    if matches!(job, Job::CreateTag { .. }) {
        state.actions.dialog = None;
    }
    if matches!(
        error,
        ActionError::AuthFailed { .. } | ActionError::NetworkFailed { .. }
    ) {
        state.actions.retry = Some(job);
    }
    state.actions.notice = Some(notice_of(state, error));
}

fn notice_of(state: &State, error: ActionError) -> Notice {
    match error {
        ActionError::Rejected { behind } => Notice::PushRejected {
            branch: current_branch(state),
            behind,
        },
        ActionError::AuthFailed { remote, git_output } => Notice::AuthFailed {
            remote,
            output: git_output,
            details_open: false,
        },
        ActionError::NetworkFailed { git_output } => Notice::NetworkFailed {
            output: git_output,
            details_open: false,
        },
        ActionError::BranchNotMerged { branch } => Notice::BranchNotMerged { branch },
        ActionError::NothingToCommit => Notice::NothingToCommit,
        ActionError::NeedsPullStrategy { .. } => Notice::NeedsPullStrategy,
        ActionError::UnresolvedConflicts { count } => Notice::UnresolvedConflicts { count },
        ActionError::UndoNotSafe(_) => Notice::UndoNotSafe,
        ActionError::InvalidName { name } => Notice::InvalidName { name },
        ActionError::Other { git_output } | ActionError::NotFastForward { git_output } => {
            Notice::Other {
                output: git_output,
                details_open: false,
            }
        }
        ActionError::LeaseRejected
        | ActionError::GitNotFound
        | ActionError::DisabledInSandbox
        | ActionError::Io(_)
        | ActionError::Read(_) => Notice::Other {
            output: error.to_string(),
            details_open: false,
        },
    }
}
