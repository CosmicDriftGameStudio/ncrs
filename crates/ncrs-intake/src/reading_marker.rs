//! Names the files being read right now in a file next to the index. A file
//! that crashes the process in a decoder is still named there at the next
//! start. One such start is not proof (the app may just have been closed while
//! reading), so the file is read again; named at two starts in a row, it is
//! quarantined instead of crashing the app on every start.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// How many starts in a row must have found a file in the marker before it
/// is quarantined.
const STARTS_BEFORE_QUARANTINE: u32 = 2;

#[derive(Debug)]
pub struct ReadingMarker {
    file: PathBuf,
    /// Per file named in the marker found at start: the number of starts in a
    /// row that found it there, this one included.
    starts_seen: Mutex<BTreeMap<String, u32>>,
    /// The files being read, with the value written for them.
    in_flight: Mutex<Vec<(String, u32)>>,
}

/// The line key of a path. Paths that are not UTF-8 compare by their lossy
/// form, which is stable between runs.
fn key_of(path: &Path) -> String {
    path.to_string_lossy().replace(['\n', '\t'], " ")
}

impl ReadingMarker {
    pub fn open(file: PathBuf) -> Self {
        let mut starts_seen = BTreeMap::new();
        for line in std::fs::read_to_string(&file).unwrap_or_default().lines() {
            if let Some((count, name)) = line.split_once('\t') {
                let before: u32 = count.parse().unwrap_or(0);
                starts_seen.insert(name.to_owned(), before.saturating_add(1));
            }
        }
        // Best effort: a marker that cannot be removed only means a stale
        // line is read again, and counted once more only if it is written
        // again.
        let _ = std::fs::remove_file(&file);
        Self {
            file,
            starts_seen: Mutex::new(starts_seen),
            in_flight: Mutex::default(),
        }
    }

    pub fn is_quarantined(&self, path: &Path) -> bool {
        self.starts_seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key_of(path))
            .is_some_and(|starts| *starts >= STARTS_BEFORE_QUARANTINE)
    }

    /// Reads the file again from now on, for a person who asked for it.
    pub fn forget_crash(&self, path: &Path) {
        self.starts_seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&key_of(path));
    }

    fn write(&self, names: &[(String, u32)]) {
        if names.is_empty() {
            let _ = std::fs::remove_file(&self.file);
            return;
        }
        let lines: Vec<String> = names
            .iter()
            .map(|(name, starts)| format!("{starts}\t{name}"))
            .collect();
        let _ = std::fs::write(&self.file, lines.join("\n"));
    }

    pub fn begin(&self, path: &Path) {
        let key = key_of(path);
        let starts = self
            .starts_seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
            .copied()
            .unwrap_or(0);
        let mut names = self
            .in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        names.push((key, starts));
        self.write(&names);
    }

    pub fn end(&self, path: &Path) {
        let key = key_of(path);
        let mut names = self
            .in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(at) = names.iter().position(|(name, _)| *name == key) {
            names.swap_remove(at);
        }
        self.write(&names);
    }
}

impl Drop for ReadingMarker {
    /// Closing the app while a file is being read must not count as a crash.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::mem_forget
)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Starts the app, begins reading `path` and "crashes": nothing is
    /// cleaned up.
    fn crash_while_reading(file: &Path, path: &Path) {
        let marker = ReadingMarker::open(file.to_path_buf());
        marker.begin(path);
        std::mem::forget(marker);
    }

    #[test]
    fn a_file_found_once_in_the_marker_is_read_again() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("reading-now");
        let path = Path::new("/inbox/a.pdf");
        crash_while_reading(&file, path);

        let marker = ReadingMarker::open(file);

        assert!(!marker.is_quarantined(path));
    }

    #[test]
    fn a_file_found_at_two_starts_in_a_row_is_quarantined_until_forgotten() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("reading-now");
        let path = Path::new("/inbox/a.pdf");
        crash_while_reading(&file, path);
        crash_while_reading(&file, path);

        let marker = ReadingMarker::open(file);
        assert!(marker.is_quarantined(path));
        assert!(!marker.is_quarantined(Path::new("/inbox/b.pdf")));

        marker.forget_crash(path);
        assert!(!marker.is_quarantined(path));
    }

    #[test]
    fn closing_the_app_while_reading_does_not_count_as_a_crash() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("reading-now");
        let path = Path::new("/inbox/a.pdf");
        for _ in 0..3 {
            let marker = ReadingMarker::open(file.clone());
            marker.begin(path);
            drop(marker);
        }

        assert!(!ReadingMarker::open(file).is_quarantined(path));
    }

    #[test]
    fn a_file_that_finished_between_two_crashes_starts_counting_again() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("reading-now");
        let path = Path::new("/inbox/a.pdf");
        crash_while_reading(&file, path);
        let marker = ReadingMarker::open(file.clone());
        marker.begin(path);
        marker.end(path);
        std::mem::forget(marker);
        crash_while_reading(&file, path);

        assert!(!ReadingMarker::open(file).is_quarantined(path));
    }
}
