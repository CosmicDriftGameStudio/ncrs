//! Copies of archive entries for F3. A viewer is a program of its own and
//! cannot read inside an archive, so the entry is unpacked into the cache
//! folder and the viewer gets that copy.
//!
//! Every running ncrs owns one `run-*` folder in the cache root and keeps a
//! lock on the `.lock` file in it. The next start removes the `run-*` folders
//! whose lock nobody holds any more: those are what a crash or a kill left.

use std::collections::HashMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::copy_out;
use crate::fs::transfer::{OnConflict, Progress};

const INSTANCE_PREFIX: &str = "run-";
const LOCK_FILE: &str = ".lock";

/// A folder this young may belong to a start that has not locked it yet.
const SWEEP_MIN_AGE: Duration = Duration::from_secs(10 * 60);

/// The folders holding the copies of one running ncrs, one slot per unpacked
/// file, grouped by the archive the file came from.
#[derive(Debug)]
pub struct ViewCopies {
    instance: PathBuf,
    /// Held for as long as the instance lives; the lock is what tells the next
    /// start that the folder is in use.
    lock: Option<File>,
    slots: HashMap<PathBuf, Vec<PathBuf>>,
    /// Slots that could not be removed, a viewer on Windows still has the file
    /// open. Tried again on the next release and on close.
    pending: Vec<PathBuf>,
}

impl ViewCopies {
    /// Creates `root` if needed, removes what earlier runs left in it and
    /// makes the folder for this run.
    pub fn open(root: &Path) -> io::Result<Self> {
        Self::open_sweeping(root, SWEEP_MIN_AGE)
    }

    fn open_sweeping(root: &Path, min_age: Duration) -> io::Result<Self> {
        std::fs::create_dir_all(root)?;
        // Best effort: the root may sit on a volume without permissions.
        drop(restrict_to_owner(root, 0o700));
        sweep(root, min_age);
        let instance = private_dir(INSTANCE_PREFIX, root)?;
        let lock = lock_file(&instance).inspect_err(|_| {
            let _best_effort = std::fs::remove_dir_all(&instance);
        })?;
        Ok(Self {
            instance,
            lock: Some(lock),
            slots: HashMap::new(),
            pending: Vec::new(),
        })
    }

    /// A new, empty folder for one copy of an entry of `archive`.
    pub fn slot(&mut self, archive: &Path) -> io::Result<PathBuf> {
        let slot = private_dir("slot-", &self.instance)?;
        self.slots
            .entry(archive.to_path_buf())
            .or_default()
            .push(slot.clone());
        Ok(slot)
    }

    /// Removes every copy that came out of `archive`.
    pub fn release(&mut self, archive: &Path) {
        let retry = std::mem::take(&mut self.pending);
        let released = self.slots.remove(archive).unwrap_or_default();
        for slot in retry.into_iter().chain(released) {
            self.remove(slot);
        }
    }

    /// Stops tracking `slot` without touching the disk, for a slot whose
    /// owner removes it itself.
    pub fn forget_slot(&mut self, slot: &Path) {
        self.slots.retain(|_, slots| {
            slots.retain(|known| known != slot);
            !slots.is_empty()
        });
    }

    /// Stops tracking `slot` and removes it.
    pub fn discard(&mut self, slot: &Path) {
        self.forget_slot(slot);
        self.remove(slot.to_path_buf());
    }

    /// Removes everything this run made and gives up the lock. Safe to call
    /// again.
    pub fn close(&mut self) {
        let slots = std::mem::take(&mut self.slots);
        let pending = std::mem::take(&mut self.pending);
        for slot in pending.into_iter().chain(slots.into_values().flatten()) {
            self.remove(slot);
        }
        if let Some(lock) = self.lock.take() {
            let _best_effort = lock.unlock();
        }
        let _best_effort = std::fs::remove_dir_all(&self.instance);
    }

    fn remove(&mut self, slot: PathBuf) {
        match std::fs::remove_dir_all(&slot) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(_) => self.pending.push(slot),
        }
    }
}

impl Drop for ViewCopies {
    fn drop(&mut self) {
        self.close();
    }
}

