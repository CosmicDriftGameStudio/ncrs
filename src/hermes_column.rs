//! What the Hermes column of a file panel knows: the state of each listed
//! file, the counts and role of each listed folder. Pure state, no widgets.
//!
//! The index and the pipeline are read elsewhere (`ColumnReader`, the runner
//! snapshot); this module holds what they returned and decides what is stale.
//! Stale data keeps being shown until the new reading replaces it, so a file
//! that changes does not flicker through an empty state.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ncrs_index::{DocumentStatus, Index, IndexError, PathState, StatusCounts};
use ncrs_intake::LocationMode;
use ncrs_pipeline::{Snapshot, Task};

use crate::config::FilingLocation;

/// What the status column shows for a file, strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HermesFileState {
    Waiting,
    Reading,
    Filed,
    /// Hermes asked the user a question about the file.
    Inquiry,
    Error,
    Excluded,
}

/// The part a configured folder plays for Hermes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRole {
    Inbox,
    /// An archive that files are moved into.
    FilingLocation,
    /// An archive that is only read.
    Archive,
}

impl FolderRole {
    fn of(location: &FilingLocation) -> Self {
        match (location.mode, location.is_target) {
            (LocationMode::Inbox, _) => Self::Inbox,
            (LocationMode::Archive, true) => Self::FilingLocation,
            (LocationMode::Archive, false) => Self::Archive,
        }
    }
}

/// The role of exactly this path; a folder below a location has none.
pub fn role_of(path: &Path, locations: &[FilingLocation]) -> Option<FolderRole> {
    locations
        .iter()
        .find(|location| location.path == path)
        .map(FolderRole::of)
}

/// What the index holds about the files directly in one folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirectoryStates {
    files: HashMap<PathBuf, PathState>,
    /// Excluded paths below the folder.
    excluded_below: Vec<PathBuf>,
    /// The folder itself is excluded, or one around it.
    excluded_itself: bool,
}

impl DirectoryStates {
    fn is_excluded(&self, path: &Path) -> bool {
        self.excluded_itself
            || self
                .excluded_below
                .iter()
                .any(|excluded| path.starts_with(excluded))
    }
}

/// The counts and flags of one folder. `None` counts are not read yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderSummary {
    pub direct: Option<StatusCounts>,
    pub deep: Option<StatusCounts>,
    pub excluded: bool,
    /// Errors of the current pipeline run for files in or below the folder.
    pub errors: usize,
}

#[derive(Debug, Clone, Default)]
struct CachedFolder {
    summary: FolderSummary,
    direct_fresh: bool,
    deep_fresh: bool,
}

#[derive(Debug, Clone, Default)]
struct CachedDirectory {
    states: DirectoryStates,
    stale: bool,
}

