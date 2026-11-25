//! The pane of the local changes: the keys that move through it, stage a file,
//! commit and push, and the ignore question.

use iced::widget::operation;
use iced::widget::scrollable::AbsoluteOffset;
use iced::Task;
use ncrs_git::ChangedFile;
use ncrs_ui_kit::focus::release_focus;
use ncrs_ui_kit::theme::size;

use crate::action::Action;
use crate::actions::{ChangeSide, Dialog, IgnoreChoice, LocalChanges, Question, WipFocus};
use crate::diff_layout::rows_before;
use crate::handlers::{ask, commit, start};
use crate::job::Job;
use crate::load;
use crate::message::Message;
use crate::state::{Area, State};
use crate::update::{ensure_detail, quiet, Outcome};
use crate::view::{BODY_ID, HUNKS_ID, TITLE_ID};

fn selected(local: &LocalChanges) -> Option<(ChangeSide, usize, &ChangedFile)> {
    let (side, index) = local.selected?;
    let files = match side {
        ChangeSide::Unstaged => &local.unstaged,
        ChangeSide::Staged => &local.staged,
    };
    files.get(index).map(|file| (side, index, file))
}

pub fn focus_of(side: ChangeSide) -> WipFocus {
    match side {
        ChangeSide::Unstaged => WipFocus::Unstaged,
        ChangeSide::Staged => WipFocus::Staged,
    }
}

fn focus_task(focus: WipFocus) -> Task<Message> {
    match focus {
        WipFocus::Title => operation::focus(TITLE_ID),
        WipFocus::Body => operation::focus(BODY_ID),
        _ => release_focus(),
    }
}

/// The stop of the file list the chosen file is on.
fn file_focus(local: &LocalChanges) -> WipFocus {
    local
        .selected
        .map_or(WipFocus::Unstaged, |(side, _)| focus_of(side))
}

/// Enter or the arrow into the pane: the keys act on the files.
pub fn enter_pane(state: &mut State) {
    state.area = Area::Detail;
    if let Some(local) = state.actions.local.as_mut() {
        local.focus = file_focus(local);
    }
}

/// ⌘L: the commit title, from wherever the user is.
pub fn focus_title(state: &mut State) -> Task<Message> {
    if state.actions.local.is_none() || !state.has_working_row() {
        return Task::none();
    }
    state.commit_cursor = 0;
    state.area = Area::Detail;
    if let Some(local) = state.actions.local.as_mut() {
        local.focus = WipFocus::Title;
    }
    Task::batch([ensure_detail(state), focus_task(WipFocus::Title)])
}

pub fn typed_in(state: &mut State, focus: WipFocus) {
    if let Some(local) = state.actions.local.as_mut() {
        local.focus = focus;
    }
}

pub fn focused(state: &mut State, focus: WipFocus) -> Task<Message> {
    if state.actions.local.is_none() {
        return Task::none();
    }
    state.area = Area::Detail;
    typed_in(state, focus);
    focus_task(focus)
}

/// n and p (and the arrows of the toolbar) through the changes of the open diff.
pub fn step_hunk(state: &mut State, forward: bool) -> Task<Message> {
    let Some(local) = state.actions.local.as_mut() else {
        return Task::none();
    };
    let Some(diff) = local.diff.as_ref().filter(|_| local.diff_open) else {
        return Task::none();
    };
    let last = diff.hunks.len().saturating_sub(1);
    local.hunk = if forward {
        (local.hunk + 1).min(last)
    } else {
        local.hunk.saturating_sub(1)
    };
    let top = rows_before(diff, local.unified, local.hunk) as f32 * size::ROW
        + local.hunk as f32 * (size::ROW_TALL - size::ROW);
    operation::scroll_to(
        HUNKS_ID,
        AbsoluteOffset {
            x: None,
            y: Some(top),
        },
    )
}

/// ↑ and ↓ through the unstaged files, then the staged ones.
pub fn move_selection(state: &mut State, delta: isize) -> Task<Message> {
    let Some(local) = state.actions.local.as_mut() else {
        return Task::none();
    };
    if !matches!(local.focus, WipFocus::Unstaged | WipFocus::Staged) {
        return Task::none();
    }
    let all: Vec<(ChangeSide, usize)> = (0..local.unstaged.len())
        .map(|index| (ChangeSide::Unstaged, index))
        .chain((0..local.staged.len()).map(|index| (ChangeSide::Staged, index)))
        .collect();
    if all.is_empty() {
        return Task::none();
    }
    let at = local
        .selected
        .and_then(|chosen| all.iter().position(|place| *place == chosen))
        .unwrap_or_default();
    let next = at
        .saturating_add_signed(delta)
        .min(all.len().saturating_sub(1));
    let Some(&(side, index)) = all.get(next) else {
        return Task::none();
    };
    local.selected = Some((side, index));
    local.diff = None;
    local.focus = focus_of(side);
    load::actions(state)
}

/// Tab and Shift+Tab: through the stops of the pane, then on to the next area.
pub fn tab(state: &mut State, forward: bool) -> Task<Message> {
    if !(state.area == Area::Detail && state.changes_shown()) {
        state.area = if forward {
            state.area.next()
        } else {
            state.area.previous()
        };
        if state.area == Area::Detail {
            enter_pane(state);
        }
        return Task::none();
    }
    let Some(local) = state.actions.local.as_mut() else {
        return Task::none();
    };
    let at_the_end = if forward {
        local.focus == WipFocus::Commit
    } else {
        local.focus == WipFocus::Unstaged
    };
    if at_the_end {
        state.area = if forward {
            state.area.next()
        } else {
            state.area.previous()
        };
        return release_focus();
    }
    local.focus = if forward {
        local.focus.next()
    } else {
        local.focus.previous()
    };
    let focus = local.focus;
    let wanted = match focus {
        WipFocus::Unstaged => Some((ChangeSide::Unstaged, !local.unstaged.is_empty())),
        WipFocus::Staged => Some((ChangeSide::Staged, !local.staged.is_empty())),
        _ => None,
    };
    let reselect = wanted
        .filter(|(side, has_files)| {
            *has_files && local.selected.map(|(chosen, _)| chosen) != Some(*side)
        })
        .map(|(side, _)| side);
    if let Some(side) = reselect {
        local.selected = Some((side, 0));
        local.diff = None;
        return Task::batch([focus_task(focus), load::actions(state)]);
    }
    focus_task(focus)
}

