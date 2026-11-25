//! The menu of a commit in the list (#369): which entries it has, which of
//! them work, what the keys do in it, and what each entry starts.

use iced::widget::operation;
use iced::Task;
use ncrs_git::ResetMode;
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::popup_menu::{submenu_step, SubmenuArrow, SubmenuStep};

use crate::actions::{Dialog, MenuAction, MenuTarget, Question, TagDraft};
use crate::ctx::Ctx;
use crate::handlers;
use crate::job::Job;
use crate::load;
use crate::message::Message;
use crate::state::{Area, DiffState, State, Subject};
use crate::update::{failed, quiet, reload_diff, Outcome};
use crate::view::TAG_ID;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitAction {
    CopySha,
    Checkout,
    NewBranch,
    Tag,
    CherryPick,
    Revert,
    Reset(ResetMode),
    CompareWorking,
    CompareMarked,
}

/// The entries of the menu in order; `None` is the reset submenu.
pub const ENTRIES: [Option<CommitAction>; 9] = [
    Some(CommitAction::CopySha),
    Some(CommitAction::Checkout),
    Some(CommitAction::NewBranch),
    Some(CommitAction::Tag),
    Some(CommitAction::CherryPick),
    Some(CommitAction::Revert),
    None,
    Some(CommitAction::CompareWorking),
    Some(CommitAction::CompareMarked),
];

/// The place of the reset submenu among [`ENTRIES`].
pub const RESET_ENTRY: usize = 6;

pub const RESET_MODES: [ResetMode; 3] = [ResetMode::Soft, ResetMode::Mixed, ResetMode::Hard];

/// Whether git may be written to right now.
fn writable(state: &State) -> bool {
    let actions = &state.actions;
    actions.unavailable.is_none() && actions.busy.is_none() && actions.merge.is_none()
}

/// Whether the entry works on the commit of `row`. Copying the SHA and
/// comparing only read, so they work in the sandbox too.
pub fn enabled(state: &State, row: usize, action: CommitAction) -> bool {
    let Some(commit) = state.commit_at(row) else {
        return false;
    };
    match action {
        CommitAction::CopySha | CommitAction::CompareWorking => true,
        CommitAction::CompareMarked => state
            .marked_commit
            .as_ref()
            .is_some_and(|marked| *marked != commit.id),
        CommitAction::CherryPick | CommitAction::Revert => writable(state) && !commit.is_merge,
        CommitAction::Checkout
        | CommitAction::NewBranch
        | CommitAction::Tag
        | CommitAction::Reset(_) => writable(state),
    }
}

/// The branch HEAD is on, or the word for a detached HEAD: never empty.
pub fn branch_label(state: &State, lang: Language) -> String {
    let name = handlers::current_branch(state);
    if name.is_empty() {
        lang.text(Msg::GitViewDetachedHead).to_owned()
    } else {
        name
    }
}

/// The commit menu is open.
pub fn is_open(state: &State) -> bool {
    state
        .actions
        .menu
        .as_ref()
        .is_some_and(|open| matches!(open.target, MenuTarget::Commit(_)))
}

/// The row of the open commit menu, else the commit under the cursor of the list.
fn target_row(state: &State) -> Option<usize> {
    match &state.actions.menu {
        Some(open) => match &open.target {
            MenuTarget::Commit(sha) => state.row_of_commit(sha),
            _ => None,
        },
        None => (state.area == Area::Commits && !state.list_is_covered())
            .then_some(state.commit_cursor),
    }
}

/// The key for "check out" or "new branch" in the commit list means the commit under the cursor.
pub fn of_shortcut(state: &State, action: MenuAction) -> Option<CommitAction> {
    let on_commit = state.actions.menu.is_none()
        && state.area == Area::Commits
        && !state.list_is_covered()
        && state.commit_at(state.commit_cursor).is_some();
    match action {
        MenuAction::Checkout if on_commit => Some(CommitAction::Checkout),
        MenuAction::NewBranchFrom if on_commit => Some(CommitAction::NewBranch),
        _ => None,
    }
}

