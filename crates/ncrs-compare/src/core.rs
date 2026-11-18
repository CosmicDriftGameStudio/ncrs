//! What two folders have in common, decided from their listings. Pure: no
//! disk, no clock; the scan reads the listings and the content and asks.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ncrs_core::{CompareMark, CompareMarks, FileEntry, PanelSide};

/// Servers and FAT keep dates to two seconds; closer dates are the same date.
pub const DATE_TOLERANCE: Duration = Duration::from_secs(2);

/// What the start dialog asks. The name always counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choices {
    pub size: bool,
    pub date: bool,
    pub content: bool,
    pub subfolders: bool,
}

impl Default for Choices {
    fn default() -> Self {
        Self {
            size: true,
            date: true,
            content: false,
            subfolders: true,
        }
    }
}

/// What the comparison knows of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl Facts {
    pub fn of(entry: &FileEntry) -> Self {
        Self {
            size: entry.size,
            modified: entry.modified,
        }
    }
}

/// How two files of one name compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Judgement {
    Same,
    LeftNewer,
    RightNewer,
    Different,
}

impl Judgement {
    /// The marks of the left and the right file.
    pub const fn marks(self) -> (CompareMark, CompareMark) {
        match self {
            Self::Same => (CompareMark::Same, CompareMark::Same),
            Self::LeftNewer => (CompareMark::NewerHere, CompareMark::OlderHere),
            Self::RightNewer => (CompareMark::OlderHere, CompareMark::NewerHere),
            Self::Different => (CompareMark::Different, CompareMark::Different),
        }
    }
}

/// Which side has the later date, `None` when the dates are the same within
/// [`DATE_TOLERANCE`] or one is unknown.
fn newer_side(left: Facts, right: Facts) -> Option<Ordering> {
    let (left_date, right_date) = (left.modified?, right.modified?);
    let ordering = left_date.cmp(&right_date);
    let apart = match ordering {
        Ordering::Less => right_date.duration_since(left_date),
        Ordering::Equal | Ordering::Greater => left_date.duration_since(right_date),
    }
    .ok()?;
    (apart > DATE_TOLERANCE).then_some(ordering)
}

fn dates_differ(choices: Choices, left: Facts, right: Facts) -> bool {
    choices.date && newer_side(left, right).is_some()
}

/// The content has to be read to tell: it is asked for, the sizes are equal
/// and nothing else has told the files apart yet.
pub fn needs_content(choices: Choices, left: Facts, right: Facts) -> bool {
    choices.content && left.size == right.size && !dates_differ(choices, left, right)
}

/// Judges two files. `same_content` is read only when [`needs_content`] says so.
pub fn judge_files(choices: Choices, left: Facts, right: Facts, same_content: bool) -> Judgement {
    if choices.date {
        match newer_side(left, right) {
            Some(Ordering::Greater) => return Judgement::LeftNewer,
            Some(Ordering::Less) => return Judgement::RightNewer,
            Some(Ordering::Equal) | None => {}
        }
    }
    let sizes_differ = (choices.size || choices.content) && left.size != right.size;
    let content_differs = needs_content(choices, left, right) && !same_content;
    if sizes_differ || content_differs {
        Judgement::Different
    } else {
        Judgement::Same
    }
}

/// The result of one comparison: the marks of the rows of both folders and
/// the rows, for the sync.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Compared {
    pub left: CompareMarks,
    pub right: CompareMarks,
    pub left_rows: Vec<FileEntry>,
    pub right_rows: Vec<FileEntry>,
    pub left_folder: PathBuf,
    pub right_folder: PathBuf,
    /// What was found inside the folders that differ, by `name_key` of the
    /// folder, so a sync copies their files and not the folders as a whole.
    pub below: BTreeMap<OsString, Compared>,
}

/// The name as the two sides are matched by it: composed (NFC), because the
/// same file reads decomposed on macOS, and on macOS and Windows also without
/// case, since those keep one file system entry per name regardless of case.
pub fn name_key(name: &OsStr) -> OsString {
    use unicode_normalization::UnicodeNormalization as _;
    let Some(text) = name.to_str() else {
        return name.to_owned();
    };
    let composed: String = text.nfc().collect();
    if cfg!(any(target_os = "macos", windows)) {
        return OsString::from(composed.to_lowercase());
    }
    OsString::from(composed)
}

impl Compared {
    fn rows(&self, side: PanelSide) -> &[FileEntry] {
        match side {
            PanelSide::Left => &self.left_rows,
            PanelSide::Right => &self.right_rows,
        }
    }

    fn folder(&self, side: PanelSide) -> &Path {
        match side {
            PanelSide::Left => &self.left_folder,
            PanelSide::Right => &self.right_folder,
        }
    }

    pub const fn marks(&self, side: PanelSide) -> &CompareMarks {
        match side {
            PanelSide::Left => &self.left,
            PanelSide::Right => &self.right,
        }
    }

