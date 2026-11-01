use std::path::PathBuf;

use crate::{DocumentFields, FileStamp};

/// One document a query found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryHit {
    pub path: PathBuf,
    pub fields: DocumentFields,
    /// The index knows the file is gone.
    pub missing: bool,
    /// Size and date of the file when the index read it.
    pub stamp: Option<FileStamp>,
}
