use std::path::PathBuf;

use ncrs_core::PanelSide;
use ncrs_fs::remote::session::ConnectFailure;
use ncrs_fs::remote::{RemotePath, Secret, ServerId};

/// A field of the dialog that connects to a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectField {
    Host,
    Port,
    User,
    Password,
    StartPath,
}

/// Everything the SFTP connections and their dialog send.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// The key action and the command: opens the connect dialog.
    OpenConnect,
    /// What was typed into a field other than the password.
    Field(ConnectField, String),
    Password(Secret),
    ToggleRemember,
    /// Tab: the next field, or the one before it.
    FocusField {
        backwards: bool,
    },
    Submit,
    Cancel,
    /// The answer to the attempt the dialog made. Dropped when the dialog has
    /// been closed or has started another attempt since.
    Connected {
        attempt: u64,
        result: Result<(), ConnectFailure>,
    },
    ToggleSave,
    /// Opens the connect dialog with the box that saves the server ticked.
    OpenAdd,
    /// A saved server was chosen: its address opens in the active panel.
    OpenServer(PathBuf),
    TrustHostKey,
    /// The host key was written to the file of ncrs; connect again.
    HostKeyLearned {
        attempt: u64,
        result: Result<(), String>,
    },
    /// The command and its key action: closes the connection of the active
    /// panel's server.
    DisconnectActive,
    Disconnect(ServerId),
    /// What the keychain holds for the server of a folder that was asked for
    /// while there is no connection. Dropped when the panel has asked for
    /// something else since.
    PasswordLoaded {
        side: PanelSide,
        request_id: u64,
        remote: RemotePath,
        select: Option<String>,
        result: Result<Option<Secret>, String>,
    },
    /// The silent connection with a password from the keychain is done.
    AutoConnected {
        side: PanelSide,
        request_id: u64,
        remote: RemotePath,
        select: Option<String>,
        password: Secret,
        result: Result<(), ConnectFailure>,
    },
    /// The password was stored in the keychain, or why it was not.
    PasswordStored(Result<(), String>),
}