/// A new folder with a random name in `parent`, for its owner only.
fn private_dir(prefix: &str, parent: &Path) -> io::Result<PathBuf> {
    let dir = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)?
        .keep();
    restrict_to_owner(&dir, 0o700).inspect_err(|_| {
        let _best_effort = std::fs::remove_dir_all(&dir);
    })?;
    Ok(dir)
}

fn lock_file(instance: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(instance.join(LOCK_FILE))?;
    file.try_lock().map_err(|failure| match failure {
        TryLockError::Error(err) => err,
        TryLockError::WouldBlock => io::Error::other("the lock is held"),
    })?;
    Ok(file)
}

/// Removes the `run-*` folders in `root` that are old enough and whose lock is
/// free. Best effort: what cannot be judged or removed stays.
fn sweep(root: &Path, min_age: Duration) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_instance = entry
            .file_name()
            .to_string_lossy()
            .starts_with(INSTANCE_PREFIX);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if is_instance && meta.is_dir() && is_old(&meta, min_age) && is_unlocked(&path) {
            let _best_effort = std::fs::remove_dir_all(&path);
        }
    }
}

fn is_old(meta: &std::fs::Metadata, min_age: Duration) -> bool {
    meta.modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= min_age)
}

/// Whether no ncrs holds `instance`. A missing lock file means nobody does;
/// anything else that goes wrong means the folder is left alone.
fn is_unlocked(instance: &Path) -> bool {
    match File::open(instance.join(LOCK_FILE)) {
        Ok(file) => match file.try_lock() {
            Ok(()) => {
                let _best_effort = file.unlock();
                true
            }
            Err(_) => false,
        },
        Err(err) => err.kind() == io::ErrorKind::NotFound,
    }
}