    pub fn tally(&self) -> Tally {
        let count = |marks: &CompareMarks, wanted: CompareMark| {
            marks.values().filter(|mark| **mark == wanted).count()
        };
        Tally {
            only_left: count(&self.left, CompareMark::OnlyHere),
            newer_left: count(&self.left, CompareMark::NewerHere),
            only_right: count(&self.right, CompareMark::OnlyHere),
            newer_right: count(&self.right, CompareMark::NewerHere),
            different: count(&self.left, CompareMark::Different),
            same: count(&self.left, CompareMark::Same),
        }
    }

    /// Whether the row of `side` called `name` differs from the other side.
    pub fn row_differs(&self, side: PanelSide, name: &OsString) -> bool {
        self.marks(side)
            .get(name)
            .is_some_and(|mark| mark.is_difference())
    }
}

/// The figures of the compare bar, in the entries of the two folders.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub only_left: usize,
    pub newer_left: usize,
    pub only_right: usize,
    pub newer_right: usize,
    pub different: usize,
    pub same: usize,
}

/// Which way a sync copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    ToRight,
    ToLeft,
}

impl Direction {
    pub const fn source(self) -> PanelSide {
        match self {
            Self::ToRight => PanelSide::Left,
            Self::ToLeft => PanelSide::Right,
        }
    }

    pub fn target(self) -> PanelSide {
        self.source().other()
    }
}

/// An entry of a sync list.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncRow {
    pub name: OsString,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub mark: CompareMark,
    /// The folder the entry is copied into; for a deletion, the folder it is in.
    pub target_dir: PathBuf,
}

/// What a sync copies and, when asked, deletes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncPlan {
    pub copies: Vec<SyncRow>,
    pub deletions: Vec<SyncRow>,
}

fn row_of(entry: &FileEntry, mark: CompareMark, target_dir: &Path) -> SyncRow {
    SyncRow {
        name: entry.name.clone(),
        path: entry.path.clone(),
        is_dir: entry.is_dir,
        size: entry.size,
        mark,
        target_dir: target_dir.to_owned(),
    }
}

/// Everything of the source that is only there, newer or different is copied.
/// A folder that is on both sides is not copied as a whole, its files are,
/// each into the folder it belongs to: the copy of a folder onto its twin
/// would otherwise be a replacement. What is only on the target is deleted
/// only with `delete`.
pub fn sync_plan(compared: &Compared, direction: Direction, delete: bool) -> SyncPlan {
    let mut plan = SyncPlan::default();
    plan_level(compared, direction, delete, &mut plan);
    plan
}

fn plan_level(compared: &Compared, direction: Direction, delete: bool, plan: &mut SyncPlan) {
    let (source, target) = (direction.source(), direction.target());
    let target_dir = compared.folder(target);
    // A folder in an archive is written as a whole by the archive's own copy.
    let can_descend = ncrs_fs::archive::locate_in_archive(target_dir).is_none();
    for entry in compared.rows(source) {
        let Some(&mark) = compared.marks(source).get(&entry.name) else {
            continue;
        };
        if can_descend && entry.is_dir && mark == CompareMark::Different {
            if let Some(below) = compared.below.get(&name_key(&entry.name)) {
                plan_level(below, direction, delete, plan);
                continue;
            }
        }
        if matches!(
            mark,
            CompareMark::OnlyHere | CompareMark::NewerHere | CompareMark::Different
        ) {
            plan.copies.push(row_of(entry, mark, target_dir));
        }
    }
    if delete {
        for entry in compared.rows(target) {
            if compared.marks(target).get(&entry.name) == Some(&CompareMark::OnlyHere) {
                plan.deletions
                    .push(row_of(entry, CompareMark::OnlyHere, target_dir));
            }
        }
    }
}

