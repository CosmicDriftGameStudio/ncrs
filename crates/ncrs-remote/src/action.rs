use std::path::PathBuf;

use ncrs_core::PanelSide;
use ncrs_fs::remote::{RemotePath, ServerId};

/// What the connect feature asks of the app, which it cannot do itself. The app
/// runs the actions in order, right after `update`.
#[derive(Debug)]
pub enum Action {
    CloseContextMenu,
    ClearJobError,
    ShowNotice(String),
    ShowError(String),
    /// A saved server was chosen: the status line clears and the active panel
    /// opens its address.
    OpenServer(PathBuf),
    /// The panel opens `path`, which is on the server just connected.
    LoadFolder {
        side: PanelSide,
        path: PathBuf,
        select: Option<String>,
    },
    /// The server goes into `[[servers]]`, or its start folder is updated.
    SaveServer {
        server: ServerId,
        typed_path: Option<String>,
    },
    /// The dialog that was to open a folder is gone: the panel stops waiting
    /// for it, unless it has asked for something else since.
    StopWaiting {
        side: PanelSide,
        request_id: u64,
    },
    /// The connection is closed: each panel that was on the server goes home.
    LeaveServer(ServerId),
    /// The connection is made: the panel lists the folder.
    ListFolder {
        side: PanelSide,
        request_id: u64,
        remote: RemotePath,
        select: Option<String>,
    },
    /// No dialog can open for the folder: the panel is told it has no
    /// connection.
    ReportDisconnected {
        side: PanelSide,
        request_id: u64,
        remote: RemotePath,
    },
}
