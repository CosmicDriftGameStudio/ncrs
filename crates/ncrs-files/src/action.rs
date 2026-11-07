use std::path::PathBuf;

use crate::context_menu::{ContextAction, HermesAction};
use iced::Point;
use ncrs_core::{Column, PanelSide, TransferKind};
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
    /// The chosen file entry of the context menu, as the message its key sends.
    RunContextEntry(ContextAction),
    /// The chosen Hermes entry of the context menu.
    RunHermesEntry(HermesAction),
    /// Enter on a search result in the menu: shows it in the other panel.
    ActivateSearchResult,
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
    /// A click on a panel: the panels are what the window shows again, while
    /// the pages stay open as tabs.
    ShowPanels,
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
    /// The shown columns of `side` changed (ticked, unticked, moved). The app
    /// stores them; with the same columns in both panels they hold for both.
    ColumnsChanged {
        side: PanelSide,
        columns: Vec<Column>,
    },
    /// A column was dragged to `width` pixels.
    ColumnWidth {
        column: Column,
        width: u16,
    },
    /// The "same columns in both panels" entry of the header menu.
    SameColumnsInBoth(bool),
    /// Back to the default columns and widths.
    RestoreColumnDefaults,
    /// Shows the Columns page of the settings.
    OpenColumnSettings,
    /// Enter in the name input: rename `from` to `new_name` in its folder.
    SubmitRename {
        side: PanelSide,
        from: PathBuf,
        new_name: String,
    },
}