/// Nothing is deleted on a server or inside an archive.
pub fn can_delete_in(folder: &Path) -> bool {
    ncrs_fs::remote::RemotePath::parse(folder).is_none()
        && ncrs_fs::archive::locate_in_archive(folder).is_none()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    const EPOCH_SECONDS: u64 = 1_700_000_000;

    #[test]
    fn a_decomposed_and_a_composed_name_are_the_same_name() {
        assert_eq!(
            name_key(OsStr::new("e\u{301}")),
            name_key(OsStr::new("\u{e9}"))
        );
    }

    fn at(seconds: u64) -> Option<SystemTime> {
        Some(SystemTime::UNIX_EPOCH + Duration::from_secs(EPOCH_SECONDS + seconds))
    }

    fn file(size: u64, seconds: u64) -> Facts {
        Facts {
            size,
            modified: at(seconds),
        }
    }

    #[test]
    fn a_later_date_is_newer_whichever_side_it_is_on() {
        let choices = Choices::default();
        assert_eq!(
            judge_files(choices, file(5, 100), file(5, 10), true),
            Judgement::LeftNewer
        );
        assert_eq!(
            judge_files(choices, file(5, 10), file(5, 100), true),
            Judgement::RightNewer
        );
        assert_eq!(
            Judgement::LeftNewer.marks(),
            (CompareMark::NewerHere, CompareMark::OlderHere)
        );
        assert_eq!(
            Judgement::RightNewer.marks(),
            (CompareMark::OlderHere, CompareMark::NewerHere)
        );
    }

    #[test]
    fn dates_within_the_tolerance_are_the_same_date() {
        let choices = Choices::default();
        assert_eq!(
            judge_files(choices, file(5, 10), file(5, 12), true),
            Judgement::Same
        );
        assert_eq!(
            judge_files(choices, file(5, 10), file(5, 13), true),
            Judgement::RightNewer
        );
    }

    #[test]
    fn another_size_with_the_same_date_is_different() {
        assert_eq!(
            judge_files(Choices::default(), file(5, 10), file(6, 10), true),
            Judgement::Different
        );
    }

    #[test]
    fn the_size_does_not_count_when_it_is_not_asked() {
        let choices = Choices {
            size: false,
            ..Choices::default()
        };
        assert_eq!(
            judge_files(choices, file(5, 10), file(6, 10), true),
            Judgement::Same
        );
    }

    #[test]
    fn the_date_does_not_count_when_it_is_not_asked() {
        let choices = Choices {
            date: false,
            ..Choices::default()
        };
        assert_eq!(
            judge_files(choices, file(5, 10), file(5, 900), true),
            Judgement::Same
        );
    }

    #[test]
    fn equal_size_and_date_with_other_content_is_different_only_with_content() {
        let with_content = Choices {
            content: true,
            ..Choices::default()
        };
        assert!(needs_content(with_content, file(5, 10), file(5, 10)));
        assert_eq!(
            judge_files(with_content, file(5, 10), file(5, 10), false),
            Judgement::Different
        );
        assert_eq!(
            judge_files(with_content, file(5, 10), file(5, 10), true),
            Judgement::Same
        );
        assert_eq!(
            judge_files(Choices::default(), file(5, 10), file(5, 10), false),
            Judgement::Same
        );
    }

    #[test]
    fn content_is_read_only_for_files_of_equal_size_that_nothing_else_tells_apart() {
        let choices = Choices {
            content: true,
            ..Choices::default()
        };
        assert!(!needs_content(choices, file(5, 10), file(6, 10)));
        assert!(!needs_content(choices, file(5, 10), file(5, 900)));
        assert!(!needs_content(Choices::default(), file(5, 10), file(5, 10)));
    }

    fn entry(name: &str, is_dir: bool) -> FileEntry {
        let mut entry = FileEntry::parent(Path::new("/"));
        entry.name = OsString::from(name);
        entry.path = PathBuf::from("/side").join(name);
        entry.is_parent = false;
        entry.is_dir = is_dir;
        entry.size = 10;
        entry
    }

    fn compared() -> Compared {
        let name = |text: &str| OsString::from(text);
        Compared {
            left: CompareMarks::from([
                (name("only"), CompareMark::OnlyHere),
                (name("newer"), CompareMark::NewerHere),
                (name("older"), CompareMark::OlderHere),
                (name("differs"), CompareMark::Different),
                (name("same"), CompareMark::Same),
            ]),
            right: CompareMarks::from([
                (name("gone"), CompareMark::OnlyHere),
                (name("newer"), CompareMark::OlderHere),
                (name("older"), CompareMark::NewerHere),
                (name("differs"), CompareMark::Different),
                (name("same"), CompareMark::Same),
            ]),
            left_rows: ["only", "newer", "older", "differs", "same"]
                .map(|text| entry(text, false))
                .to_vec(),
            right_rows: ["gone", "newer", "older", "differs", "same"]
                .map(|text| entry(text, false))
                .to_vec(),
            ..Compared::default()
        }
    }

    #[test]
    fn a_sync_copies_what_is_only_newer_or_different_on_the_source() {
        let plan = sync_plan(&compared(), Direction::ToRight, false);
        let names: Vec<_> = plan.copies.iter().map(|row| row.name.clone()).collect();
        assert_eq!(names, ["only", "newer", "differs"].map(OsString::from));
        assert!(plan.deletions.is_empty());
    }

    #[test]
    fn a_sync_to_the_left_reads_the_right_side() {
        let plan = sync_plan(&compared(), Direction::ToLeft, true);
        let copies: Vec<_> = plan.copies.iter().map(|row| row.name.clone()).collect();
        assert_eq!(copies, ["gone", "older", "differs"].map(OsString::from));
        let deletions: Vec<_> = plan.deletions.iter().map(|row| row.name.clone()).collect();
        assert_eq!(deletions, [OsString::from("only")]);
    }

    #[test]
    fn what_is_only_on_the_target_is_deleted_only_when_asked() {
        let kept = sync_plan(&compared(), Direction::ToRight, false);
        assert!(kept.deletions.is_empty());
        let mirrored = sync_plan(&compared(), Direction::ToRight, true);
        let gone: Vec<_> = mirrored
            .deletions
            .iter()
            .map(|row| row.name.clone())
            .collect();
        assert_eq!(gone, [OsString::from("gone")]);
    }

    #[test]
    fn the_tally_counts_the_marks() {
        let tally = compared().tally();
        assert_eq!(
            tally,
            Tally {
                only_left: 1,
                newer_left: 1,
                only_right: 1,
                newer_right: 1,
                different: 1,
                same: 1,
            }
        );
    }
}
