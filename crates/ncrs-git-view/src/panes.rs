//! How wide the sidebar and the detail pane are, and what dragging a splitter
//! or pressing its keys does to them.

use ncrs_ui_kit::splitter::{Direction, PaneBounds, PaneWidth, Step};
use ncrs_ui_kit::theme::size;

pub const SIDEBAR_BOUNDS: PaneBounds = PaneBounds::new(200.0, 480.0, size::SIDE_PANE);
pub const DETAIL_BOUNDS: PaneBounds = PaneBounds::new(280.0, 720.0, size::DETAIL_PANE);

/// The two splitters: left of the commit list and right of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Split {
    Sidebar,
    Detail,
}

impl Split {
    pub const fn bounds(self) -> PaneBounds {
        match self {
            Self::Sidebar => SIDEBAR_BOUNDS,
            Self::Detail => DETAIL_BOUNDS,
        }
    }

    /// Moving the pointer to the right makes the sidebar wider and the detail pane narrower.
    pub const fn grows_rightwards(self) -> bool {
        matches!(self, Self::Sidebar)
    }
}

/// The widths in logical pixels, as the app remembers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneWidths {
    pub sidebar: u16,
    pub detail: u16,
}

impl PaneWidths {
    /// The widths the tab starts with.
    pub const STANDARD: Self = Self {
        sidebar: 300,
        detail: 450,
    };

    /// The saved widths, or the standard ones where none was saved.
    pub fn saved(sidebar: Option<u16>, detail: Option<u16>) -> Self {
        Self {
            sidebar: sidebar.unwrap_or(Self::STANDARD.sidebar),
            detail: detail.unwrap_or(Self::STANDARD.detail),
        }
    }

    fn raw(self, split: Split) -> u16 {
        match split {
            Split::Sidebar => self.sidebar,
            Split::Detail => self.detail,
        }
    }

    /// The pane of `split`, brought into its bounds.
    pub fn pane(self, split: Split) -> PaneWidth {
        PaneWidth::at(split.bounds(), f32::from(self.raw(split)))
    }

    pub fn width(self, split: Split) -> f32 {
        self.pane(split).width()
    }

    pub fn with(self, split: Split, pane: PaneWidth) -> Self {
        let pixels = pane.width().round() as u16;
        match split {
            Split::Sidebar => Self {
                sidebar: pixels,
                ..self
            },
            Split::Detail => Self {
                detail: pixels,
                ..self
            },
        }
    }

    /// `split` after the pointer moved by `delta` (positive: to the right).
    pub fn dragged(self, split: Split, delta: f32) -> Self {
        let signed = if split.grows_rightwards() {
            delta
        } else {
            -delta
        };
        self.with(split, self.pane(split).dragged(signed))
    }

    pub fn nudged(self, split: Split, step: Step, direction: Direction) -> Self {
        self.with(split, self.pane(split).nudged(step, direction))
    }

    pub fn reset(self, split: Split) -> Self {
        self.with(split, self.pane(split).reset())
    }
}

impl Default for PaneWidths {
    fn default() -> Self {
        Self::STANDARD
    }
}

/// A splitter being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneDrag {
    pub split: Split,
    /// The widths when the press began.
    pub start: PaneWidths,
    /// Where the pointer was at the first move after the press.
    pub origin: Option<f32>,
    /// The widths as dragged so far.
    pub current: PaneWidths,
}

impl PaneDrag {
    pub fn begin(split: Split, widths: PaneWidths) -> Self {
        Self {
            split,
            start: widths,
            origin: None,
            current: widths,
        }
    }

    pub fn moved(&mut self, x: f32) {
        let origin = *self.origin.get_or_insert(x);
        self.current = self.start.dragged(self.split, x - origin);
    }
}
