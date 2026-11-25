//! Moving around the tab without changing the repository: the pickers, the
//! splitters, the jumps to HEAD, the working copy and a parent, and copying a SHA.

use iced::widget::operation;
use iced::widget::scrollable::AbsoluteOffset;
use iced::Task;
use ncrs_ui_kit::theme::size;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::handlers;
use crate::message::{Chip, Message};
use crate::panes::PaneDrag;
use crate::picker;
use crate::state::{Area, State};
use crate::update::{ensure_detail, quiet, Outcome};
use crate::view::COMMITS_ID;

pub fn handle(state: &mut State, message: Message, ctx: &Ctx) -> Outcome {
    match message {
        Message::OpenPicker(kind) => {
            let open_already = state.picker.as_ref().is_some_and(|open| open.kind == kind);
            if open_already {
                quiet(picker::close(state))
            } else {
                quiet(picker::open(state, kind, ctx))
            }
        }
        Message::PickerQuery(query) => {
            picker::set_query(state, query);
            quiet(Task::none())
        }
        Message::PickerPressed(id) => picker::press(state, id, false, ctx),
        Message::PickerSubmit => picker::enter(state, false, ctx),
        Message::PickerEnterWithStash => picker::enter(state, true, ctx),
        Message::PickerNewBranch => {
            let closed = picker::close(state);
            let from = handlers::current_branch(state);
            let (asked, action) = handlers::begin_new_branch(state, from);
            (Task::batch([closed, asked]), action)
        }
        Message::SplitPressed(split) => {
            state.area = if split == crate::panes::Split::Sidebar {
                Area::SidebarSplit
            } else {
                Area::DetailSplit
            };
            state.pane_drag = Some(PaneDrag::begin(split, ctx.panes));
            quiet(Task::none())
        }
        Message::SplitHover(split) => {
            state.hovered_split = Some(split);
            quiet(Task::none())
        }
        Message::SplitLeave => {
            state.hovered_split = None;
            quiet(Task::none())
        }
        Message::SplitReset(split) => resized(ctx, ctx.panes.reset(split)),
        Message::SplitNudge(split, step, direction) => {
            resized(ctx, ctx.panes.nudged(split, step, direction))
        }
        Message::PaneMoved(x) => {
            if let Some(drag) = state.pane_drag.as_mut() {
                drag.moved(x);
            }
            quiet(Task::none())
        }
        Message::PaneReleased => {
            let finished = state.pane_drag.take();
            match finished {
                Some(drag) => resized(ctx, drag.current),
                None => quiet(Task::none()),
            }
        }
        Message::JumpToHead => quiet(jump_to_head(state)),
        Message::JumpToWip => quiet(jump_to_working_copy(state)),
        Message::JumpToCommit(sha) => quiet(jump_to(state, sha)),
        Message::GoToParent => {
            let parent = state
                .detail
                .as_ref()
                .and_then(|detail| detail.info.as_ref())
                .and_then(|info| info.parents.first().cloned());
            match parent {
                Some(sha) => quiet(jump_to(state, sha)),
                None => quiet(Task::none()),
            }
        }
        Message::CopySha => {
            let copied = state
                .detail
                .as_ref()
                .and_then(|detail| detail.info.as_ref())
                .map(|info| (info.id.clone(), info.short.clone()));
            match copied {
                Some((id, short)) => {
                    state.copied_sha = Some(short);
                    quiet(iced::clipboard::write(id))
                }
                None => quiet(Task::none()),
            }
        }
        Message::ToggleTreeKey => {
            if state.area == Area::Detail {
                state.tree_mode = !state.tree_mode;
            }
            quiet(Task::none())
        }
        Message::ChipPressed(chip) => match chip {
            Chip::Behind | Chip::Ahead => quiet(jump_to_head(state)),
            Chip::Changes => quiet(jump_to_working_copy(state)),
            Chip::Conflicts => {
                state.area = Area::Detail;
                quiet(Task::none())
            }
        },
        Message::ContainingLoaded(generation, sha, result) => {
            if state.loads.containing.is_current(generation) {
                state.containing = (*result).ok().map(|labels| (sha, labels));
            }
            quiet(Task::none())
        }
        _ => quiet(Task::none()),
    }
}

fn jump_to_head(state: &mut State) -> Task<Message> {
    let head = state.log.as_ref().and_then(|log| log.head.clone());
    head.map_or_else(Task::none, |sha| jump_to(state, sha))
}

fn resized(ctx: &Ctx, widths: crate::panes::PaneWidths) -> Outcome {
    (
        Task::none(),
        (widths != ctx.panes).then_some(Action::PanesResized(widths)),
    )
}

/// Puts the cursor of the commit list on `sha`. A commit the list has not
/// reached yet is waited for: the list reads on and `LogLoaded` tries again.
pub fn jump_to(state: &mut State, sha: String) -> Task<Message> {
    state.pending_jump = None;
    let found = state
        .log
        .as_ref()
        .and_then(|log| log.commits.iter().position(|commit| commit.id == sha));
    let Some(index) = found else {
        if state.log.as_ref().is_some_and(|log| log.has_more) {
            state.pending_jump = Some(sha);
            return crate::update::load_more(state);
        }
        return Task::none();
    };
    state.filter_focused = false;
    state.area = Area::Commits;
    state.commit_cursor = index + usize::from(state.has_working_row());
    let detail = ensure_detail(state);
    Task::batch([detail, scroll_to_cursor(state)])
}

fn jump_to_working_copy(state: &mut State) -> Task<Message> {
    if state.has_working_row() {
        state.area = Area::Commits;
        state.commit_cursor = 0;
        return Task::batch([ensure_detail(state), scroll_to_cursor(state)]);
    }
    Task::none()
}

fn scroll_to_cursor(state: &State) -> Task<Message> {
    operation::scroll_to(
        COMMITS_ID,
        AbsoluteOffset {
            x: None,
            y: Some(state.commit_cursor.saturating_sub(3) as f32 * size::ROW),
        },
    )
}
