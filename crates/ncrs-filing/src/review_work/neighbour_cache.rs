//! The neighbours of the archive are costly to build: every document and its
//! text is read and scanned. They are kept while nothing they were built from
//! has changed.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ncrs_index::{ChangeStamp, Index, KindCatalog};
use ncrs_intake::{location_documents, location_structure, NeighbourIndex, OwnNames};
use ncrs_pipeline::OwnDetails;

/// What the neighbours were built from; they stay valid while it is the same.
struct Built {
    stamp: ChangeStamp,
    kinds: Arc<KindCatalog>,
    target_root: PathBuf,
    own: OwnDetails,
    neighbours: Arc<NeighbourIndex>,
}

/// Shared by every handle of one module, so the next review list or quick fix
/// finds what the last one built.
#[derive(Clone, Default)]
pub struct NeighbourCache {
    built: Arc<Mutex<Option<Built>>>,
    #[cfg(test)]
    builds: Arc<std::sync::atomic::AtomicUsize>,
}

impl std::fmt::Debug for NeighbourCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NeighbourCache").finish_non_exhaustive()
    }
}

impl NeighbourCache {
    pub fn neighbours(
        &self,
        index: &Index,
        target_root: &Path,
        own: &OwnDetails,
    ) -> Result<Arc<NeighbourIndex>, String> {
        let stamp = index.change_stamp();
        let mut built = self.built.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(kept) = built.as_ref().filter(|kept| {
            kept.stamp == stamp
                && Arc::ptr_eq(&kept.kinds, index.kinds())
                && kept.target_root == target_root
                && kept.own == *own
        }) {
            return Ok(Arc::clone(&kept.neighbours));
        }
        #[cfg(test)]
        self.builds
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let documents = location_documents(index, target_root).map_err(|err| err.to_string())?;
        let structure = location_structure(&documents, index.kinds());
        let names = OwnNames {
            names: &own.names,
            ibans: &own.ibans,
            tax_ids: &own.tax_ids,
        };
        let neighbours = Arc::new(NeighbourIndex::new(&documents, &names, &structure));
        *built = Some(Built {
            stamp,
            kinds: Arc::clone(index.kinds()),
            target_root: target_root.to_path_buf(),
            own: own.clone(),
            neighbours: Arc::clone(&neighbours),
        });
        Ok(neighbours)
    }

    #[cfg(test)]
    pub fn builds(&self) -> usize {
        self.builds.load(std::sync::atomic::Ordering::Relaxed)
    }
}
