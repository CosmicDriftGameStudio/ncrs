use std::path::PathBuf;

use iced::Point;
use ncrs_core::{PanelSide, TransferKind};
use ncrs_fs::remote::{RemoteUrl, ServerId};

/// What a panel asks the app for, because it reaches outside the panels. The
/// app runs the actions of one `update` in order, right after it.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Reads `path` into `side`; `select` is the entry to put the cursor on.
    Load {
        side: PanelSide,
        path: PathBuf,
        select: Option<String>,
    },
    /// The load that was just started is a Back or Forward step.
    MarkHistoryStep(PanelSide),
    /// Takes the job error and the notice off the status line.
    ClearMessages,
    /// Opens the context menu on a row at the pointer.
    OpenContextMenu {
        side: PanelSide,
        index: usize,
        pointer: Point,
    },
    /// The context menu of a right click on a row, through the app's own message.
    ContextMenuAt {
        side: PanelSide,
        index: usize,
    },
    CloseContextMenu,
    /// A reload of `side` makes an open context menu stale.
    CloseMenuOfReloaded(PanelSide),
    /// The copies of the files of this archive or server made for viewing are
    /// no longer needed.
    DropViewCopies(PathBuf),
    /// A reload of the same folder keeps the folder size run; another folder ends it.
    StopFolderSizeRunIfLeft(PanelSide),
    /// A row is held (`true`) or let go (`false`): the raw event stream needs to know.
    DragArmed(bool),
    /// The pointer left the window with files held.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    StartDragOut,
    /// A row was dropped on the other panel: asks the F5 or F6 question.
    OpenTransferDialog(TransferKind),
    /// Enter on the cursor row, through the app's own message so that the
    /// refusals of archives and hit lists apply.
    OpenSelected,
    CloseSettings,
    /// Shows `text` in the status bar for a while.
    ShowNotice(String),
    /// Shows `text` as the error of the status bar.
    ShowError(String),
    /// The trash row of the drive menu: shows the trash on `side`.
    OpenTrash(PanelSide),
    /// A server closed from its row in the drive menu.
    Disconnect(ServerId),
    /// Gives the keyboard focus back to the panels.
    ReleaseFocus,
    /// The connect dialog for an address typed without a user.
    OpenConnectDialog(RemoteUrl),
}
