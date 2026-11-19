//! What a transfer leaves out while the rest goes on, so the caller can say so
//! afterwards instead of reporting a clean finish.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipKind {
    /// A link that was not copied or could not be created.
    Link,
    /// A socket, device or FIFO.
    Special,
    /// Extended attributes the target refused.
    Attributes,
    /// An unpacked entry whose name another entry had already taken, as two
    /// names that differ only in case do on most Mac and Windows disks.
    NameTaken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub path: PathBuf,
    pub kind: SkipKind,
    /// The system's wording or the names of the attributes, for a log line.
    pub detail: String,
}
