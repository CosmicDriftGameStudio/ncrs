use std::path::{Path, PathBuf};

use crate::{DocumentFields, QueryHit, QuerySpec, SavedQueryId};

/// The questions the search asks the document index. Runs on a blocking
/// thread, so it may take its time.
pub trait SearchIndex: Send + Sync {
    /// The documents of `spec` at or below `under` whose file is still there,
    /// at most `limit` of them.
    fn run_under(
        &self,
        spec: &QuerySpec,
        under: &Path,
        limit: usize,
    ) -> Result<Vec<QueryHit>, String>;

    /// The pieces of text around `words` of the documents at `paths`.
    fn snippets(&self, paths: &[PathBuf], words: &str) -> Result<Vec<(PathBuf, String)>, String>;

    /// The fields read for the documents at `paths`; a path the index does
    /// not know has none.
    fn document_fields(&self, paths: &[PathBuf]) -> Result<Vec<(PathBuf, DocumentFields)>, String>;

    /// Overwrites the criteria of the saved query `id` and keeps its name.
    fn replace_spec(&self, id: SavedQueryId, spec: &QuerySpec) -> Result<(), String>;
}