pub fn handle(state: &mut State, message: Message, ctx: &Ctx) -> Outcome {
    match message {
        Message::CommitAction(action) => act(state, action),
        Message::MenuCursor(entry) => {
            if let Some(open) = state.actions.menu.as_mut() {
                open.cursor = entry;
                open.submenu = (entry == RESET_ENTRY).then_some(0);
            }
            quiet(Task::none())
        }
        Message::MenuSubmenuCursor(entry) => {
            if let Some(open) = state.actions.menu.as_mut() {
                open.submenu = Some(entry);
            }
            quiet(Task::none())
        }
        Message::MarkCommit => {
            let id = state
                .commit_at(state.commit_cursor)
                .map(|commit| commit.id.clone());
            if id.is_some() {
                state.marked_commit = if state.marked_commit == id { None } else { id };
            }
            quiet(Task::none())
        }
        Message::CommitsScrolled(offset) => {
            state.commits_scroll = offset;
            quiet(Task::none())
        }
        Message::TagNameChanged(name) => {
            if let Some(Dialog::Tag(draft)) = state.actions.dialog.as_mut() {
                draft.problem = name
                    .contains(char::is_whitespace)
                    .then(|| ctx.lang.text(Msg::GitViewNewBranchSpaces).to_owned());
                draft.name = name;
            }
            quiet(Task::none())
        }
        Message::CreateTag => {
            let Some(Dialog::Tag(draft)) = &state.actions.dialog else {
                return quiet(Task::none());
            };
            let name = draft.name.trim().to_owned();
            if name.is_empty() || draft.problem.is_some() {
                return quiet(Task::none());
            }
            let job = Job::CreateTag {
                name,
                sha: draft.sha.clone(),
            };
            handlers::start(state, job)
        }
        Message::ResetLossLoaded(sha, loss) => match *loss {
            Ok(lost) => {
                let branch = branch_label(state, ctx.lang);
                handlers::ask(
                    state,
                    Some(Question::ResetHard {
                        branch,
                        sha,
                        commits: lost.commits,
                        changed_files: lost.changed_files + lost.overwritten_untracked,
                    }),
                )
            }
            Err(error) => {
                let job = Job::Reset {
                    mode: ResetMode::Hard,
                    sha,
                };
                handlers::report(state, ctx, job, error);
                quiet(Task::none())
            }
        },
        Message::CompareLoaded(generation, subject, files) => {
            if !state.loads.diff.is_current(generation) {
                return quiet(Task::none());
            }
            match *files {
                Ok(files) => {
                    state.file_cursor = 0;
                    state.diff = Some(DiffState {
                        subject,
                        files,
                        file: 0,
                        diff: None,
                        unified: false,
                        ignore_whitespace: false,
                        hunk: 0,
                    });
                    quiet(reload_diff(state))
                }
                Err(reason) => failed(state, ctx, &reason),
            }
        }
        _ => quiet(Task::none()),
    }
}

/// An arrow or Enter while the commit menu is open.
pub fn key(state: &mut State, message: &Message) -> Outcome {
    state.close_menu_if_stale();
    let Some(mut open) = state.actions.menu.clone() else {
        return quiet(Task::none());
    };
    let last = |len: usize| len.saturating_sub(1);
    match message {
        Message::Up | Message::Down => {
            let down = *message == Message::Down;
            let (place, len) = match open.submenu {
                Some(inside) => (inside, RESET_MODES.len()),
                None => (open.cursor, ENTRIES.len()),
            };
            let moved = if down {
                (place + 1).min(last(len))
            } else {
                place.saturating_sub(1)
            };
            match open.submenu {
                Some(_) => open.submenu = Some(moved),
                None => open.cursor = moved,
            }
        }
        Message::Right | Message::Left => {
            let arrow = if *message == Message::Right {
                SubmenuArrow::Right
            } else {
                SubmenuArrow::Left
            };
            match submenu_step(arrow, open.cursor == RESET_ENTRY, open.submenu.is_some()) {
                SubmenuStep::Open => open.submenu = Some(0),
                SubmenuStep::Close => open.submenu = None,
                SubmenuStep::Ignore => {}
            }
        }
        _ => {
            let chosen = match open.submenu {
                Some(inside) => RESET_MODES.get(inside).map(|mode| CommitAction::Reset(*mode)),
                None => match ENTRIES.get(open.cursor).copied().flatten() {
                    Some(action) => Some(action),
                    None => {
                        open.submenu = Some(0);
                        None
                    }
                },
            };
            if let Some(action) = chosen {
                return act(state, action);
            }
        }
    }
    state.actions.menu = Some(open);
    quiet(Task::none())
}

pub fn act(state: &mut State, action: CommitAction) -> Outcome {
    let was_open = state.actions.menu.is_some();
    state.close_menu_if_stale();
    if was_open && state.actions.menu.is_none() {
        return quiet(Task::none());
    }
    let Some(row) = target_row(state) else {
        return quiet(Task::none());
    };
    if !enabled(state, row, action) {
        return quiet(Task::none());
    }
    let Some(commit) = state.commit_at(row) else {
        return quiet(Task::none());
    };
    let (id, short) = (commit.id.clone(), commit.short.clone());
    state.actions.menu = None;
    match action {
        CommitAction::CopySha => {
            state.copied_sha = Some(short);
            quiet(iced::clipboard::write(id))
        }
        CommitAction::Checkout => handlers::start(state, Job::CheckoutDetached(id)),
        CommitAction::NewBranch => handlers::begin_new_branch(state, id),
        CommitAction::Tag => {
            state.actions.dialog = Some(Dialog::Tag(TagDraft {
                name: String::new(),
                sha: id,
                problem: None,
            }));
            quiet(operation::focus(TAG_ID))
        }
        CommitAction::CherryPick => handlers::start(state, Job::CherryPick(id)),
        CommitAction::Revert => handlers::start(state, Job::Revert(id)),
        CommitAction::Reset(ResetMode::Hard) => {
            let Some(root) = state.root.clone() else {
                return quiet(Task::none());
            };
            quiet(load::reset_loss(root, id))
        }
        CommitAction::Reset(mode) => handlers::start(state, Job::Reset { mode, sha: id }),
        CommitAction::CompareWorking => quiet(load::compare_files(state, id, None)),
        CommitAction::CompareMarked => {
            let Some(marked) = state.marked_commit.clone() else {
                return quiet(Task::none());
            };
            quiet(load::compare_files(state, marked, Some(id)))
        }
    }
}

/// The two ends of a comparison as the header names them: short ids.
pub fn compare_ends(subject: &Subject) -> Option<(String, Option<String>)> {
    let short = |sha: &str| sha.chars().take(7).collect::<String>();
    match subject {
        Subject::Compare { from, to } => Some((short(from), to.as_deref().map(short))),
        Subject::Working | Subject::Commit(_) => None,
    }
}
