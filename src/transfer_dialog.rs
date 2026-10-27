//! What the copy and move dialog knows: the selection, what a background scan
//! found in it, how many names the target already has, and whether the target
//! may be written to at all.
//!
//! Everything that touches the disk is a free function taking plain paths, so
//! the app runs it on the blocking pool and the tests run it on a tempdir.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::fs::archive;
use crate::fs::transfer::OnConflict;
use crate::messages::TransferKind;

/// Up to this many entries the dialog names them all; above it, numbers.
pub const NAMES_DIRECTLY: usize = 20;

/// How many names stand in front of "and N more" in the summary.
pub const FIRST_NAMES: usize = 3;

/// The expanded list shows this many names at most: a text widget per name
/// makes a list of ten thousand unusable, and the scrollable would not help.
pub const LISTED_NAMES: usize = 300;

/// What to do when the target already has a name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Policy {
    /// Ask per file; the answer dialog offers "for all".
    #[default]
    Ask,
    Overwrite,
    Skip,
}

impl Policy {
    pub const ALL: [Self; 3] = [Self::Ask, Self::Overwrite, Self::Skip];

    /// What the filesystem layer does with the first row. `Ask` lets it fail
    /// on a taken name, which is what opens the question.
    pub const fn on_conflict(self) -> OnConflict {
        match self {
            Self::Ask => OnConflict::Fail,
            Self::Overwrite => OnConflict::Overwrite,
            Self::Skip => OnConflict::Skip,
        }
    }

    /// The answer that holds for every later row, if the user already gave it.
    pub const fn standing_rule(self) -> Option<OnConflict> {
        match self {
            Self::Ask => None,
            Self::Overwrite => Some(OnConflict::Overwrite),
            Self::Skip => Some(OnConflict::Skip),
        }
    }
}

/// What the dialog and its background work send the app.
#[derive(Debug, Clone, PartialEq)]
pub enum TransferDialogMessage {
    /// The scan of the selection is done. `None`: the thread died.
    Scanned {
        dialog: u64,
        scan: Option<Scan>,
    },
    /// The names the target already has, for the target as it was typed at
    /// edit number `edit`; a count for an older edit is dropped.
    ConflictsCounted {
        dialog: u64,
        edit: u64,
        count: usize,
    },
    TargetInput(String),
    SetKind(TransferKind),
    /// Tab: the other one of copy and move.
    ToggleKind,
    SetPolicy(Policy),
    ToggleList,
    Confirm,
    Cancel,
    /// Closes the progress mask; the job goes on.
    Background,
    /// Opens the progress mask again, from the status bar.
    ShowProgress,
}

/// What a walk through the selection counted. Links are never followed: one
/// counts as a file of the size of the link itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scan {
    pub files: usize,
    pub folders: usize,
    pub bytes: u64,
    /// Entries that could not be read or listed; they are left out of the
    /// other numbers.
    pub unreadable: usize,
}

impl Scan {
    /// An entry that is on no disk: counted from the archive's index, or
    /// unreadable when it is not in one either.
    fn count_archive_entry(&mut self, path: &Path) {
        match archive::measure(path) {
            Some(inside) => {
                self.files += inside.files;
                self.folders += inside.folders;
                self.bytes += inside.bytes;
            }
            None => self.unreadable += 1,
        }
    }
}

/// Walks `sources` without following links, until done or `cancel` is set.
/// Blocking: the app calls it on the blocking pool.
pub fn scan(sources: &[PathBuf], cancel: &AtomicBool) -> Scan {
    let mut found = Scan::default();
    let mut pending: Vec<PathBuf> = sources.to_vec();
    while let Some(path) = pending.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            found.count_archive_entry(&path);
            continue;
        };
        if !meta.is_dir() {
            found.files += 1;
            found.bytes += meta.len();
            continue;
        }
        found.folders += 1;
        match std::fs::read_dir(&path) {
            Ok(children) => {
                for listed in children {
                    match listed {
                        Ok(child) => pending.push(child.path()),
                        Err(_) => found.unreadable += 1,
                    }
                }
            }
            Err(_) => found.unreadable += 1,
        }
    }
    found
}

