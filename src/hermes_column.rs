//! What the Hermes column of a file panel knows: the state of each listed
//! file, the counts and role of each listed folder. Pure state, no widgets.
//!
//! The index and the pipeline are read elsewhere (`ColumnReader`, the runner
//! snapshot); this module holds what they returned and decides what is stale.
//! Stale data keeps being shown until the new reading replaces it, so a file
//! that changes does not flicker through an empty state.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ncrs_index::{DocumentStatus, FileTags, Kinds, PathState};
use ncrs_intake::LocationMode;
use ncrs_pipeline::{Snapshot, Task};

use crate::app::fill_template;
use crate::config::FilingLocation;
use crate::file_list_tags::{shown_tags, FileListTags, ShownTags, TagExceptions};
use crate::i18n::{Language, Msg};
use crate::review_work::TextState;
use crate::ui::format;
use crate::ui::review_list::reason_text;

pub use ncrs_filing::column_reading::{ColumnReading, ColumnRequest, ColumnStage, DirectoryStates};
pub use ncrs_ui_kit::hermes_state::{state_label, FolderRole, FolderSummary, HermesFileState};

trait FolderRoleOf {
    fn of(location: &FilingLocation) -> Self;
}

impl FolderRoleOf for FolderRole {
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

#[derive(Debug, Default)]
pub struct HermesColumn {
    directories: HashMap<PathBuf, CachedDirectory>,
    folders: HashMap<PathBuf, CachedFolder>,
    busy: HashSet<PathBuf>,
    /// Newest last, with the message.
    errors: Vec<(PathBuf, String)>,
    seen_sequence: u64,
    /// Counts up whenever cached data was invalidated.
    epoch: u64,
    /// What the status bar needs for the file under the cursor.
    details: HashMap<PathBuf, FileDetail>,
}

/// What the review list knows about a file, for the status bar line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDetail {
    pub text_state: TextState,
    /// The place the closest rule would file it in.
    pub suggestion: Option<PathBuf>,
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
            .filter(|error| !snapshot.is_superseded(error))
            .map(|error| (error.file.clone(), error.message.clone()))
            .collect();
        let failed: Vec<&PathBuf> = self.errors.iter().map(|(path, _)| path).collect();
        self.folders.iter_mut().for_each(|(folder, cached)| {
            cached.summary.errors = count_under(failed.iter().copied(), folder);
            cached.summary.reading = count_under(&self.busy, folder);
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
        self.epoch += 1;
        self.details.remove(path);
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
        self.epoch += 1;
        self.directories.clear();
        self.folders.clear();
        self.details.clear();
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

    /// Stores a reading unless something was invalidated since it was asked
    /// for; `false` then, and the caller asks again.
    pub fn apply(&mut self, reading: ColumnReading) -> bool {
        if reading.epoch != self.epoch {
            return false;
        }
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
            let errors = count_under(self.errors.iter().map(|(path, _)| path), &folder.path);
            let reading_now = count_under(&self.busy, &folder.path);
            let cached = self.folders.entry(folder.path).or_default();
            cached.summary.excluded = folder.excluded;
            cached.summary.errors = errors;
            cached.summary.reading = reading_now;
            if folder.direct.is_some() {
                cached.summary.direct = folder.direct;
                cached.direct_fresh = true;
            }
            if folder.deep.is_some() {
                cached.summary.deep = folder.deep;
                cached.deep_fresh = true;
            }
        }
        true
    }

    /// Counts up whenever cached data was invalidated; a request carries the
    /// value it was made at.
    pub const fn epoch(&self) -> u64 {
        self.epoch
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
        if self.errors.iter().any(|(failed, _)| failed == path) {
            return Some(HermesFileState::Error);
        }
        Some(match self.stored_state(path)?.status {
            DocumentStatus::InReview => HermesFileState::Inquiry,
            DocumentStatus::Filed => HermesFileState::Filed,
            DocumentStatus::New => HermesFileState::Unread,
            DocumentStatus::Planned => HermesFileState::Planned,
        })
    }

    /// The tags of the document at the path; none for a file without an
    /// index entry.
    pub fn tags_of(&self, path: &Path) -> Option<&FileTags> {
        self.stored_state(path).map(|stored| &stored.tags)
    }

    fn stored_state(&self, path: &Path) -> Option<&PathState> {
        self.directories.get(path.parent()?)?.states.files.get(path)
    }

    pub fn summary_of(&self, folder: &Path) -> Option<&FolderSummary> {
        self.folders.get(folder).map(|cached| &cached.summary)
    }

    /// Whether the status bar line for this file still lacks its detail.
    pub fn wants_detail(&self, path: &Path) -> bool {
        self.state_of(path) == Some(HermesFileState::Inquiry) && !self.details.contains_key(path)
    }

    pub fn apply_detail(&mut self, path: PathBuf, detail: FileDetail) {
        self.details.insert(path, detail);
    }

    /// The line for the status bar: the state, why, and for an inquiry the
    /// place the closest rule would file the file in.
    pub fn status_line(&self, path: &Path, lang: Language) -> Option<String> {
        let state = self.state_of(path)?;
        let mut line = state_label(state, lang).to_owned();
        if let Some(reason) = self.reason_of(path, state, lang) {
            line.push_str(": ");
            line.push_str(&reason);
        }
        let suggestion = self
            .details
            .get(path)
            .and_then(|detail| detail.suggestion.as_ref());
        if let (HermesFileState::Inquiry, Some(target)) = (state, suggestion) {
            line.push_str(" \u{b7} ");
            line.push_str(&fill_template(
                lang.text(Msg::HermesColumnSuggestion),
                &[("{target}", &target.display().to_string())],
            ));
        }
        Some(line)
    }

    fn reason_of(&self, path: &Path, state: HermesFileState, lang: Language) -> Option<String> {
        match state {
            HermesFileState::Inquiry => {
                let reason = self.stored_state(path)?.review?;
                let text_state = self
                    .details
                    .get(path)
                    .map_or(TextState::Read, |detail| detail.text_state);
                Some(reason_text(reason, text_state, lang))
            }
            HermesFileState::Error => self
                .errors
                .iter()
                .rev()
                .find(|(failed, _)| failed == path)
                .and_then(|(_, message)| message.lines().next())
                .map(|first| format::ellipsize_end(first, ERROR_REASON_CHARS)),
            HermesFileState::Unread
            | HermesFileState::Planned
            | HermesFileState::Reading
            | HermesFileState::Filed
            | HermesFileState::Excluded => None,
        }
    }
}

/// How much of an error message the status bar line shows.
const ERROR_REASON_CHARS: usize = 70;

/// What a panel needs from the column to draw its rows: lookups only.
#[derive(Debug, Clone, Copy)]
pub struct ColumnView<'a> {
    column: &'a HermesColumn,
    locations: &'a [FilingLocation],
    /// How far the spinner of a file being read has turned, in radians.
    angle: f32,
    /// Which tags follow a file name.
    tags: &'a FileListTags,
    /// The tags that differ from the default of their group.
    exceptions: &'a TagExceptions,
    /// The width of one panel, to see how much of a name is left.
    panel_width: f32,
    kinds: &'a Kinds,
}

