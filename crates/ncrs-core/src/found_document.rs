use std::path::PathBuf;

/// How the text of a found document was read, as far as the file type says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSource {
    Pdf,
    Ocr,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundDocument {
    pub path: PathBuf,
    pub read_by: TextSource,
}