/// How many top-level names of `sources` the folder `target` already has.
/// Looked up without following links, so a dangling link counts as taken:
/// the copy would run into it too.
pub fn count_conflicts(sources: &[PathBuf], target: &Path, cancel: &AtomicBool) -> usize {
    sources
        .iter()
        .take_while(|_| !cancel.load(Ordering::Relaxed))
        .filter_map(|source| source.file_name())
        .filter(|name| std::fs::symlink_metadata(target.join(name)).is_ok())
        .count()
}

/// Why a target is refused before anything is queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Missing,
    NotAFolder,
    /// The target is the folder a selected entry is in.
    SameFolder,
    /// The target is a selected folder or lies below one.
    InsideSource,
    /// The system would not say (permission, a mount that does not answer).
    Unreadable,
    /// The target is an archive or lies inside one; archives are read only.
    IntoArchive,
    /// A move was asked for entries that lie inside an archive.
    MoveFromArchive,
}

fn refusal_of(error: &io::Error) -> Refusal {
    if error.kind() == io::ErrorKind::NotFound {
        Refusal::Missing
    } else {
        Refusal::Unreadable
    }
}

/// Checks `target` against the selection and returns the resolved folder to
/// transfer into.
///
/// Both sides are resolved through links first (`canonicalize`), so a link
/// into the source or a `..` in the typed text cannot get past the check the
/// way it would past a comparison of the text. The last component of a source
/// is not resolved: copying a link copies the link, so the source *is* the
/// link, not what it points to.
pub fn validate(
    kind: TransferKind,
    sources: &[PathBuf],
    target: &Path,
) -> Result<PathBuf, Refusal> {
    let resolved = match std::fs::canonicalize(target) {
        Ok(resolved) => resolved,
        Err(_) if archive::locate_in_archive(target).is_some() => return Err(Refusal::IntoArchive),
        Err(error) => return Err(refusal_of(&error)),
    };
    if archive::is_archive_file(&resolved) {
        return Err(Refusal::IntoArchive);
    }
    if !resolved.is_dir() {
        return Err(Refusal::NotAFolder);
    }
    for source in sources {
        // An entry of an archive is on no disk: it has no folder to be the same
        // as and nothing the target could lie inside.
        if lies_in_archive(source) {
            if kind == TransferKind::Move {
                return Err(Refusal::MoveFromArchive);
            }
            continue;
        }
        let (Some(parent), Some(name)) = (source.parent(), source.file_name()) else {
            continue;
        };
        let resolved_parent = std::fs::canonicalize(parent).map_err(|error| refusal_of(&error))?;
        if resolved_parent == resolved {
            return Err(Refusal::SameFolder);
        }
        if resolved.starts_with(resolved_parent.join(name)) {
            return Err(Refusal::InsideSource);
        }
    }
    Ok(resolved)
}

fn lies_in_archive(source: &Path) -> bool {
    std::fs::symlink_metadata(source).is_err() && archive::locate_in_archive(source).is_some()
}

/// The open confirmation. The selection is resolved once, when the key is
/// pressed: by the time the user confirms, the cursor may have moved.
#[derive(Debug)]
pub struct TransferDialog {
    id: u64,
    kind: TransferKind,
    sources: Vec<PathBuf>,
    /// What a relative target is relative to: the other panel's folder.
    base: PathBuf,
    target: String,
    policy: Policy,
    scan: Option<Scan>,
    conflicts: Option<usize>,
    edit: u64,
    list_open: bool,
    refusal: Option<Refusal>,
    /// Set when the dialog goes away, which stops the scan and the count.
    stop_background: Arc<AtomicBool>,
}

impl Drop for TransferDialog {
    fn drop(&mut self) {
        self.stop_background.store(true, Ordering::Relaxed);
    }
}

impl TransferDialog {
    pub fn new(id: u64, kind: TransferKind, sources: Vec<PathBuf>, base: PathBuf) -> Self {
        let target = base.display().to_string();
        Self {
            id,
            kind,
            sources,
            base,
            target,
            policy: Policy::default(),
            scan: None,
            conflicts: None,
            edit: 0,
            list_open: false,
            refusal: None,
            stop_background: Arc::new(AtomicBool::new(false)),
        }
    }

    pub const fn id(&self) -> u64 {
        self.id
    }

    pub const fn kind(&self) -> TransferKind {
        self.kind
    }