#[cfg(unix)]
pub fn restrict_to_owner(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
pub fn restrict_to_owner(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

/// Unpacks the archive entry `source` into `slot` and returns the path of the
/// copy, readable by its owner only: a file that ran in the archive must not
/// run from here. Blocking.
///
/// Whatever goes wrong, `slot` is gone when this returns an error; the thread
/// that unpacks owns it, so a cancel cannot race with a removal elsewhere.
pub fn extract_for_view<P: Progress>(
    source: &Path,
    slot: &Path,
    progress: &mut P,
) -> io::Result<PathBuf> {
    let result = unpack_for_view(source, slot, progress);
    if result.is_err() {
        let _best_effort = std::fs::remove_dir_all(slot);
    }
    result
}

fn unpack_for_view<P: Progress>(
    source: &Path,
    slot: &Path,
    progress: &mut P,
) -> io::Result<PathBuf> {
    let name = source
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the entry has no name"))?;
    copy_out(source, slot, progress, OnConflict::Fail)?;
    let copy = slot.join(name);
    restrict_to_owner(&copy, 0o600)?;
    // A cancel that came while the last block was written ends here.
    progress.keep_going()?;
    Ok(copy)
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::super::fixtures::{scratch, write_archive, Item, ALL_FORMATS};
    use super::*;
    use crate::fs::transfer::Counting;

    const ANY_AGE: Duration = Duration::ZERO;
    const NEVER_OLD: Duration = Duration::from_secs(3600);

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn left_by_a_dead_run(root: &Path, name: &str, with_lock_file: bool) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(dir.join("slot-x")).unwrap();
        std::fs::write(dir.join("slot-x").join("a.txt"), "old").unwrap();
        if with_lock_file {
            std::fs::write(dir.join(LOCK_FILE), "").unwrap();
        }
        dir
    }

    #[test]
    fn the_sweep_removes_what_a_dead_run_left() {
        let scratch = scratch("view-sweep");
        let root = scratch.path().join("view");
        std::fs::create_dir(&root).unwrap();
        let with_lock_file = left_by_a_dead_run(&root, "run-dead", true);
        let no_lock_file = left_by_a_dead_run(&root, "run-nolock", false);
        let foreign = root.join("other");
        std::fs::create_dir(&foreign).unwrap();

        let copies = ViewCopies::open_sweeping(&root, ANY_AGE).unwrap();

        assert!(!with_lock_file.exists());
        assert!(!no_lock_file.exists());
        assert!(foreign.exists(), "only run-* folders are the sweep's");
        assert!(copies.instance.exists());
    }

    #[test]
    fn the_sweep_leaves_a_run_that_holds_its_lock() {
        let scratch = scratch("view-sweep-locked");
        let root = scratch.path().join("view");
        let running = ViewCopies::open(&root).unwrap();

        let _second = ViewCopies::open_sweeping(&root, ANY_AGE).unwrap();

        assert!(running.instance.exists());
        assert!(running.instance.join(LOCK_FILE).exists());
    }

    #[test]
    fn the_sweep_leaves_a_folder_that_is_too_young() {
        let scratch = scratch("view-sweep-young");
        let root = scratch.path().join("view");
        std::fs::create_dir(&root).unwrap();
        let young = left_by_a_dead_run(&root, "run-young", true);

        let _copies = ViewCopies::open_sweeping(&root, NEVER_OLD).unwrap();

        assert!(young.join("slot-x").join("a.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_sweep_judges_by_the_age_of_the_folder() {
        let scratch = scratch("view-sweep-age");
        let root = scratch.path().join("view");
        std::fs::create_dir(&root).unwrap();
        let old = left_by_a_dead_run(&root, "run-old", true);
        let young = left_by_a_dead_run(&root, "run-young", true);
        let long_ago = SystemTime::now() - 2 * SWEEP_MIN_AGE;
        File::open(&old).unwrap().set_modified(long_ago).unwrap();

        let _copies = ViewCopies::open(&root).unwrap();

        assert!(!old.exists());
        assert!(young.exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_sweep_does_not_follow_a_link() {
        let scratch = scratch("view-sweep-link");
        let root = scratch.path().join("view");
        std::fs::create_dir(&root).unwrap();
        let outside = left_by_a_dead_run(scratch.path(), "outside", true);
        std::os::unix::fs::symlink(&outside, root.join("run-link")).unwrap();

        let _copies = ViewCopies::open_sweeping(&root, ANY_AGE).unwrap();

        assert!(outside.join("slot-x").join("a.txt").exists());
    }

    #[test]
    fn releasing_an_archive_empties_its_slots_only() {
        let scratch = scratch("view-release");
        let mut copies = ViewCopies::open(&scratch.path().join("view")).unwrap();
        let first = Path::new("first.zip");
        let second = Path::new("second.zip");
        let one = copies.slot(first).unwrap();
        let two = copies.slot(first).unwrap();
        let other = copies.slot(second).unwrap();
        std::fs::write(one.join("a.txt"), "a").unwrap();

        copies.release(first);

        assert!(!one.exists());
        assert!(!two.exists());
        assert!(other.exists());
        assert!(copies.instance.exists());
    }

    #[test]
    fn a_forgotten_slot_stays_on_disk_and_a_discarded_one_does_not() {
        let scratch = scratch("view-forget");
        let mut copies = ViewCopies::open(&scratch.path().join("view")).unwrap();
        let archive = Path::new("a.zip");
        let forgotten = copies.slot(archive).unwrap();
        let discarded = copies.slot(archive).unwrap();

        copies.forget_slot(&forgotten);
        copies.discard(&discarded);
        copies.release(archive);

        assert!(forgotten.exists(), "release must not know a forgotten slot");
        assert!(!discarded.exists());
    }

    #[test]
    fn closing_leaves_no_folder_of_this_run_and_may_repeat() {
        let scratch = scratch("view-close");
        let root = scratch.path().join("view");
        let mut copies = ViewCopies::open(&root).unwrap();
        let slot = copies.slot(Path::new("a.zip")).unwrap();
        std::fs::write(slot.join("a.txt"), "a").unwrap();
        assert_eq!(names_in(&root).len(), 1);

        copies.close();
        copies.close();

        assert!(names_in(&root).is_empty(), "{:?}", names_in(&root));
    }

    #[test]
    fn dropping_cleans_up_like_closing() {
        let scratch = scratch("view-drop");
        let root = scratch.path().join("view");
        let mut copies = ViewCopies::open(&root).unwrap();
        copies.slot(Path::new("a.zip")).unwrap();

        drop(copies);

        assert!(names_in(&root).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_root_and_the_run_folder_are_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let scratch = scratch("view-private");
        let root = scratch.path().join("view");
        let mut copies = ViewCopies::open(&root).unwrap();
        let slot = copies.slot(Path::new("a.zip")).unwrap();

        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&root), 0o700);
        assert_eq!(mode(&copies.instance), 0o700);
        assert_eq!(mode(&slot), 0o700);
    }

    #[test]
    fn a_file_is_unpacked_to_a_copy_with_its_content_in_every_format() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("view-extract");
            let archive = dir.path().join(file);
            write_archive(&archive, &[Item::File("docs/a.txt", b"inside")]);
            let mut copies = ViewCopies::open(&dir.path().join("view")).unwrap();
            let slot = copies.slot(&archive).unwrap();

            let copy = extract_for_view(&archive.join("docs").join("a.txt"), &slot, &mut Counting)
                .unwrap();

            assert_eq!(copy, slot.join("a.txt"), "{file}");
            assert_eq!(std::fs::read(&copy).unwrap(), b"inside", "{file}");
            assert_eq!(names_in(&slot), ["a.txt"], "{file}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_copy_is_private_even_when_the_file_ran_in_the_archive() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = scratch("view-mode");
        let archive = dir.path().join("m.tar");
        let mut builder = tar::Builder::new(File::create(&archive).unwrap());
        let mut header = tar::Header::new_gnu();
        header.set_size(9);
        header.set_mode(0o755);
        builder
            .append_data(&mut header, "run.sh", &b"#!/bin/sh"[..])
            .unwrap();
        builder.finish().unwrap();
        let mut copies = ViewCopies::open(&dir.path().join("view")).unwrap();
        let slot = copies.slot(&archive).unwrap();

        let copy = extract_for_view(&archive.join("run.sh"), &slot, &mut Counting).unwrap();

        let mode = std::fs::metadata(&copy).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    struct Cancelled;

    impl Progress for Cancelled {
        fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
            Err(io::Error::other("cancelled"))
        }
    }

    struct CancelledAtTheEnd;

    impl Progress for CancelledAtTheEnd {
        fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
            Ok(())
        }

        fn keep_going(&mut self) -> io::Result<()> {
            Err(io::Error::other("cancelled"))
        }
    }

    #[test]
    fn a_cancel_removes_the_slot() {
        let dir = scratch("view-cancel");
        let archive = dir.path().join("c.zip");
        write_archive(&archive, &[Item::File("a.txt", b"x")]);
        let mut copies = ViewCopies::open(&dir.path().join("view")).unwrap();
        let slot = copies.slot(&archive).unwrap();

        let err = extract_for_view(&archive.join("a.txt"), &slot, &mut Cancelled).unwrap_err();

        assert_eq!(err.to_string(), "cancelled");
        assert!(!slot.exists());
    }

    #[test]
    fn a_cancel_after_the_last_block_removes_the_slot_too() {
        let dir = scratch("view-cancel-end");
        let archive = dir.path().join("c.tar");
        write_archive(&archive, &[Item::File("a.txt", b"x")]);
        let mut copies = ViewCopies::open(&dir.path().join("view")).unwrap();
        let slot = copies.slot(&archive).unwrap();

        extract_for_view(&archive.join("a.txt"), &slot, &mut CancelledAtTheEnd).unwrap_err();

        assert!(!slot.exists());
    }

    #[test]
    fn an_entry_that_is_not_in_the_archive_fails_and_removes_the_slot() {
        let dir = scratch("view-missing");
        let archive = dir.path().join("m.zip");
        write_archive(&archive, &[Item::File("a.txt", b"x")]);
        let mut copies = ViewCopies::open(&dir.path().join("view")).unwrap();
        let slot = copies.slot(&archive).unwrap();

        extract_for_view(&archive.join("nope.txt"), &slot, &mut Counting).unwrap_err();

        assert!(!slot.exists());
    }
}