/// How much of a folder's counts one reading holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnStage {
    /// The states of the listed files and the direct counts of the subfolders.
    Listing,
    /// The counts of everything below each subfolder; slower on a deep tree.
    Deep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRequest {
    /// The listing generation the request was made for; the answer is dropped
    /// when the panel has moved on.
    pub generation: u64,
    pub directory: PathBuf,
    pub subfolders: Vec<PathBuf>,
    pub stage: ColumnStage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderReading {
    pub path: PathBuf,
    pub excluded: bool,
    pub direct: Option<StatusCounts>,
    pub deep: Option<StatusCounts>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnReading {
    pub generation: u64,
    pub directory: PathBuf,
    pub stage: ColumnStage,
    /// Only in the `Listing` stage.
    pub states: Option<DirectoryStates>,
    pub folders: Vec<FolderReading>,
}

/// Everything one panel needs from the index, in one go.
pub fn read_column(index: &Index, request: &ColumnRequest) -> Result<ColumnReading, IndexError> {
    let (states, folders) = match request.stage {
        ColumnStage::Listing => (
            Some(read_directory(index, &request.directory)?),
            read_folders(index, &request.subfolders, false)?,
        ),
        ColumnStage::Deep => (None, read_folders(index, &request.subfolders, true)?),
    };
    Ok(ColumnReading {
        generation: request.generation,
        directory: request.directory.clone(),
        stage: request.stage,
        states,
        folders,
    })
}

fn read_directory(index: &Index, directory: &Path) -> Result<DirectoryStates, IndexError> {
    Ok(DirectoryStates {
        files: index
            .document_states_in(directory)?
            .into_iter()
            .map(|state| (state.path.clone(), state))
            .collect(),
        excluded_below: index.excluded_paths_under(directory)?,
        excluded_itself: index.is_excluded(directory)?,
    })
}

fn read_folders(
    index: &Index,
    folders: &[PathBuf],
    deep: bool,
) -> Result<Vec<FolderReading>, IndexError> {
    let (direct_stage, deep_stage) = if deep {
        let both = index.status_counts_under(folders)?;
        (
            vec![None; folders.len()],
            both.into_iter().map(|counts| Some(counts.deep)).collect(),
        )
    } else {
        (
            index
                .direct_status_counts_under(folders)?
                .into_iter()
                .map(Some)
                .collect(),
            vec![None; folders.len()],
        )
    };
    folders
        .iter()
        .zip(direct_stage)
        .zip(deep_stage)
        .map(|((path, direct_counts), deep_counts)| {
            Ok(FolderReading {
                path: path.clone(),
                excluded: index.is_excluded(path)?,
                direct: direct_counts,
                deep: deep_counts,
            })
        })
        .collect()
}

#[derive(Debug, Default)]
pub struct HermesColumn {
    directories: HashMap<PathBuf, CachedDirectory>,
    folders: HashMap<PathBuf, CachedFolder>,
    busy: HashSet<PathBuf>,
    errors: Vec<PathBuf>,
    seen_sequence: u64,
}

impl HermesColumn {
    /// Takes in the runner's snapshot: drops what its changes made stale, and
    /// remembers which files are being read and which failed.
    pub fn set_snapshot(&mut self, snapshot: &Snapshot) {
        self.consume_changes(snapshot);
        self.busy = snapshot
            .workers
            .iter()
            .filter_map(|worker| worker.busy.as_ref())
            .filter(|busy| matches!(busy.task, Task::Step(_)))
            .map(|busy| busy.file.clone())
            .collect();
        self.errors = snapshot
            .errors
            .iter()
            .map(|error| error.file.clone())
            .collect();
        self.folders.iter_mut().for_each(|(folder, cached)| {
            cached.summary.errors = count_under(&self.errors, folder);
        });
    }

    /// Marks stale what the changes after the last seen one touched; all of it
    /// when some were missed.
    pub fn consume_changes(&mut self, snapshot: &Snapshot) {
        match snapshot.changes_since(self.seen_sequence) {
            Some(changes) => {
                let changed: Vec<PathBuf> = changes.map(|change| change.path.clone()).collect();
                for path in &changed {
                    self.invalidate_path(path);
                }
            }
            None => self.invalidate_all(),
        }
        self.seen_sequence = snapshot.changed_sequence;
    }

    /// The file at `path` changed, was added, moved, removed or excluded: its
    /// folder is read again, and so are the counts of every folder around it
    /// (and of the path itself, if it is a folder).
    pub fn invalidate_path(&mut self, path: &Path) {
        if let Some(cached) = path
            .parent()
            .and_then(|parent| self.directories.get_mut(parent))
        {
            cached.stale = true;
        }
        if let Some(cached) = self.directories.get_mut(path) {
            cached.stale = true;
        }
        for ancestor in path.ancestors() {
            if let Some(cached) = self.folders.get_mut(ancestor) {
                cached.direct_fresh = false;
                cached.deep_fresh = false;
            }
        }
    }

    pub fn invalidate_all(&mut self) {
        self.directories.clear();
        self.folders.clear();
    }

    /// Of `directories`, the ones whose file states are missing or stale.
    pub fn wanted_listings(&self, directories: &[PathBuf]) -> Vec<PathBuf> {
        directories
            .iter()
            .filter(|directory| {
                self.directories
                    .get(*directory)
                    .is_none_or(|cached| cached.stale)
            })
            .cloned()
            .collect()
    }

    /// Of `folders`, the ones whose counts of this stage are missing or stale.
    pub fn wanted_counts(&self, folders: &[PathBuf], stage: ColumnStage) -> Vec<PathBuf> {
        folders
            .iter()
            .filter(|folder| {
                self.folders.get(*folder).is_none_or(|cached| match stage {
                    ColumnStage::Listing => !cached.direct_fresh,
                    ColumnStage::Deep => !cached.deep_fresh,
                })
            })
            .cloned()
            .collect()
    }

    /// Stores a reading. The caller has checked its generation.
    pub fn apply(&mut self, reading: ColumnReading) {
        if let Some(states) = reading.states {
            self.directories.insert(
                reading.directory,
                CachedDirectory {
                    states,
                    stale: false,
                },
            );
        }
        for folder in reading.folders {
            let errors = count_under(&self.errors, &folder.path);
            let cached = self.folders.entry(folder.path).or_default();
            cached.summary.excluded = folder.excluded;
            cached.summary.errors = errors;
            if folder.direct.is_some() {
                cached.summary.direct = folder.direct;
                cached.direct_fresh = true;
            }
            if folder.deep.is_some() {
                cached.summary.deep = folder.deep;
                cached.deep_fresh = true;
            }
        }
    }

    /// `None` for a file without a state, and for a folder: the column shows
    /// those through [`Self::summary_of`].
    pub fn state_of(&self, path: &Path) -> Option<HermesFileState> {
        let excluded = path
            .parent()
            .and_then(|parent| self.directories.get(parent))
            .is_some_and(|cached| cached.states.is_excluded(path));
        if excluded {
            return Some(HermesFileState::Excluded);
        }
        if self.busy.contains(path) {
            return Some(HermesFileState::Reading);
        }
        if self.errors.iter().any(|failed| failed == path) {
            return Some(HermesFileState::Error);
        }
        let stored = self
            .directories
            .get(path.parent()?)?
            .states
            .files
            .get(path)?;
        Some(match stored.status {
            DocumentStatus::InReview => HermesFileState::Inquiry,
            DocumentStatus::Filed => HermesFileState::Filed,
            DocumentStatus::New | DocumentStatus::Planned => HermesFileState::Waiting,
        })
    }

    pub fn summary_of(&self, folder: &Path) -> Option<&FolderSummary> {
        self.folders.get(folder).map(|cached| &cached.summary)
    }
}

fn count_under(errors: &[PathBuf], folder: &Path) -> usize {
    errors
        .iter()
        .filter(|failed| failed.starts_with(folder))
        .count()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use ncrs_pipeline::{Busy, ChangedPath, ErrorEntry, WorkerSnapshot, MAX_CHANGED_PATHS};
    use std::time::UNIX_EPOCH;

    use super::*;

    fn location(path: &str, mode: LocationMode, is_target: bool) -> FilingLocation {
        FilingLocation {
            path: PathBuf::from(path),
            mode,
            images: false,
            is_target,
        }
    }

    fn states(directory: &str, files: &[(&str, DocumentStatus)]) -> DirectoryStates {
        DirectoryStates {
            files: files
                .iter()
                .map(|(name, status)| {
                    let path = Path::new(directory).join(name);
                    (
                        path.clone(),
                        PathState {
                            path,
                            status: *status,
                            review: None,
                        },
                    )
                })
                .collect(),
            ..DirectoryStates::default()
        }
    }

    fn listing(directory: &str, files: &[(&str, DocumentStatus)]) -> ColumnReading {
        ColumnReading {
            generation: 1,
            directory: PathBuf::from(directory),
            stage: ColumnStage::Listing,
            states: Some(states(directory, files)),
            folders: Vec::new(),
        }
    }

    fn counted(path: &str) -> FolderReading {
        let counts = StatusCounts {
            new: 1,
            ..StatusCounts::default()
        };
        FolderReading {
            path: PathBuf::from(path),
            excluded: false,
            direct: Some(counts),
            deep: Some(counts),
        }
    }

    fn snapshot_with(changes: &[(u64, &str)], latest: u64, floor: u64) -> Snapshot {
        Snapshot {
            changed_paths: changes
                .iter()
                .map(|(sequence, path)| ChangedPath {
                    sequence: *sequence,
                    path: PathBuf::from(path),
                })
                .collect(),
            changed_sequence: latest,
            changed_sequence_floor: floor,
            ..Snapshot::off()
        }
    }

    fn loaded() -> HermesColumn {
        let mut column = HermesColumn::default();
        for directory in ["/a", "/b"] {
            column.apply(listing(directory, &[("x.pdf", DocumentStatus::New)]));
        }
        column.apply(ColumnReading {
            folders: vec![counted("/a"), counted("/a/sub"), counted("/b")],
            ..listing("/", &[])
        });
        column
    }

    fn directories(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn roles_follow_mode_and_target_and_need_the_exact_path() {
        let locations = [
            location("/in", LocationMode::Inbox, false),
            location("/target", LocationMode::Archive, true),
            location("/old", LocationMode::Archive, false),
        ];
        assert_eq!(
            role_of(Path::new("/in"), &locations),
            Some(FolderRole::Inbox)
        );
        assert_eq!(
            role_of(Path::new("/target"), &locations),
            Some(FolderRole::FilingLocation)
        );
        assert_eq!(
            role_of(Path::new("/old"), &locations),
            Some(FolderRole::Archive)
        );
        assert_eq!(role_of(Path::new("/old/2024"), &locations), None);
        assert_eq!(role_of(Path::new("/other"), &locations), None);
    }

    #[test]
    fn the_index_status_maps_to_a_state() {
        let mut column = HermesColumn::default();
        column.apply(listing(
            "/d",
            &[
                ("new.pdf", DocumentStatus::New),
                ("planned.pdf", DocumentStatus::Planned),
                ("review.pdf", DocumentStatus::InReview),
                ("filed.pdf", DocumentStatus::Filed),
            ],
        ));
        let state = |name: &str| column.state_of(&Path::new("/d").join(name));
        assert_eq!(state("new.pdf"), Some(HermesFileState::Waiting));
        assert_eq!(state("planned.pdf"), Some(HermesFileState::Waiting));
        assert_eq!(state("review.pdf"), Some(HermesFileState::Inquiry));
        assert_eq!(state("filed.pdf"), Some(HermesFileState::Filed));
        assert_eq!(state("unknown.pdf"), None);
        assert_eq!(column.state_of(Path::new("/elsewhere/a.pdf")), None);
    }

    #[test]
    fn excluded_beats_reading_beats_error_beats_the_index_status() {
        let mut column = HermesColumn::default();
        let mut reading = listing(
            "/d",
            &[
                ("a.pdf", DocumentStatus::Filed),
                ("b.pdf", DocumentStatus::Filed),
                ("c.pdf", DocumentStatus::Filed),
            ],
        );
        reading.states.as_mut().unwrap().excluded_below = directories(&["/d/a.pdf"]);
        column.apply(reading);
        let mut snapshot = Snapshot::off();
        let on = |file: &str| WorkerSnapshot {
            id: 0,
            busy: Some(Busy {
                file: PathBuf::from(file),
                task: Task::Step("text"),
                started_at: UNIX_EPOCH,
            }),
        };
        snapshot.workers = vec![on("/d/a.pdf"), on("/d/b.pdf")];
        snapshot.errors = ["/d/a.pdf", "/d/b.pdf", "/d/c.pdf"]
            .iter()
            .map(|file| ErrorEntry {
                file: PathBuf::from(file),
                task: Task::Step("text"),
                message: String::new(),
                at: UNIX_EPOCH,
            })
            .collect();

        column.set_snapshot(&snapshot);

        let state = |name: &str| column.state_of(&Path::new("/d").join(name));
        assert_eq!(state("a.pdf"), Some(HermesFileState::Excluded));
        assert_eq!(state("b.pdf"), Some(HermesFileState::Reading));
        assert_eq!(state("c.pdf"), Some(HermesFileState::Error));
    }

    #[test]
    fn a_file_in_an_excluded_folder_is_excluded() {
        let mut column = HermesColumn::default();
        let mut reading = listing("/d/private", &[("a.pdf", DocumentStatus::New)]);
        reading.states.as_mut().unwrap().excluded_itself = true;
        column.apply(reading);
        assert_eq!(
            column.state_of(Path::new("/d/private/a.pdf")),
            Some(HermesFileState::Excluded)
        );
    }

    #[test]
    fn a_scan_task_is_not_a_file_being_read() {
        let mut column = HermesColumn::default();
        let mut snapshot = Snapshot::off();
        snapshot.workers = vec![WorkerSnapshot {
            id: 0,
            busy: Some(Busy {
                file: PathBuf::from("/d/a.pdf"),
                task: Task::Scan,
                started_at: UNIX_EPOCH,
            }),
        }];
        column.set_snapshot(&snapshot);
        assert_eq!(column.state_of(Path::new("/d/a.pdf")), None);
    }

    #[test]
    fn nothing_is_wanted_once_read_and_a_changed_file_makes_its_folder_and_ancestors_stale() {
        let mut column = loaded();
        let listed = directories(&["/a", "/b"]);
        let folders = directories(&["/a", "/a/sub", "/b"]);
        assert!(column.wanted_listings(&listed).is_empty());
        assert!(column
            .wanted_counts(&folders, ColumnStage::Listing)
            .is_empty());
        assert!(column.wanted_counts(&folders, ColumnStage::Deep).is_empty());

        column.consume_changes(&snapshot_with(&[(1, "/a/sub/x.pdf")], 1, 0));

        assert_eq!(column.wanted_listings(&listed), Vec::<PathBuf>::new());
        assert_eq!(
            column.wanted_counts(&folders, ColumnStage::Listing),
            directories(&["/a", "/a/sub"])
        );
        assert_eq!(
            column.wanted_counts(&folders, ColumnStage::Deep),
            directories(&["/a", "/a/sub"])
        );
        let stale_but_shown = column.summary_of(Path::new("/a")).unwrap();
        assert!(stale_but_shown.deep.is_some());
    }

    #[test]
    fn a_changed_file_makes_only_the_listing_of_its_own_folder_stale() {
        let mut column = loaded();
        let listed = directories(&["/a", "/b"]);

        column.consume_changes(&snapshot_with(&[(1, "/a/x.pdf")], 1, 0));

        assert_eq!(column.wanted_listings(&listed), directories(&["/a"]));
        assert_eq!(
            column.state_of(Path::new("/a/x.pdf")),
            Some(HermesFileState::Waiting)
        );
    }

    #[test]
    fn only_changes_after_the_last_seen_sequence_count() {
        let mut column = loaded();
        column.consume_changes(&snapshot_with(&[(1, "/a/x.pdf")], 1, 0));
        column.apply(listing("/a", &[("x.pdf", DocumentStatus::Filed)]));

        column.consume_changes(&snapshot_with(&[(1, "/a/x.pdf"), (2, "/b/x.pdf")], 2, 0));

        assert_eq!(
            column.wanted_listings(&directories(&["/a", "/b"])),
            directories(&["/b"])
        );
    }

    #[test]
    fn missed_changes_and_a_restarted_runner_drop_everything() {
        let mut column = loaded();
        column.consume_changes(&snapshot_with(&[], 3, 0));
        column.apply(listing("/a", &[]));
        let overflow = snapshot_with(
            &[(MAX_CHANGED_PATHS as u64 + 4, "/z/q.pdf")],
            MAX_CHANGED_PATHS as u64 + 4,
            4,
        );

        column.consume_changes(&overflow);

        assert_eq!(
            column.wanted_listings(&directories(&["/a", "/b"])),
            directories(&["/a", "/b"])
        );
        assert!(column.summary_of(Path::new("/a")).is_none());

        column.apply(listing("/a", &[]));
        column.consume_changes(&Snapshot::off());
        assert_eq!(
            column.wanted_listings(&directories(&["/a"])),
            directories(&["/a"])
        );
    }

    #[test]
    fn invalidating_a_path_by_hand_reaches_a_folder_itself_and_its_ancestors() {
        let mut column = loaded();
        column.invalidate_path(Path::new("/a/sub"));
        assert_eq!(
            column.wanted_counts(&directories(&["/a", "/a/sub", "/b"]), ColumnStage::Listing),
            directories(&["/a", "/a/sub"])
        );
    }

    #[test]
    fn errors_under_a_folder_are_counted_per_folder_and_not_per_prefix() {
        let mut column = loaded();
        column.apply(ColumnReading {
            folders: vec![counted("/a/s")],
            ..listing("/a", &[])
        });
        let mut snapshot = Snapshot::off();
        snapshot.errors = ["/a/sub/x.pdf", "/a/y.pdf"]
            .iter()
            .map(|file| ErrorEntry {
                file: PathBuf::from(file),
                task: Task::Step("text"),
                message: String::new(),
                at: UNIX_EPOCH,
            })
            .collect();

        column.set_snapshot(&snapshot);

        assert_eq!(column.summary_of(Path::new("/a")).unwrap().errors, 2);
        assert_eq!(column.summary_of(Path::new("/a/sub")).unwrap().errors, 1);
        assert_eq!(column.summary_of(Path::new("/a/s")).unwrap().errors, 0);
    }

    fn index_with_documents() -> (tempfile::TempDir, Index) {
        let dir = tempfile::TempDir::new().unwrap();
        let index = Index::create_plain(&dir.path().join("index.db")).unwrap();
        for (seed, path) in [
            (1, "/d/Scan/a.pdf"),
            (2, "/d/Scan/sub/b.pdf"),
            (3, "/d/Scans/c.pdf"),
        ] {
            index
                .insert_document(&ncrs_index::NewDocument {
                    hash: ncrs_index::ContentHash([seed; 32]),
                    path: PathBuf::from(path),
                    source: ncrs_index::DocumentSource::Inbox,
                    received_at: chrono::DateTime::UNIX_EPOCH,
                    stamp: None,
                })
                .unwrap();
        }
        (dir, index)
    }

    #[test]
    fn a_reading_fills_the_cache_in_two_stages() {
        let (_dir, index) = index_with_documents();
        index.exclude_path(Path::new("/d/Scan/sub")).unwrap();
        let request = |stage| ColumnRequest {
            generation: 7,
            directory: PathBuf::from("/d/Scan"),
            subfolders: directories(&["/d/Scan/sub"]),
            stage,
        };
        let mut column = HermesColumn::default();

        let first = read_column(&index, &request(ColumnStage::Listing)).unwrap();
        assert_eq!(first.generation, 7);
        column.apply(first);

        assert_eq!(
            column.state_of(Path::new("/d/Scan/a.pdf")),
            Some(HermesFileState::Waiting)
        );
        let sub = column.summary_of(Path::new("/d/Scan/sub")).unwrap();
        assert_eq!(sub.direct.unwrap().new, 1);
        assert_eq!(sub.deep, None);
        assert!(sub.excluded);
        assert_eq!(
            column.wanted_counts(&directories(&["/d/Scan/sub"]), ColumnStage::Deep),
            directories(&["/d/Scan/sub"])
        );

        column.apply(read_column(&index, &request(ColumnStage::Deep)).unwrap());

        let counted_deep = column.summary_of(Path::new("/d/Scan/sub")).unwrap();
        assert_eq!(counted_deep.deep.unwrap().new, 1);
        assert_eq!(counted_deep.direct.unwrap().new, 1);
    }
}