    pub fn set_kind(&mut self, kind: TransferKind) {
        self.kind = kind;
    }

    pub fn sources(&self) -> &[PathBuf] {
        &self.sources
    }

    pub fn target_text(&self) -> &str {
        &self.target
    }

    /// The typed target as a path: a relative one means "below the other
    /// panel's folder".
    pub fn target_path(&self) -> PathBuf {
        self.base.join(&self.target)
    }

    pub const fn policy(&self) -> Policy {
        self.policy
    }

    pub fn set_policy(&mut self, policy: Policy) {
        self.policy = policy;
    }

    pub const fn scan(&self) -> Option<Scan> {
        self.scan
    }

    pub fn set_scan(&mut self, scan: Option<Scan>) {
        self.scan = scan;
    }

    pub const fn conflicts(&self) -> Option<usize> {
        self.conflicts
    }

    pub const fn edit(&self) -> u64 {
        self.edit
    }

    pub fn set_conflicts(&mut self, edit: u64, count: usize) {
        if edit == self.edit {
            self.conflicts = Some(count);
        }
    }

    pub const fn list_open(&self) -> bool {
        self.list_open
    }

    pub fn toggle_list(&mut self) {
        self.list_open = !self.list_open;
    }

    pub const fn refusal(&self) -> Option<Refusal> {
        self.refusal
    }

    pub fn refuse(&mut self, refusal: Refusal) {
        self.refusal = Some(refusal);
    }

    /// Typing changes the target: what was counted for the old text and the
    /// refusal of the old text no longer hold.
    pub fn type_target(&mut self, text: String) {
        self.target = text;
        self.edit = self.edit.wrapping_add(1);
        self.conflicts = None;
        self.refusal = None;
    }

    /// The flag the background work polls.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop_background)
    }

    /// Whether the names are listed one by one rather than summed up.
    pub fn names_directly(&self) -> bool {
        self.sources.len() <= NAMES_DIRECTLY
    }

    /// The top-level names, as they are shown.
    pub fn names(&self) -> impl Iterator<Item = String> + '_ {
        self.sources
            .iter()
            .filter_map(|source| source.file_name())
            .map(|name| name.to_string_lossy().into_owned())
    }
}

