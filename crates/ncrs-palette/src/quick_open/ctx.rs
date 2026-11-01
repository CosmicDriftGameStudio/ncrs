/// What Quick Open reads from the app.
#[derive(Debug, Clone, Copy)]
pub struct QuickOpenCtx {
    /// There is an index to search the documents' text in.
    pub documents_available: bool,
}
