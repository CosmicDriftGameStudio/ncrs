use std::path::PathBuf;

/// What the page asks the app for, because it reaches outside the page. The
/// app runs it right after `update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The page opens: the app closes the other pages.
    Opened,
    /// The page closed.
    Closed,
    /// The active panel shows `folder`, with the cursor on `file` when it is
    /// named, and the page closes.
    GoToFolder {
        folder: PathBuf,
        file: Option<String>,
    },
    CopyToClipboard(String),
}
