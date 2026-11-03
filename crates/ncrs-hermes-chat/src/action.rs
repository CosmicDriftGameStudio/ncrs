use std::path::PathBuf;

use ncrs_ui_kit::hermes_state::FolderRole;

/// What the chat asks the app for, because it reaches outside the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Close,
    /// Opens the folder picker; the answer comes back as `Message::FolderPicked`.
    PickFolder,
    OpenSearch,
    SetFolderRole {
        folder: PathBuf,
        role: FolderRole,
    },
    OpenFilingSetup,
    /// Opens the download of the language model; `Message::ModelReady`
    /// follows when it is there.
    DownloadModel,
}
