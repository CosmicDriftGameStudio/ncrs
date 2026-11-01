//! What follows a file name in a row: the chips and how many more there are.

/// One chip after a file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChip {
    pub tag: String,
    /// The user set it; the others were read from the document.
    pub own: bool,
}

/// What follows a file name: at most `MOST_CHIPS` (in the app) chips and how many more
/// there are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShownTags {
    pub chips: Vec<FileChip>,
    pub more: usize,
}
