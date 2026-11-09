use std::collections::HashMap;
use std::ffi::OsString;

/// What the comparison of two folders found out about one entry, seen from the
/// panel the entry is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareMark {
    /// The other folder has nothing of this name.
    OnlyHere,
    NewerHere,
    OlderHere,
    /// Both sides have it and it differs, without a date that says which side
    /// is the newer one.
    Different,
    Same,
}

impl CompareMark {
    /// Everything but "same": what "only differences" keeps.
    pub const fn is_difference(self) -> bool {
        !matches!(self, Self::Same)
    }
}

/// The marks of the entries of one panel's folder, by entry name.
pub type CompareMarks = HashMap<OsString, CompareMark>;