impl<'a> ColumnView<'a> {
    pub const fn new(
        column: &'a HermesColumn,
        locations: &'a [FilingLocation],
        angle: f32,
        tags: &'a FileListTags,
        exceptions: &'a TagExceptions,
        panel_width: f32,
        kinds: &'a Kinds,
    ) -> Self {
        Self {
            column,
            locations,
            angle,
            tags,
            exceptions,
            panel_width,
            kinds,
        }
    }

    pub const fn panel_width(&self) -> f32 {
        self.panel_width
    }

    /// The chips after the name of the file; none for a folder or a file the
    /// index does not know.
    pub fn tags_after(&self, path: &Path) -> ShownTags {
        self.column
            .tags_of(path)
            .map_or_else(ShownTags::default, |tags| {
                shown_tags(tags, self.tags, self.exceptions, self.kinds)
            })
    }

    pub const fn angle(&self) -> f32 {
        self.angle
    }

    pub fn state_of(&self, path: &Path) -> Option<HermesFileState> {
        self.column.state_of(path)
    }

    pub fn summary_of(&self, folder: &Path) -> Option<&'a FolderSummary> {
        self.column.summary_of(folder)
    }

    pub fn role_of(&self, folder: &Path) -> Option<FolderRole> {
        role_of(folder, self.locations)
    }
}

