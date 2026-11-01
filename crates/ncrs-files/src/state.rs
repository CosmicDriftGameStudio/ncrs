//! The state the two panels share: both panels, which one is active, the
//! type-ahead prefix and the drag between the panels.

use std::time::{Duration, Instant};

use iced::keyboard::Modifiers;
use iced::mouse;
use ncrs_core::PanelSide;

use crate::folder_size::{FolderSizeRun, FolderSizes};
use crate::panel::PanelState;
use crate::panel_drag::{self, PanelDrag};

/// How long a typed character still belongs to the same prefix.
const TYPE_AHEAD_WINDOW: Duration = Duration::from_secs(1);

/// What has been typed to jump to an entry.
#[derive(Debug, Default)]
pub struct TypeAhead {
    prefix: String,
    last_typed: Option<Instant>,
}

impl TypeAhead {
    /// The prefix after `typed` arrives at `now`: it extends the previous one
    /// within the window and starts a new one after it.
    pub fn extend(&mut self, typed: &str, now: Instant) -> &str {
        let continues = self
            .last_typed
            .is_some_and(|last| now.saturating_duration_since(last) <= TYPE_AHEAD_WINDOW);
        if !continues {
            self.prefix.clear();
        }
        self.prefix.push_str(typed);
        self.last_typed = Some(now);
        &self.prefix
    }

    pub fn reset(&mut self) {
        self.prefix.clear();
        self.last_typed = None;
    }
}

#[derive(Debug)]
pub struct Panels {
    pub left: PanelState,
    pub right: PanelState,
    pub active: PanelSide,
    pub type_ahead: TypeAhead,
    /// A row is held with the left button; it is a drag once the pointer moved.
    pub drag: Option<PanelDrag>,
    /// The panel under the pointer, which is where a drag can be dropped.
    pub hovered: Option<PanelSide>,
    /// The folder sizes measured in this session.
    pub folder_sizes: FolderSizes,
    /// The "Compute folder sizes" run that is under way.
    pub folder_size_run: Option<FolderSizeRun>,
    pub folder_size_runs_started: u64,
}

impl Panels {
    pub fn new(left: PanelState, right: PanelState, active: PanelSide) -> Self {
        Self {
            left,
            right,
            active,
            type_ahead: TypeAhead::default(),
            drag: None,
            hovered: None,
            folder_sizes: FolderSizes::new(),
            folder_size_run: None,
            folder_size_runs_started: 0,
        }
    }

    pub fn panel(&self, side: PanelSide) -> &PanelState {
        match side {
            PanelSide::Left => &self.left,
            PanelSide::Right => &self.right,
        }
    }

    pub fn panel_mut(&mut self, side: PanelSide) -> &mut PanelState {
        match side {
            PanelSide::Left => &mut self.left,
            PanelSide::Right => &mut self.right,
        }
    }

    pub fn active_panel(&self) -> &PanelState {
        self.panel(self.active)
    }

    pub fn active_panel_mut(&mut self) -> &mut PanelState {
        self.panel_mut(self.active)
    }

    /// Rows a page of this panel moves by: a hit list fits fewer, taller rows.
    pub fn visible_rows_of(&self, side: PanelSide, folder_rows: usize) -> usize {
        self.panel(side).rows_in_view(folder_rows)
    }

    /// A row dragged from the other panel is over `side`.
    pub fn drag_hovers_side(&self, side: PanelSide) -> bool {
        self.drag.is_some_and(|drag| {
            drag.is_dragging() && drag.source().other() == side && self.hovered == Some(side)
        })
    }

    /// A cursor only where a drag from a panel is over a panel; files held over
    /// the window from another app keep the OS cursor.
    pub fn drag_cursor(&self, side: PanelSide, modifiers: Modifiers) -> Option<mouse::Interaction> {
        let drag = self.drag.filter(|drag| drag.is_dragging())?;
        if self.hovered != Some(side) {
            return None;
        }
        if drag.source() == side {
            Some(mouse::Interaction::NoDrop)
        } else if panel_drag::move_modifier_held(modifiers) {
            Some(mouse::Interaction::Move)
        } else {
            Some(mouse::Interaction::Copy)
        }
    }
}