/// Enter in the pane: the diff of the chosen file, or the press of the stop.
pub fn enter(state: &mut State) -> Outcome {
    let focus = match state.actions.local.as_ref() {
        Some(local) => local.focus,
        None => return quiet(Task::none()),
    };
    match focus {
        WipFocus::Unstaged | WipFocus::Staged => {
            if let Some(local) = state.actions.local.as_mut() {
                local.diff_open = local.selected.is_some();
            }
            quiet(Task::none())
        }
        WipFocus::Amend | WipFocus::PushAfter | WipFocus::Commit => press_stop(state, focus),
        WipFocus::Title | WipFocus::Body => quiet(Task::none()),
    }
}

/// Space: stages or unstages the chosen file, or presses the stop Tab is on.
pub fn toggle_stage_selected(state: &mut State) -> Outcome {
    let Some(local) = state.actions.local.as_ref() else {
        return quiet(Task::none());
    };
    let focus = local.focus;
    match focus {
        WipFocus::Unstaged | WipFocus::Staged => {
            let job = selected(local).map(|(side, _, file)| match side {
                ChangeSide::Unstaged => Job::Stage(vec![file.path.clone()]),
                ChangeSide::Staged => Job::Unstage(vec![file.path.clone()]),
            });
            match job {
                Some(toggle) => start(state, toggle),
                None => quiet(Task::none()),
            }
        }
        WipFocus::Amend | WipFocus::PushAfter | WipFocus::Commit => press_stop(state, focus),
        WipFocus::Title | WipFocus::Body => quiet(Task::none()),
    }
}

fn press_stop(state: &mut State, focus: WipFocus) -> Outcome {
    match focus {
        WipFocus::Amend => toggle_amend(state),
        WipFocus::PushAfter => toggle_push_after(state),
        WipFocus::Commit => return commit(state, false),
        WipFocus::Unstaged | WipFocus::Staged | WipFocus::Title | WipFocus::Body => {}
    }
    quiet(Task::none())
}

pub fn toggle_amend(state: &mut State) {
    if let Some(local) = state.actions.local.as_mut() {
        local.amend = !local.amend;
    }
}

pub fn toggle_push_after(state: &mut State) {
    if let Some(local) = state.actions.local.as_mut() {
        local.push_after = !local.push_after;
    }
}

/// Esc, one step at a time: the field, the diff, the pane. `None`: nothing of
/// the pane is in the way, the tab goes on.
pub fn escape_step(state: &mut State) -> Option<Task<Message>> {
    if !state.changes_shown() {
        return None;
    }
    let local = state.actions.local.as_mut()?;
    if local.focus.is_field() {
        local.focus = file_focus(local);
        return Some(release_focus());
    }
    if local.diff_open {
        local.diff_open = false;
        return Some(Task::none());
    }
    if state.area == Area::Detail {
        state.area = Area::Commits;
        return Some(Task::none());
    }
    None
}

fn ignore_question(file: &ChangedFile) -> Option<Question> {
    if !file.untracked {
        return None;
    }
    let (folder, name) = match file.path.rsplit_once('/') {
        Some((folder, name)) => (Some(folder.to_owned()), name),
        None => (None, file.path.as_str()),
    };
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .filter(|extension| !extension.is_empty() && !name.starts_with('.'))
        .map(str::to_owned);
    Some(Question::Ignore {
        path: file.path.clone(),
        folder,
        extension,
        chosen: IgnoreChoice::File,
    })
}

/// "Ignore…" on a file that git does not know yet; a tracked file is not
/// offered, `git rm --cached` is a decision of its own.
pub fn ignore_file(state: &mut State, index: usize) -> Outcome {
    let question = state
        .actions
        .local
        .as_ref()
        .and_then(|local| local.unstaged.get(index))
        .and_then(ignore_question);
    ask(state, question)
}

pub fn ignore_selected(state: &mut State) -> Outcome {
    let question = state
        .actions
        .local
        .as_ref()
        .and_then(selected)
        .filter(|(side, _, _)| *side == ChangeSide::Unstaged)
        .and_then(|(_, _, file)| ignore_question(file));
    ask(state, question)
}

pub fn choose_ignore(state: &mut State, choice: IgnoreChoice) {
    if let Some(Dialog::Ask(Question::Ignore {
        folder,
        extension,
        chosen,
        ..
    })) = state.actions.dialog.as_mut()
    {
        let offered = match choice {
            IgnoreChoice::File => true,
            IgnoreChoice::Folder => folder.is_some(),
            IgnoreChoice::Extension => extension.is_some(),
        };
        if offered {
            *chosen = choice;
        }
    }
}

pub fn open_selected_in_editor(state: &State) -> Outcome {
    let file = state
        .actions
        .local
        .as_ref()
        .and_then(selected)
        .zip(state.root.as_ref())
        .map(|((_, _, file), root)| root.join(&file.path));
    (Task::none(), file.map(Action::OpenInEditor))
}