#[cfg(test)]
impl HermesColumn {
    /// Files being read and files that failed, as a snapshot would say.
    pub fn mark_for_test(&mut self, busy: Vec<PathBuf>, errors: Vec<(PathBuf, String)>) {
        self.busy = busy.into_iter().collect();
        self.errors = errors;
    }
}

fn count_under<'a>(paths: impl IntoIterator<Item = &'a PathBuf>, folder: &Path) -> usize {
    paths
        .into_iter()
        .filter(|path| path.starts_with(folder))
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
    fn p(path: &str) -> PathBuf {
        PathBuf::from(path.replace('/', std::path::MAIN_SEPARATOR_STR))
    }

    use ncrs_filing::column_reading::{read_column, FolderReading};
    use ncrs_index::{Index, ReviewReason, StatusCounts};
    use ncrs_pipeline::{
        Busy, ChangeOutcome, ChangedPath, ErrorEntry, WorkerSnapshot, MAX_CHANGED_PATHS,
    };
    use std::time::UNIX_EPOCH;

    use super::*;

    fn location(path: &str, mode: LocationMode, is_target: bool) -> FilingLocation {
        FilingLocation {
            path: p(path),
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
                    let path = p(directory).join(name);
                    (
                        path.clone(),
                        PathState {
                            path,
                            status: *status,
                            review: None,
                            tags: FileTags::default(),
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
            epoch: 0,
            directory: p(directory),
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
            path: p(path),
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
                    path: p(path),
                    outcome: ChangeOutcome::Finished,
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
        names.iter().map(|name| p(name)).collect()
    }

    fn column_with_inquiry() -> HermesColumn {
        let path = p("/in/a.pdf");
        let mut column = HermesColumn::default();
        column.apply(ColumnReading {
            states: Some(DirectoryStates::sample(
                vec![PathState {
                    path: path.clone(),
                    status: DocumentStatus::InReview,
                    review: Some(ReviewReason::NoMatchingRule),
                    tags: FileTags::default(),
                }],
                Vec::new(),
            )),
            ..listing("/in", &[])
        });
        column
    }

    #[test]
    fn the_status_line_names_state_reason_and_suggestion() {
        let path = &p("/in/a.pdf");
        let mut column = column_with_inquiry();
        let without_detail = column.status_line(path, Language::English).unwrap();
        column.apply_detail(
            path.to_path_buf(),
            FileDetail {
                text_state: TextState::Read,
                suggestion: Some(PathBuf::from("Receipts/2025/a.pdf")),
            },
        );

        let with_detail = column.status_line(path, Language::English).unwrap();

        assert!(
            without_detail.starts_with("Please review: "),
            "{without_detail}"
        );
        assert!(!without_detail.contains("Suggestion"), "{without_detail}");
        assert!(
            with_detail.ends_with(" \u{b7} Suggestion: Receipts/2025/a.pdf"),
            "{with_detail}"
        );
        assert!(column
            .status_line(&p("/in/none.pdf"), Language::English)
            .is_none());
    }

    #[test]
    fn an_error_line_carries_the_shortened_message() {
        let mut column = column_with_inquiry();
        let long = "x".repeat(300);
        column.mark_for_test(
            Vec::new(),
            vec![(p("/in/a.pdf"), format!("{long}\nsecond line"))],
        );

        let line = column
            .status_line(&p("/in/a.pdf"), Language::English)
            .unwrap();

        assert!(line.starts_with("Error: xxx"), "{line}");
        assert!(line.chars().count() < 100, "{line}");
        assert!(!line.contains("second line"), "{line}");
    }

    #[test]
    fn a_detail_is_wanted_once_for_an_inquiry_only() {
        let mut column = column_with_inquiry();
        let path = p("/in/a.pdf");
        assert!(column.wants_detail(&path));

        column.apply_detail(
            path.clone(),
            FileDetail {
                text_state: TextState::Read,
                suggestion: None,
            },
        );

        assert!(!column.wants_detail(&path));
    }

    #[test]
    fn roles_follow_mode_and_target_and_need_the_exact_path() {
        let locations = [
            location("/in", LocationMode::Inbox, false),
            location("/target", LocationMode::Archive, true),
            location("/old", LocationMode::Archive, false),
        ];
        assert_eq!(role_of(&p("/in"), &locations), Some(FolderRole::Inbox));
        assert_eq!(
            role_of(&p("/target"), &locations),
            Some(FolderRole::FilingLocation)
        );
        assert_eq!(role_of(&p("/old"), &locations), Some(FolderRole::Archive));
        assert_eq!(role_of(&p("/old/2024"), &locations), None);
        assert_eq!(role_of(&p("/other"), &locations), None);
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
        let state = |name: &str| column.state_of(&p("/d").join(name));
        assert_eq!(state("new.pdf"), Some(HermesFileState::Unread));
        assert_eq!(state("planned.pdf"), Some(HermesFileState::Planned));
        assert_eq!(state("review.pdf"), Some(HermesFileState::Inquiry));
        assert_eq!(state("filed.pdf"), Some(HermesFileState::Filed));
        assert_eq!(state("unknown.pdf"), None);
        assert_eq!(column.state_of(&p("/elsewhere/a.pdf")), None);
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
                file: p(file),
                task: Task::Step("text"),
                started_at: UNIX_EPOCH,
            }),
        };
        snapshot.workers = vec![on("/d/a.pdf"), on("/d/b.pdf")];
        snapshot.errors = ["/d/a.pdf", "/d/b.pdf", "/d/c.pdf"]
            .iter()
            .map(|file| ErrorEntry {
                file: p(file),
                task: Task::Step("text"),
                message: String::new(),
                at: UNIX_EPOCH,
                sequence: 0,
            })
            .collect();

        column.set_snapshot(&snapshot);

        let state = |name: &str| column.state_of(&p("/d").join(name));
        assert_eq!(state("a.pdf"), Some(HermesFileState::Excluded));
        assert_eq!(state("b.pdf"), Some(HermesFileState::Reading));
        assert_eq!(state("c.pdf"), Some(HermesFileState::Error));
    }

    #[test]
    fn an_error_ends_when_a_later_change_of_the_file_did_not_fail() {
        let mut column = HermesColumn::default();
        column.apply(listing("/d", &[("a.pdf", DocumentStatus::Filed)]));
        let failure = |sequence| ChangedPath {
            sequence,
            path: p("/d/a.pdf"),
            outcome: ChangeOutcome::Failed,
        };
        let mut snapshot = Snapshot::off();
        snapshot.changed_paths = vec![failure(1)];
        snapshot.changed_sequence = 1;
        snapshot.errors = vec![ErrorEntry {
            file: p("/d/a.pdf"),
            task: Task::Step("text"),
            message: "broken".to_owned(),
            at: UNIX_EPOCH,
            sequence: 1,
        }];
        column.set_snapshot(&snapshot);
        assert_eq!(
            column.state_of(&p("/d/a.pdf")),
            Some(HermesFileState::Error)
        );

        snapshot.changed_paths.push(ChangedPath {
            outcome: ChangeOutcome::Finished,
            ..failure(2)
        });
        snapshot.changed_sequence = 2;
        column.set_snapshot(&snapshot);

        assert_eq!(
            column.state_of(&p("/d/a.pdf")),
            Some(HermesFileState::Filed)
        );
    }

    #[test]
    fn a_file_in_an_excluded_folder_is_excluded() {
        let mut column = HermesColumn::default();
        let mut reading = listing("/d/private", &[("a.pdf", DocumentStatus::New)]);
        reading.states.as_mut().unwrap().excluded_itself = true;
        column.apply(reading);
        assert_eq!(
            column.state_of(&p("/d/private/a.pdf")),
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
                file: p("/d/a.pdf"),
                task: Task::Scan,
                started_at: UNIX_EPOCH,
            }),
        }];
        column.set_snapshot(&snapshot);
        assert_eq!(column.state_of(&p("/d/a.pdf")), None);
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
        let stale_but_shown = column.summary_of(&p("/a")).unwrap();
        assert!(stale_but_shown.deep.is_some());
    }

    #[test]
    fn a_changed_file_makes_only_the_listing_of_its_own_folder_stale() {
        let mut column = loaded();
        let listed = directories(&["/a", "/b"]);

        column.consume_changes(&snapshot_with(&[(1, "/a/x.pdf")], 1, 0));

        assert_eq!(column.wanted_listings(&listed), directories(&["/a"]));
        assert_eq!(
            column.state_of(&p("/a/x.pdf")),
            Some(HermesFileState::Unread)
        );
    }

    #[test]
    fn only_changes_after_the_last_seen_sequence_count() {
        let mut column = loaded();
        column.consume_changes(&snapshot_with(&[(1, "/a/x.pdf")], 1, 0));
        let mut reading = listing("/a", &[("x.pdf", DocumentStatus::Filed)]);
        reading.epoch = column.epoch();
        assert!(column.apply(reading));

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
        assert!(column.summary_of(&p("/a")).is_none());

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
        column.invalidate_path(&p("/a/sub"));
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
                file: p(file),
                task: Task::Step("text"),
                message: String::new(),
                at: UNIX_EPOCH,
                sequence: 0,
            })
            .collect();

        column.set_snapshot(&snapshot);

        assert_eq!(column.summary_of(&p("/a")).unwrap().errors, 2);
        assert_eq!(column.summary_of(&p("/a/sub")).unwrap().errors, 1);
        assert_eq!(column.summary_of(&p("/a/s")).unwrap().errors, 0);
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
                    path: p(path),
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
        index.exclude_path(&p("/d/Scan/sub")).unwrap();
        let request = |stage| ColumnRequest {
            generation: 7,
            epoch: 0,
            directory: p("/d/Scan"),
            subfolders: directories(&["/d/Scan/sub"]),
            stage,
        };
        let mut column = HermesColumn::default();

        let first = read_column(&index, &request(ColumnStage::Listing)).unwrap();
        assert_eq!(first.generation, 7);
        column.apply(first);

        assert_eq!(
            column.state_of(&p("/d/Scan/a.pdf")),
            Some(HermesFileState::Unread)
        );
        let sub = column.summary_of(&p("/d/Scan/sub")).unwrap();
        assert_eq!(sub.direct.unwrap().new, 1);
        assert_eq!(sub.deep, None);
        assert!(sub.excluded);
        assert_eq!(
            column.wanted_counts(&directories(&["/d/Scan/sub"]), ColumnStage::Deep),
            directories(&["/d/Scan/sub"])
        );

        column.apply(read_column(&index, &request(ColumnStage::Deep)).unwrap());

        let counted_deep = column.summary_of(&p("/d/Scan/sub")).unwrap();
        assert_eq!(counted_deep.deep.unwrap().new, 1);
        assert_eq!(counted_deep.direct.unwrap().new, 1);
    }
}
