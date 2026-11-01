use std::path::{Path, PathBuf};

use crate::{QueryHit, QuerySpec, SavedQueryId};

/// The questions the search asks the document index. Runs on a blocking
/// thread, so it may take its time.
pub trait SearchIndex: Send + Sync {
    /// The documents of `spec` at or below `under`.
    fn run_under(&self, spec: &QuerySpec, under: &Path) -> Result<Vec<QueryHit>, String>;

    /// The pieces of text around `words` of the documents at `paths`.
    fn snippets(&self, paths: &[PathBuf], words: &str) -> Result<Vec<(PathBuf, String)>, String>;

    /// Overwrites the criteria of the saved query `id` and keeps its name.
    fn replace_spec(&self, id: SavedQueryId, spec: &QuerySpec) -> Result<(), String>;
}
