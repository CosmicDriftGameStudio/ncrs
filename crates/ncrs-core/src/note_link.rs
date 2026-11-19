use std::path::PathBuf;

/// What a note on a file knows about its file, and what the index answers for
/// it: the document number and content hash name the file even after it moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteLink {
    /// The id of the file in the index, when it was indexed.
    pub document: Option<i64>,
    /// The content hash of the file as lowercase hex, when it was known.
    pub hash: Option<String>,
    pub file: PathBuf,
}
