//! What the Hermes column of a file panel knows: the state of each listed
//! file, the counts and role of each listed folder. Pure state, no widgets.
//!
//! The index and the pipeline are read elsewhere (`ColumnReader`, the runner
//! snapshot); this module holds what they returned and decides what is stale.
//! Stale data keeps being shown until the new reading replaces it, so a file
//! that changes does not flicker through an empty state.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use ncrs_index::{DocumentStatus, FileTags, Kinds, PathState};
use ncrs_pipeline::{Snapshot, Task};

use crate::ui::review_reason;
use ncrs_config::config::{same_folder, Filing};
use ncrs_config::file_list_tags::{shown_tags, FileListTags, ShownTags, TagExceptions};
use ncrs_i18n::Language;
use ncrs_ui_kit::format;

const MAX_REMEMBERED_INVALIDATIONS: usize = 256;

pub use ncrs_filing::column_reading::{ColumnReading, ColumnRequest, ColumnStage, DirectoryStates};
pub use ncrs_ui_kit::hermes_state::{state_label, FolderRole, FolderSummary, HermesFileState};

/// The role of exactly this path; a folder below a source or a target has
/// none. A folder that is both is shown as the target.
pub fn role_of(path: &Path, filing: &Filing) -> Option<FolderRole> {
    let is_target = filing
        .targets
        .iter()
        .any(|setting| same_folder(&setting.target.path, path));
    let is_source = filing
        .sources
        .iter()
        .any(|setting| same_folder(&setting.source.path, path));
    if is_target {
        Some(FolderRole::Target)
    } else if is_source {
        Some(FolderRole::Source)
    } else {
        None
    }
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
    /// What each of the latest epochs invalidated, `None` for everything: a
    /// reading is stale only when a later entry touches its directory.
    invalidations: VecDeque<(u64, Option<PathBuf>)>,
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
        self.next_epoch(Some(path.to_path_buf()));
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

    fn next_epoch(&mut self, invalidated: Option<PathBuf>) {
        self.epoch += 1;
        self.invalidations.push_back((self.epoch, invalidated));
        if self.invalidations.len() > MAX_REMEMBERED_INVALIDATIONS {
            self.invalidations.pop_front();
        }
    }

    /// Whether something that reaches `directory` was invalidated after a
    /// reading was asked for at `epoch`. A change elsewhere leaves it valid.
    fn invalidated_since(&self, epoch: u64, directory: &Path) -> bool {
        if epoch == self.epoch {
            return false;
        }
        let forgotten = self
            .invalidations
            .front()
            .is_none_or(|(oldest, _)| *oldest > epoch + 1);
        forgotten
            || self
                .invalidations
                .iter()
                .filter(|(at, _)| *at > epoch)
                .any(|(_, invalidated)| {
                    invalidated.as_deref().is_none_or(|path| {
                        path.starts_with(directory) || directory.starts_with(path)
                    })
                })
    }

    pub fn invalidate_all(&mut self) {
        self.next_epoch(None);
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

    /// Stores a reading unless something was invalidated since it was asked
    /// for; `false` then, and the caller asks again.
    pub fn apply(&mut self, reading: ColumnReading) -> bool {
        if self.invalidated_since(reading.epoch, &reading.directory) {
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

    /// The line for the status bar: the state, and why.
    pub fn status_line(&self, path: &Path, lang: Language) -> Option<String> {
        let state = self.state_of(path)?;
        let mut line = state_label(state, lang).to_owned();
        if let Some(reason) = self.reason_of(path, state, lang) {
            line.push_str(": ");
            line.push_str(&reason);
        }
        Some(line)
    }

    fn reason_of(&self, path: &Path, state: HermesFileState, lang: Language) -> Option<String> {
        match state {
            HermesFileState::Inquiry => {
                let reason = self.stored_state(path)?.review?;
                Some(review_reason::reason_of(reason, lang))
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
    filing: &'a Filing,
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
        filing: &'a Filing,
        angle: f32,
        tags: &'a FileListTags,
        exceptions: &'a TagExceptions,
        panel_width: f32,
        kinds: &'a Kinds,
    ) -> Self {
        Self {
            column,
            filing,
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
        role_of(folder, self.filing)
    }
}

#[cfg(any(test, feature = "test-support"))]
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
mod tests;