// reason: a failing assertion is the signal in a test, so unwrap belongs here
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn never() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn write(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    #[test]
    fn the_scan_counts_files_folders_and_bytes() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("tree/a.txt"), 10);
        write(&root.path().join("tree/deep/b.txt"), 5);
        write(&root.path().join("loose.txt"), 1);
        let sources = vec![root.path().join("tree"), root.path().join("loose.txt")];

        let found = scan(&sources, &never());

        assert_eq!(
            found,
            Scan {
                files: 3,
                folders: 2,
                bytes: 16,
                unreadable: 0
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_scan_does_not_follow_a_link_and_counts_it_as_one_file() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("elsewhere/big.bin"), 1000);
        write(&root.path().join("tree/small.txt"), 1);
        std::os::unix::fs::symlink(root.path().join("elsewhere"), root.path().join("tree/link"))
            .unwrap();
        let link_size = std::fs::symlink_metadata(root.path().join("tree/link"))
            .unwrap()
            .len();

        let found = scan(&[root.path().join("tree")], &never());

        assert_eq!(found.files, 2, "the small file and the link itself");
        assert_eq!(found.folders, 1, "the folder behind the link was entered");
        assert_eq!(found.bytes, 1 + link_size);
    }

    #[cfg(unix)]
    #[test]
    fn the_scan_counts_what_it_cannot_read() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("tree/sealed/inside.txt"), 3);
        let sealed = root.path().join("tree/sealed");
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Whoever may read it anyway (root) cannot show the case.
        let can_still_read = std::fs::read_dir(&sealed).is_ok();

        let found = scan(&[root.path().join("tree")], &never());
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o755)).unwrap();

        if !can_still_read {
            assert_eq!(found.unreadable, 1);
            assert_eq!(found.folders, 2, "the sealed folder itself still counts");
            assert_eq!(found.files, 0);
        }
    }

    #[test]
    fn a_vanished_source_is_unreadable_not_a_panic() {
        let root = tempfile::tempdir().unwrap();

        let found = scan(&[root.path().join("gone")], &never());

        assert_eq!(found.unreadable, 1);
        assert_eq!(found.files + found.folders, 0);
    }

    #[test]
    fn a_stopped_scan_returns_what_it_has() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("a.txt"), 1);

        let found = scan(&[root.path().join("a.txt")], &AtomicBool::new(true));

        assert_eq!(found, Scan::default());
    }

    #[test]
    fn conflicts_are_the_top_level_names_the_target_has() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("from/a.txt"), 1);
        write(&root.path().join("from/b.txt"), 1);
        write(&root.path().join("from/dir/inner.txt"), 1);
        write(&root.path().join("to/a.txt"), 2);
        // A folder of the same name is in the way too, and what is inside the
        // source folder is nobody's business.
        write(&root.path().join("to/dir/other.txt"), 2);
        let sources = vec![
            root.path().join("from/a.txt"),
            root.path().join("from/b.txt"),
            root.path().join("from/dir"),
        ];

        let count = count_conflicts(&sources, &root.path().join("to"), &never());

        assert_eq!(count, 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_link_in_the_target_is_a_taken_name() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("from/a.txt"), 1);
        std::fs::create_dir_all(root.path().join("to")).unwrap();
        std::os::unix::fs::symlink(root.path().join("nowhere"), root.path().join("to/a.txt"))
            .unwrap();

        let count = count_conflicts(
            &[root.path().join("from/a.txt")],
            &root.path().join("to"),
            &never(),
        );

        assert_eq!(count, 1);
    }

    #[test]
    fn a_missing_target_is_refused() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("from/a.txt"), 1);

        let result = validate(
            TransferKind::Copy,
            &[root.path().join("from/a.txt")],
            &root.path().join("nope"),
        );

        assert_eq!(result, Err(Refusal::Missing));
    }

    #[test]
    fn a_file_is_not_a_target() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("from/a.txt"), 1);
        write(&root.path().join("file.txt"), 1);

        let result = validate(
            TransferKind::Copy,
            &[root.path().join("from/a.txt")],
            &root.path().join("file.txt"),
        );

        assert_eq!(result, Err(Refusal::NotAFolder));
    }

    #[test]
    fn the_folder_of_the_source_is_refused() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("from/a.txt"), 1);

        let result = validate(
            TransferKind::Copy,
            &[root.path().join("from/a.txt")],
            &root.path().join("from"),
        );

        assert_eq!(result, Err(Refusal::SameFolder));
    }

    #[test]
    fn a_folder_cannot_go_into_itself_or_below_itself() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("from/tree/deep/x.txt"), 1);
        let sources = vec![root.path().join("from/tree")];

        assert_eq!(
            validate(TransferKind::Copy, &sources, &root.path().join("from/tree")),
            Err(Refusal::InsideSource)
        );
        assert_eq!(
            validate(
                TransferKind::Copy,
                &sources,
                &root.path().join("from/tree/deep")
            ),
            Err(Refusal::InsideSource)
        );
    }

    #[test]
    fn dots_in_the_typed_target_do_not_get_past_the_check() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("from/tree/deep/x.txt"), 1);
        std::fs::create_dir_all(root.path().join("to")).unwrap();
        let sneaky = root.path().join("to/../from/tree/deep");

        let result = validate(
            TransferKind::Copy,
            &[root.path().join("from/tree")],
            &sneaky,
        );

        assert_eq!(result, Err(Refusal::InsideSource));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_into_the_source_does_not_get_past_the_check() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("from/tree/deep/x.txt"), 1);
        std::fs::create_dir_all(root.path().join("to")).unwrap();
        std::os::unix::fs::symlink(
            root.path().join("from/tree/deep"),
            root.path().join("to/shortcut"),
        )
        .unwrap();

        let result = validate(
            TransferKind::Copy,
            &[root.path().join("from/tree")],
            &root.path().join("to/shortcut"),
        );

        assert_eq!(result, Err(Refusal::InsideSource));
    }

    #[cfg(unix)]
    #[test]
    fn a_selected_link_is_the_link_not_what_it_points_to() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("from/real/x.txt"), 1);
        std::os::unix::fs::symlink(root.path().join("from/real"), root.path().join("from/link"))
            .unwrap();

        let result = validate(
            TransferKind::Copy,
            &[root.path().join("from/link")],
            &root.path().join("from/real"),
        );

        assert_eq!(
            result,
            Ok(std::fs::canonicalize(root.path().join("from/real")).unwrap()),
            "copying a link copies the link, so its target folder is a fine place"
        );
    }

    #[test]
    fn a_sibling_folder_is_accepted_and_resolved() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("from/tree/x.txt"), 1);
        std::fs::create_dir_all(root.path().join("to")).unwrap();

        let result = validate(
            TransferKind::Copy,
            &[root.path().join("from/tree")],
            &root.path().join("to/../to"),
        );

        assert_eq!(
            result,
            Ok(std::fs::canonicalize(root.path().join("to")).unwrap())
        );
    }

    fn zip_in(root: &Path) -> PathBuf {
        use crate::fs::archive::fixtures::{write_archive, Item};
        let archive = root.join("box.zip");
        write_archive(
            &archive,
            &[Item::File("in/a.txt", b"abc"), Item::File("b.txt", b"12")],
        );
        archive
    }

    #[test]
    fn an_archive_or_a_place_in_one_is_no_target() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("from/a.txt"), 1);
        let archive = zip_in(root.path());
        let sources = [root.path().join("from/a.txt")];

        for target in [archive.clone(), archive.join("in"), archive.join("gone")] {
            for kind in [TransferKind::Copy, TransferKind::Move] {
                assert_eq!(
                    validate(kind, &sources, &target),
                    Err(Refusal::IntoArchive),
                    "{kind:?} {target:?}"
                );
            }
        }
    }

    #[test]
    fn entries_of_an_archive_can_be_copied_but_not_moved() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("to")).unwrap();
        let archive = zip_in(root.path());
        let sources = [archive.join("in"), archive.join("b.txt")];

        assert_eq!(
            validate(TransferKind::Copy, &sources, &root.path().join("to")),
            Ok(std::fs::canonicalize(root.path().join("to")).unwrap())
        );
        assert_eq!(
            validate(TransferKind::Copy, &sources, root.path()),
            Ok(std::fs::canonicalize(root.path()).unwrap()),
            "the folder holding the archive is a fine place to copy out to"
        );
        assert_eq!(
            validate(TransferKind::Move, &sources, &root.path().join("to")),
            Err(Refusal::MoveFromArchive)
        );
    }

    #[test]
    fn the_scan_counts_what_is_inside_an_archive_source() {
        let root = tempfile::tempdir().unwrap();
        let archive = zip_in(root.path());

        let found = scan(&[archive.join("in"), archive.join("b.txt")], &never());

        assert_eq!(
            found,
            Scan {
                files: 2,
                folders: 1,
                bytes: 5,
                unreadable: 0
            }
        );
        assert_eq!(scan(&[archive.join("gone")], &never()).unreadable, 1);
    }

    #[test]
    fn a_relative_target_is_below_the_other_panel() {
        let dialog = {
            let mut dialog = TransferDialog::new(
                1,
                TransferKind::Copy,
                vec![PathBuf::from("/a/b")],
                PathBuf::from("/other"),
            );
            dialog.type_target("sub".to_string());
            dialog
        };

        assert_eq!(dialog.target_path(), PathBuf::from("/other/sub"));
    }

    #[test]
    fn typing_forgets_the_count_and_the_refusal_and_drops_late_counts() {
        let mut dialog = TransferDialog::new(
            1,
            TransferKind::Copy,
            vec![PathBuf::from("/a/b")],
            PathBuf::from("/other"),
        );
        let stale = dialog.edit();
        dialog.set_conflicts(stale, 4);
        dialog.refuse(Refusal::Missing);

        dialog.type_target("/elsewhere".to_string());
        dialog.set_conflicts(stale, 9);

        assert_eq!(dialog.conflicts(), None);
        assert_eq!(dialog.refusal(), None);
    }

    #[test]
    fn dropping_the_dialog_stops_the_background_work() {
        let dialog = TransferDialog::new(
            1,
            TransferKind::Copy,
            vec![PathBuf::from("/a/b")],
            PathBuf::from("/other"),
        );
        let flag = dialog.stop_flag();

        drop(dialog);

        assert!(flag.load(Ordering::Relaxed));
    }
}
