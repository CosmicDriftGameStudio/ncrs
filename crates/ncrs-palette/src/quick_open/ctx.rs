use std::path::PathBuf;

/// What Quick Open reads from the app.
#[derive(Debug, Clone)]
pub struct QuickOpenCtx {
    /// The folder the box lists the files of: the active panel's.
    pub root: PathBuf,
    /// Nothing else is up that the box must not open over.
    pub may_open: bool,
    /// There is an index to search the documents' text in.
    pub documents_available: bool,
}
