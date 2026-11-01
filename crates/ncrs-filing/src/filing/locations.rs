//! Which configured locations exist on this machine and which overlap.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLocation {
    pub configured: FilingLocation,
    pub status: LocationStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LocationStatus {
    Ready(Location),
    NotReachable {
        reason: String,
    },
    /// Equal to or nested in another location, by file identity. Both
    /// members of the pair get this, so neither the target nor one spelling of
    /// a linked inbox is silently dropped.
    Overlaps {
        with: PathBuf,
    },
}

/// Looks every configured folder up on disk. Blocking: a dead mount can hang
/// `canonicalize`, so this runs in the start work and never on the UI thread.
pub fn resolve_locations(configured: &[FilingLocation]) -> Vec<ResolvedLocation> {
    let mut resolved: Vec<ResolvedLocation> = configured
        .iter()
        .map(|entry| ResolvedLocation {
            configured: entry.clone(),
            status: match Location::new(&entry.path, entry.mode, entry.images) {
                Ok(location) => LocationStatus::Ready(location),
                Err(err) => LocationStatus::NotReachable {
                    reason: err.to_string(),
                },
            },
        })
        .collect();

    let ready: Vec<(usize, &Path)> = resolved
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match &entry.status {
            LocationStatus::Ready(location) => Some((index, location.path.as_path())),
            LocationStatus::NotReachable { .. } | LocationStatus::Overlaps { .. } => None,
        })
        .collect();
    let mut overlapping_pairs = Vec::new();
    for (position, (first, first_path)) in ready.iter().enumerate() {
        for (second, second_path) in ready.iter().skip(position + 1) {
            if paths_overlap(first_path, second_path) {
                overlapping_pairs.push((*first, *second));
            }
        }
    }

    for (first, second) in overlapping_pairs {
        mark_overlap(&mut resolved, first, second);
        mark_overlap(&mut resolved, second, first);
    }
    resolved
}

pub(in crate::filing) fn mark_overlap(
    resolved: &mut [ResolvedLocation],
    index: usize,
    other: usize,
) {
    let Some(other_path) = resolved
        .get(other)
        .map(|entry| entry.configured.path.clone())
    else {
        return;
    };
    if let Some(entry) = resolved.get_mut(index) {
        if matches!(entry.status, LocationStatus::Ready(_)) {
            entry.status = LocationStatus::Overlaps { with: other_path };
        }
    }
}

pub(in crate::filing) fn paths_overlap(first: &Path, second: &Path) -> bool {
    lies_within(first, second) || lies_within(second, first)
}

/// `inner` is `outer` or below it. Compared by file identity, not by spelling:
/// case-insensitive file systems and symlinks name one folder in several ways.
pub(in crate::filing) fn lies_within(inner: &Path, outer: &Path) -> bool {
    inner
        .ancestors()
        .any(|ancestor| same_file::is_same_file(ancestor, outer).unwrap_or(false))
}
