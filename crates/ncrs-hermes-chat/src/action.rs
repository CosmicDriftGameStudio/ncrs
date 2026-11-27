use std::path::PathBuf;

/// What the chat asks the app for, because it reaches outside the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Close,
    /// Opens the folder picker; the answer comes back as `Message::FolderPicked`.
    PickFolder,
    OpenSearch,
    /// Opens the dialog with rule suggestions for a folder. `pattern` is the
    /// order the user confirmed, relative to the folder; `None` when the files
    /// show no order.
    SuggestRules {
        folder: PathBuf,
        pattern: Option<String>,
    },
    /// Opens the dialog that files a folder; `None` takes the active folder.
    OpenSortDialog {
        folder: Option<PathBuf>,
    },
    /// Opens an empty rule form.
    OpenRuleForm,
    /// Switches Hermes on, or starts it again when it stopped.
    TurnOnHermes,
    /// Opens the download of the language model; `Message::ModelReady`
    /// follows when it is there.
    DownloadModel,
    /// Switches the AI setting on; `Message::AiTurnedOn` follows once it is
    /// applied.
    TurnOnAi,
    /// The work would have changed files while the trial does not allow it;
    /// it did not run.
    ChangesLocked,
    /// Measures the drive for the Platz answer; `Message::SpaceMeasured` or
    /// `Message::SpaceMeasureFailed` follows.
    MeasureSpace,
    /// Opens the Platz tab.
    OpenSpace,
    /// Opens the Platz tab on the confirm of this run; `Message::SpaceRunDone`
    /// follows when it ran.
    SpaceRun(SpaceRunKind),
}

/// What a Platz run clears; the run itself goes through the Platz tab and its
/// confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceRunKind {
    DeleteSafe,
    RemoveWorktrees,
}
