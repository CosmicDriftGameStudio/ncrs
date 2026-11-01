//! Everything a file panel sends.

use std::path::PathBuf;

use iced::Point;
use ncrs_core::{FileEntry, PanelSide};
use ncrs_fs::{ReadError, SortColumn, SortKey};

#[derive(Debug, Clone, PartialEq)]
pub enum PanelMessage {
    /// Result of an async directory read.
    DirectoryLoaded {
        side: PanelSide,
        /// Matches `PanelState::request_id`; stale results are dropped.
        request_id: u64,
        path: PathBuf,
        entries: Vec<FileEntry>,
        error: Option<ReadError>,
        /// The order the reader put `entries` in.
        sorted_by: SortKey,
        /// Entry name to select after loading (e.g. the dir we came from).
        select: Option<String>,
        /// The archive file, when `path` lies inside one.
        archive: Option<PathBuf>,
    },

    // --- Navigation inside the active panel ---
    MoveSelection(isize),
    PageUp,
    PageDown,
    SelectFirst,
    SelectLast,
    OpenSelected,
    GoUp,
    /// A printable key: jump to the first entry that starts with what was
    /// typed in the last second.
    TypeAhead(String),
    /// Alt+Left: the active panel returns to the folder it was in before.
    HistoryBack,
    /// Alt+Right: undo of [`PanelMessage::HistoryBack`].
    HistoryForward,

    // --- Selection ---
    /// Insert or Space: tag or untag the row under the cursor and go down one.
    ToggleTag,
    /// Shift+Up/Down: tag or untag the row under the cursor, then move.
    TagMove(isize),
    /// `*`: tag everything except `..`.
    TagAll,
    /// Ctrl+`*`: drop every tag.
    ClearTags,

    // --- Panel handling ---
    /// Wheel or trackpad over a panel.
    PanelScrolled {
        side: PanelSide,
        delta: iced::mouse::ScrollDelta,
    },
    SwitchPanel,
    /// A click on a path tab in the title bar: that panel becomes the active one.
    ActivatePanel(PanelSide),
    /// Mouse click on a row of a panel.  is true when the click landed in
    /// the tag column, which toggles the tag instead of moving the cursor.
    RowClicked {
        side: PanelSide,
        index: usize,
        on_tag: bool,
    },
    /// Right click on a row of a panel: the context menu is the app's.
    RowContext {
        side: PanelSide,
        index: usize,
    },
    /// The pointer entered a panel.
    PanelEntered(PanelSide),
    /// The pointer left a panel.
    PanelExited(PanelSide),
    /// The pointer moved while a row is held with the left button.
    PanelDragMoved(Point),
    /// The left button went up while a row was held.
    PanelDragReleased,
    /// Double click on a row of a panel: the row's Enter.
    RowActivated {
        side: PanelSide,
        index: usize,
    },

    /// A click on a column title: sort that panel by the column.
    SortBy {
        side: PanelSide,
        column: SortColumn,
    },
    /// Ctrl+F3, Ctrl+F5 or Ctrl+F6: sort the active panel.
    SortActive(SortColumn),
}

impl PanelMessage {
    /// Moving the cursor or the focus by other means ends a type-ahead prefix.
    pub fn resets_type_ahead(&self) -> bool {
        matches!(
            self,
            Self::MoveSelection(_)
                | Self::PageUp
                | Self::PageDown
                | Self::SelectFirst
                | Self::SelectLast
                | Self::SwitchPanel
                | Self::ActivatePanel(_)
                | Self::RowClicked { .. }
                | Self::RowActivated { .. }
                | Self::TagMove(_)
                | Self::ToggleTag
                | Self::OpenSelected
                | Self::GoUp
                | Self::HistoryBack
                | Self::HistoryForward
        )
    }
}
