//! Which files and folders Hermes looks at. It blocks, so `App` runs it on a
//! blocking thread, never in `update`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_core::INDEX_NOT_OPEN;
use ncrs_index::{IncludeOutcome, Index, IndexError};

type IndexSlot = Arc<Mutex<Option<Index>>>;

/// What the file menu and the settings may do with the paths of the index.
#[derive(Clone)]
pub struct PathAccess {
    slot: IndexSlot,
}

impl std::fmt::Debug for PathAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathAccess").finish_non_exhaustive()
    }
}

impl PathAccess {
    pub fn new(slot: IndexSlot) -> Self {
        Self { slot }
    }

    /// Hermes leaves these paths alone from now on.
    pub fn exclude(&self, paths: &[PathBuf]) -> Result<(), String> {
        self.with_index(|index| paths.iter().try_for_each(|path| index.exclude_path(path)))
    }

    /// The documents of locations that were removed from the settings leave
    /// the index.
    pub fn forget_locations(&self, paths: &[PathBuf]) -> Result<(), String> {
        self.with_index(|index| {
            paths
                .iter()
                .try_for_each(|path| index.forget_documents_at(path).map(drop))
        })
    }

    /// Hermes looks at this path again. A folder around it that is excluded
    /// stays so.
    pub fn include(&self, path: &Path) -> Result<IncludeOutcome, String> {
        self.with_index(|index| index.include_path(path))
    }

    fn with_index<T>(
        &self,
        work: impl FnOnce(&mut Index) -> Result<T, IndexError>,
    ) -> Result<T, String> {
        let mut guard = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let index = guard.as_mut().ok_or_else(|| INDEX_NOT_OPEN.to_owned())?;
        work(index).map_err(|err| err.to_string())
    }
}
