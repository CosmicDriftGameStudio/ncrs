//! The widths of the list columns: what is remembered, and dragging one.

use ncrs_config::repos::{ReposColumnWidths, ReposSettings};
use ncrs_ui_kit::repo_row::{RepoColumn, RepoColumns};

use crate::action::Action;
use crate::state::State;

/// The chips of "3 behind origin/main" and "11 local changes" and the time
/// "yesterday 17:01" need more than [`RepoColumns::STANDARD`] gives them.
const STANDING_WIDTH: f32 = 250.0;
const CHANGES_WIDTH: f32 = 160.0;
const FETCHED_WIDTH: f32 = 122.0;

/// A column being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnDrag {
    pub column: RepoColumn,
    /// The widths when the press began.
    pub start: RepoColumns,
    /// Where the pointer was at the first move after the press.
    pub origin: Option<f32>,
    /// The widths as dragged so far.
    pub current: RepoColumns,
}

/// The widths the list has when nothing special (clean-up pane, run) changes
/// them: what the user dragged, the standard for the rest.
pub fn remembered(settings: &ReposSettings) -> RepoColumns {
    let widths = settings.columns;
    let or = |wanted: Option<u16>, standard: f32| wanted.map_or(standard, f32::from);
    RepoColumns {
        name: or(widths.name, RepoColumns::STANDARD.name),
        branch: or(widths.branch, RepoColumns::STANDARD.branch),
        standing: or(widths.standing, STANDING_WIDTH),
        changes: or(widths.changes, CHANGES_WIDTH),
        fetched: or(widths.fetched, FETCHED_WIDTH),
        ..RepoColumns::STANDARD
    }
}

/// `remembered` with the one column of `drag` set to its dragged width.
fn widths_after(drag: &ColumnDrag, remembered: ReposColumnWidths) -> ReposColumnWidths {
    // The kit clamps a width to 80..=800, so the cast cannot truncate.
    let pixels = Some(drag.current.of(drag.column).round() as u16);
    match drag.column {
        RepoColumn::Name => ReposColumnWidths {
            name: pixels,
            ..remembered
        },
        RepoColumn::Branch => ReposColumnWidths {
            branch: pixels,
            ..remembered
        },
        RepoColumn::Standing => ReposColumnWidths {
            standing: pixels,
            ..remembered
        },
        RepoColumn::Changes => ReposColumnWidths {
            changes: pixels,
            ..remembered
        },
        RepoColumn::Fetched => ReposColumnWidths {
            fetched: pixels,
            ..remembered
        },
    }
}

pub fn press(state: &mut State, column: RepoColumn, settings: &ReposSettings) {
    let start = remembered(settings);
    state.column_drag = Some(ColumnDrag {
        column,
        start,
        origin: None,
        current: start,
    });
}

pub fn moved(state: &mut State, x: f32) {
    if let Some(drag) = state.column_drag.as_mut() {
        let origin = *drag.origin.get_or_insert(x);
        drag.current = drag.start.dragged(drag.column, x - origin);
    }
}

/// Ends the drag; the widths when they changed.
pub fn release(state: &mut State, settings: &ReposSettings) -> Option<Action> {
    state
        .column_drag
        .take()
        .filter(|drag| drag.current != drag.start)
        .map(|drag| Action::ColumnsResized(widths_after(&drag, settings.columns)))
}
