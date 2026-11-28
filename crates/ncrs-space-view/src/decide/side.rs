//! The narrow overview beside the list: the keys on it and opening another folder to decide on.
//! It needs the rows of the tab, so it works on the whole state, not on the pane alone.

use iced::Task;
use ncrs_space::Group;

use super::{DecideArea, DecideMessage};
use crate::ctx::Ctx;
use crate::rows::RowKind;
use crate::state::State;
use crate::update::Outcome;

/// How far Page Up and Page Down go.
const PAGE_ROWS: isize = 10;

fn nothing() -> Outcome {
    (Task::none(), Vec::new())
}

/// What the overview does with a message; `None` when it is not the overview's.
pub fn update(state: &mut State, message: &DecideMessage, ctx: &Ctx) -> Option<Outcome> {
    let on_side = state.decide.as_ref()?.area == DecideArea::Side;
    match message {
        DecideMessage::SidePressed(path) => Some(press(state, path, ctx)),
        DecideMessage::Up if on_side => Some(step(state, -1)),
        DecideMessage::Down if on_side => Some(step(state, 1)),
        DecideMessage::PageUp if on_side => Some(step(state, -PAGE_ROWS)),
        DecideMessage::PageDown if on_side => Some(step(state, PAGE_ROWS)),
        DecideMessage::First if on_side => Some(step(state, isize::MIN)),
        DecideMessage::Last if on_side => Some(step(state, isize::MAX)),
        DecideMessage::Enter | DecideMessage::Toggle if on_side => {
            let path = state.decide.as_ref()?.side_cursor.clone();
            Some(open_row(state, &path, ctx))
        }
        _ => None,
    }
}

fn step(state: &mut State, by: isize) -> Outcome {
    let Some(cursor) = state.decide.as_ref().map(|pane| pane.side_cursor.clone()) else {
        return nothing();
    };
    let last = state.rows.len().saturating_sub(1);
    let at = state
        .rows
        .iter()
        .position(|row| row.path == cursor)
        .unwrap_or(0);
    let target = at.saturating_add_signed(by).min(last);
    if let (Some(row), Some(pane)) = (state.rows.get(target), state.decide.as_mut()) {
        pane.side_cursor = row.path.clone();
    }
    nothing()
}

fn press(state: &mut State, path: &std::path::Path, ctx: &Ctx) -> Outcome {
    if let Some(pane) = state.decide.as_mut() {
        pane.area = DecideArea::Side;
        pane.side_cursor = path.to_path_buf();
    }
    open_row(state, path, ctx)
}

/// Opens the folder of a row to decide on; any other row, and the folder that is open, stay.
fn open_row(state: &mut State, path: &std::path::Path, ctx: &Ctx) -> Outcome {
    let Some(row) = state.row_by_path(path) else {
        return nothing();
    };
    let shown = state.decide.as_ref().map(|pane| pane.folder.as_path());
    if row.group != Group::Decide || !row.can_trash() || shown == Some(path) {
        return nothing();
    }
    let is_downloads = row.kind == RowKind::Downloads;
    let (mut pane, task) = super::open(path.to_path_buf(), is_downloads, ctx);
    pane.area = DecideArea::List;
    state.decide = Some(pane);
    (task, Vec::new())
}
