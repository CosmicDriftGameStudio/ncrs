use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::step::Item;

/// A file of a location that files by itself, which the rules left open: it
/// waits here until someone takes it for the background filing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuousCandidate {
    /// The root of the location the file lies in.
    pub location: PathBuf,
    pub path: PathBuf,
}

#[derive(Debug, Default)]
pub(in crate::runner) struct Candidates {
    waiting: Vec<ContinuousCandidate>,
    seeded: bool,
    /// Files handed to the background filing already. A file it left lying
    /// stays new in the index, so every rescan passes it on again.
    // ponytail: by path, so a file whose bytes change under the same name waits for the next app start.
    taken: HashSet<PathBuf>,
}

impl Candidates {
    /// A file that is processed again replaces its earlier entry; one taken
    /// once is not offered again.
    pub(in crate::runner) fn add_from(&mut self, item: &Item) {
        if !item.continuous_candidate || self.taken.contains(&item.path) {
            return;
        }
        self.waiting.retain(|known| known.path != item.path);
        self.waiting.push(ContinuousCandidate {
            location: item.location.path.clone(),
            path: item.path.clone(),
        });
    }

    /// Files the index knew before this start; the ones the runner found
    /// itself in the meantime are not doubled.
    pub(in crate::runner) fn seed(&mut self, found: Vec<ContinuousCandidate>) {
        self.seeded = true;
        for candidate in found {
            if self
                .waiting
                .iter()
                .all(|known| known.path != candidate.path)
            {
                self.waiting.push(candidate);
            }
        }
    }

    pub(in crate::runner) fn len(&self) -> usize {
        self.waiting.len()
    }

    pub(in crate::runner) fn is_seeded(&self) -> bool {
        self.seeded
    }

    pub(in crate::runner) fn any_in(&self, wanted: &dyn Fn(&Path) -> bool) -> bool {
        self.waiting.iter().any(|known| wanted(&known.location))
    }

    pub(in crate::runner) fn take_in(
        &mut self,
        wanted: &dyn Fn(&Path) -> bool,
    ) -> Vec<ContinuousCandidate> {
        let (taken, kept) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition(|known| wanted(&known.location));
        self.waiting = kept;
        self.taken
            .extend(taken.iter().map(|candidate| candidate.path.clone()));
        taken
    }
}
