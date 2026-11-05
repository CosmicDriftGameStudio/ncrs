use std::path::PathBuf;

use iced::mouse::ScrollDelta;

use crate::kill::KillFailure;
use crate::sampler::Depth;
use crate::snapshot::{DriveRow, Snapshot};
use crate::tasks::TaskColumn;

#[derive(Debug, Clone, PartialEq)]
pub enum MonitorMessage {
    /// F12, or the badge in the status bar.
    Toggle,
    Close,
    Tick,
    /// The second measurement after opening, which gives CPU use a reading.
    MeasureAgain,
    Measured(Depth, Box<Snapshot>),
    DrivesMeasured(Vec<DriveRow>),
    MeasureFailed(String),

    Move(isize),
    PageMove(isize),
    First,
    Last,
    ShiftCommands(isize),
    Scroll(ScrollDelta),
    SelectRow(u32),
    SortBy(TaskColumn),
    FilterChanged(String),
    FocusFilter,
    /// A click landed somewhere: the filter field may have lost the focus.
    ClickedAway,
    /// What the toolkit says about the filter field: the only truth about
    /// whether it has the focus.
    FilterFocusChecked(bool),
    /// Enter in the filter field: the list has the keys again, the filter stays.
    FilterDone,
    Escape,

    AskKill,
    ConfirmKill,
    /// Enter in the dialog: presses the button that has the focus.
    DialogEnter,
    CancelKill,
    KillFocus,
    Killed {
        name: String,
        result: Result<(), KillFailure>,
    },

    OpenMenu(u32),
    OpenMenuAtCursor,
    ToggleGroups,
    /// An Option tap: closes the menu when it is open, else opens it on the cursor row.
    MenuKey,
    MenuHover(usize),
    MenuMove(isize),
    MenuChoose(usize),
    MenuActivate,
    MenuClose,

    OpenDrive(PathBuf),
}
