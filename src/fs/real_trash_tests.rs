//! The production trash against the real one of this machine: the Trash on
//! macOS, the Recycle Bin on Windows, the freedesktop trash on Linux.
//!
//! macOS and Windows have no way to point the system at another trash, so
//! those tests use the user's own and the cleanup guard removes exactly what a
//! test put there. Linux re-runs each test in a child process whose
//! `XDG_DATA_HOME` is inside the test folder, so the trash is a private one.

// reason: a failing assertion is the signal in a test
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{system_trash, RestoreError, Trash, TrashedItem};

#[cfg(target_os = "linux")]
const ROOT_VARIABLE: &str = "NCRS_REAL_TRASH_ROOT";

const WORK_FOLDER: &str = "work";

/// What the trash lists of an item can differ in from the path it was trashed
/// from (`/private/var` against `/var`, short Windows names), so only the part
/// behind the unique folder name is compared. `None` when `marker` is not one
/// of the components.
fn path_behind_marker(original: &Path, marker: &OsStr) -> Option<PathBuf> {
    let wanted = marker.to_string_lossy();
    let mut components = original.components();
    components.find(|part| {
        part.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&wanted)
    })?;
    Some(components.as_path().to_path_buf())
}

fn is_ours(original: &Path, marker: &OsStr, relative: &Path) -> bool {
    path_behind_marker(original, marker)
        .is_some_and(|behind| behind == Path::new(WORK_FOLDER).join(relative))
}

fn is_from_this_test(original: &Path, marker: &OsStr) -> bool {
    path_behind_marker(original, marker).is_some_and(|behind| behind.starts_with(WORK_FOLDER))
}

/// Takes everything this test put into the trash out again, also when an
/// assertion panicked. Never panics itself.
struct TrashCleanup {
    // reason: only the macOS cleanup reads the trash through ncrs
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    trash: Arc<dyn Trash>,
    marker: std::ffi::OsString,
}

impl TrashCleanup {
    #[cfg(not(target_os = "macos"))]
    fn purge(&self) {
        let items = match trash::os_limited::list() {
            Ok(items) => items,
            Err(err) => {
                eprintln!("cleanup: the trash cannot be listed: {err}");
                return;
            }
        };
        let ours: Vec<_> = items
            .into_iter()
            .filter(|item| is_from_this_test(&item.original_path(), &self.marker))
            .collect();
        if let Err(err) = trash::os_limited::purge_all(ours) {
            eprintln!("cleanup: the trash cannot be emptied of the test items: {err}");
        }
    }

    /// Listing `~/.Trash` can be denied to the app, but the journal knows the
    /// path of each item, and stat and remove work on a known path.
    #[cfg(target_os = "macos")]
    fn purge(&self) {
        let items = match self.trash.list() {
            Ok(items) => items,
            Err(err) => {
                eprintln!("cleanup: the trash cannot be listed: {err}");
                return;
            }
        };
        for item in items
            .iter()
            .filter(|item| is_from_this_test(&item.original, &self.marker))
        {
            let trashed = Path::new(&item.key);
            let removed = match std::fs::symlink_metadata(trashed) {
                Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(trashed),
                Ok(_) => std::fs::remove_file(trashed),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err),
            };
            if let Err(err) = removed {
                eprintln!("cleanup: {} stays in the trash: {err}", trashed.display());
            }
        }
    }
}

impl Drop for TrashCleanup {
    fn drop(&mut self) {
        self.purge();
    }
}

pub struct Scene {
    work: PathBuf,
    marker: std::ffi::OsString,
    trash: Arc<dyn Trash>,
    journal: PathBuf,
    cleanup: TrashCleanup,
}

impl Scene {
    fn new(root: &Path) -> Self {
        let marker = root.file_name().unwrap().to_owned();
        let work = root.join(WORK_FOLDER);
        std::fs::create_dir_all(&work).unwrap();
        let journal = root.join("journal.toml");
        let trash = system_trash(Some(journal.clone()));
        let cleanup = TrashCleanup {
            trash: Arc::clone(&trash),
            marker: marker.clone(),
        };
        Self {
            work,
            marker,
            trash,
            journal,
            cleanup,
        }
    }

    /// The folder the scenario works in.
    pub fn work(&self) -> &Path {
        &self.work
    }

    pub fn trash(&self) -> Arc<dyn Trash> {
        Arc::clone(&self.trash)
    }

    pub fn write(&self, relative: &str, content: &str) -> PathBuf {
        let path = self.work.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    pub fn assert_gone(&self, path: &Path) {
        let found = std::fs::symlink_metadata(path);
        assert!(
            matches!(&found, Err(err) if err.kind() == std::io::ErrorKind::NotFound),
            "{} is still there: {found:?}",
            path.display()
        );
    }

    pub fn items_of_ours(&self, relative: &str) -> Vec<TrashedItem> {
        self.trash
            .list()
            .unwrap()
            .into_iter()
            .filter(|item| is_ours(&item.original, &self.marker, Path::new(relative)))
            .collect()
    }

    fn listed(&self, relative: &str) -> TrashedItem {
        // The isolation holds only if the item went to the trash under
        // XDG_DATA_HOME, not to a per-volume or the desktop's home trash.
        if cfg!(target_os = "linux") {
            let name = Path::new(relative).file_name().unwrap();
            let private = self
                .work
                .parent()
                .unwrap()
                .join("data/Trash/files")
                .join(name);
            assert!(
                std::fs::symlink_metadata(&private).is_ok(),
                "{relative} did not land in the private trash {}",
                private.display()
            );
        }
        let mut items = self.items_of_ours(relative);
        if items.len() != 1 {
            let listed = self.trash.list().unwrap();
            let of_test_runs: Vec<_> = listed
                .iter()
                .map(|item| item.original.display().to_string())
                .filter(|original| original.to_lowercase().contains("ncrs-real-trash"))
                .collect();
            panic!(
                "{relative} is listed {} times; the trash lists {} items, of test runs: \
                 {of_test_runs:?}; trashed from {}",
                items.len(),
                listed.len(),
                self.work.join(relative).display()
            );
        }
        items.remove(0)
    }

    fn journal_text(&self) -> String {
        std::fs::read_to_string(&self.journal).unwrap_or_default()
    }

    /// Ends a scenario: removes what is left in the trash and checks that
    /// nothing of ours remains.
    pub fn finish(&self) {
        self.cleanup.purge();
        let left: Vec<_> = self
            .trash
            .list()
            .unwrap()
            .into_iter()
            .filter(|item| is_from_this_test(&item.original, &self.marker))
            .collect();
        assert!(left.is_empty(), "left in the trash: {left:?}");
    }
}

/// `test_name` is the full path of the calling test, for the Linux re-run.
pub fn on_the_real_trash(test_name: &str, scenario: impl FnOnce(&Scene)) {
    #[cfg(target_os = "linux")]
    {
        match std::env::var_os(ROOT_VARIABLE) {
            Some(root) => scenario(&Scene::new(Path::new(&root))),
            None => run_in_private_trash(test_name),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = test_name;
        let root = new_root();
        scenario(&Scene::new(root.path()));
    }
}

fn new_root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("ncrs-real-trash-")
        .tempdir()
        .unwrap()
}

/// Runs this very test again with a trash folder of its own. The trash crate
/// reads `XDG_DATA_HOME` on every call, and setting it here would reach the
/// tests running beside this one.
#[cfg(target_os = "linux")]
fn run_in_private_trash(test_name: &str) {
    let root = new_root();
    let data = root.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test_name, "--exact", "--nocapture"])
        .env(ROOT_VARIABLE, root.path())
        .env("XDG_DATA_HOME", &data)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed"),
        "{test_name} failed in its private trash\n--- stdout\n{stdout}\n--- stderr\n{stderr}"
    );
}

#[test]
fn a_file_and_a_folder_come_back_whole() {
    on_the_real_trash(
        "fs::real_trash_tests::a_file_and_a_folder_come_back_whole",
        |scene| {
            let alpha = scene.write("alpha.txt", "alpha");
            let docs = scene.work.join("docs");
            scene.write("docs/a.txt", "a");
            scene.write("docs/sub/b.txt", "b");

            scene.trash.trash(&alpha).unwrap();
            scene.trash.trash(&docs).unwrap();
            scene.assert_gone(&alpha);
            scene.assert_gone(&docs);

            let alpha_item = scene.listed("alpha.txt");
            let docs_item = scene.listed("docs");
            if cfg!(target_os = "macos") {
                let journal = scene.journal_text();
                for item in [&alpha_item, &docs_item] {
                    let trashed = Path::new(&item.key);
                    assert!(
                        std::fs::symlink_metadata(trashed).is_ok(),
                        "{} is not in the trash",
                        trashed.display()
                    );
                    assert!(
                        journal.contains(&*trashed.to_string_lossy()),
                        "the journal does not mention {}",
                        trashed.display()
                    );
                }
            }

            assert_eq!(scene.trash.restore(&alpha_item), Ok(()));
            assert_eq!(scene.trash.restore(&docs_item), Ok(()));

            assert_eq!(std::fs::read_to_string(&alpha).unwrap(), "alpha");
            assert_eq!(std::fs::read_to_string(docs.join("a.txt")).unwrap(), "a");
            assert_eq!(
                std::fs::read_to_string(docs.join("sub/b.txt")).unwrap(),
                "b"
            );
            assert!(scene.items_of_ours("alpha.txt").is_empty());
            assert!(scene.items_of_ours("docs").is_empty());
            if cfg!(target_os = "macos") {
                let journal = scene.journal_text();
                for item in [&alpha_item, &docs_item] {
                    assert!(
                        !journal.contains(&*Path::new(&item.key).to_string_lossy()),
                        "the journal still mentions {:?}",
                        item.key
                    );
                }
            }
            scene.finish();
        },
    );
}

#[test]
fn a_missing_original_folder_is_recreated() {
    on_the_real_trash(
        "fs::real_trash_tests::a_missing_original_folder_is_recreated",
        |scene| {
            let inner = scene.write("gone/inner.txt", "inner");
            scene.trash.trash(&inner).unwrap();
            std::fs::remove_dir(scene.work.join("gone")).unwrap();

            let item = scene.listed("gone/inner.txt");
            assert_eq!(scene.trash.restore(&item), Ok(()));

            assert_eq!(std::fs::read_to_string(&inner).unwrap(), "inner");
            scene.finish();
        },
    );
}

#[test]
fn a_taken_name_is_reported_and_left_alone() {
    on_the_real_trash(
        "fs::real_trash_tests::a_taken_name_is_reported_and_left_alone",
        |scene| {
            let taken = scene.write("taken.txt", "old");
            scene.trash.trash(&taken).unwrap();
            scene.write("taken.txt", "new");

            let item = scene.listed("taken.txt");
            assert_eq!(scene.trash.restore(&item), Err(RestoreError::Taken));

            assert_eq!(std::fs::read_to_string(&taken).unwrap(), "new");
            assert_eq!(scene.items_of_ours("taken.txt").len(), 1);
            if cfg!(target_os = "macos") {
                std::fs::symlink_metadata(Path::new(&item.key)).unwrap();
            }
            scene.finish();
        },
    );
}

/// A disk image attached for one test, detached again also when the test
/// failed. Never panics in the drop.
#[cfg(target_os = "macos")]
struct AttachedImage {
    mount: PathBuf,
}

#[cfg(target_os = "macos")]
impl AttachedImage {
    fn new(folder: &Path) -> Self {
        let image = folder.join("volume.dmg");
        let mount = folder.join("volume");
        hdiutil(&[
            "create".as_ref(),
            "-size".as_ref(),
            "16m".as_ref(),
            "-fs".as_ref(),
            "HFS+".as_ref(),
            "-volname".as_ref(),
            "ncrs trash test".as_ref(),
            image.as_os_str(),
        ]);
        std::fs::create_dir(&mount).unwrap();
        hdiutil(&[
            "attach".as_ref(),
            "-nobrowse".as_ref(),
            "-noautoopen".as_ref(),
            "-mountpoint".as_ref(),
            mount.as_os_str(),
            image.as_os_str(),
        ]);
        Self { mount }
    }
}

#[cfg(target_os = "macos")]
impl Drop for AttachedImage {
    fn drop(&mut self) {
        let detached = std::process::Command::new("hdiutil")
            .arg("detach")
            .arg(&self.mount)
            .arg("-force")
            .output();
        match detached {
            Ok(output) if output.status.success() => {}
            Ok(output) => eprintln!(
                "cleanup: {} stays attached: {}",
                self.mount.display(),
                String::from_utf8_lossy(&output.stderr)
            ),
            Err(err) => eprintln!("cleanup: hdiutil cannot run: {err}"),
        }
    }
}

#[cfg(target_os = "macos")]
fn hdiutil(args: &[&OsStr]) {
    let output = std::process::Command::new("hdiutil")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "hdiutil {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// tmpfs, a file system of its own on every Linux system, so a file there
/// goes to the trash of that volume and not to the home trash.
#[cfg(target_os = "linux")]
const OTHER_VOLUME: &str = "/dev/shm";

/// Removes the volume trash folder after the test when the test made it and
/// it is empty again. Never panics.
#[cfg(target_os = "linux")]
struct VolumeTrashFolder {
    folder: PathBuf,
    made_by_the_test: bool,
}

#[cfg(target_os = "linux")]
impl Drop for VolumeTrashFolder {
    fn drop(&mut self) {
        if !self.made_by_the_test {
            return;
        }
        for inside in ["files", "info", "directorysizes"] {
            let removed = std::fs::remove_file(self.folder.join(inside))
                .or_else(|_| std::fs::remove_dir(self.folder.join(inside)));
            match removed {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => eprintln!(
                    "cleanup: {inside} stays in {}: {err}",
                    self.folder.display()
                ),
                _ => {}
            }
        }
        if let Err(err) = std::fs::remove_dir(&self.folder) {
            eprintln!("cleanup: {} stays: {err}", self.folder.display());
        }
    }
}

/// Runs `scenario` in a folder on another volume than the temp folder. The
/// second argument is the trash folder of that volume. Windows has no second
/// volume on the CI runner that a test could make without admin rights.
#[cfg(not(windows))]
fn on_another_volume(test_name: &str, scenario: impl FnOnce(&Scene, &Path)) {
    #[cfg(target_os = "macos")]
    {
        let _ = test_name;
        let image_folder = new_root();
        let volume = AttachedImage::new(image_folder.path());
        let root = tempfile::Builder::new()
            .prefix("ncrs-real-trash-")
            .tempdir_in(&volume.mount)
            .unwrap();
        let volume_trash = std::fs::canonicalize(&volume.mount)
            .unwrap()
            .join(".Trashes");
        scenario(&Scene::new(root.path()), &volume_trash);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt as _;

        if std::env::var_os(ROOT_VARIABLE).is_none() {
            run_in_private_trash(test_name);
            return;
        }
        let root = tempfile::Builder::new()
            .prefix("ncrs-real-trash-")
            .tempdir_in(OTHER_VOLUME)
            .unwrap();
        let here = std::fs::metadata(root.path()).unwrap();
        assert_ne!(
            here.dev(),
            std::fs::metadata(std::env::temp_dir()).unwrap().dev(),
            "{OTHER_VOLUME} is on the same volume as the temp folder"
        );
        let folder = Path::new(OTHER_VOLUME).join(format!(".Trash-{}", here.uid()));
        let _volume_trash = VolumeTrashFolder {
            made_by_the_test: !folder.exists(),
            folder: folder.clone(),
        };
        scenario(&Scene::new(root.path()), &folder);
    }
}

#[cfg(not(windows))]
#[test]
fn a_file_and_a_folder_on_another_volume_come_back_from_its_trash() {
    on_another_volume(
        "fs::real_trash_tests::a_file_and_a_folder_on_another_volume_come_back_from_its_trash",
        |scene, volume_trash| {
            let alpha = scene.write("alpha.txt", "alpha");
            let docs = scene.work.join("docs");
            scene.write("docs/inner.txt", "inner");

            scene.trash.trash(&alpha).unwrap();
            scene.trash.trash(&docs).unwrap();
            scene.assert_gone(&alpha);
            scene.assert_gone(&docs);

            let mut items = scene.items_of_ours("alpha.txt");
            items.extend(scene.items_of_ours("docs"));
            assert_eq!(items.len(), 2, "listed: {items:?}");
            for item in &items {
                let in_trash = std::fs::canonicalize(Path::new(&item.key)).unwrap();
                assert!(
                    in_trash.starts_with(std::fs::canonicalize(volume_trash).unwrap()),
                    "{} is not in the trash of the volume {}",
                    in_trash.display(),
                    volume_trash.display()
                );
            }

            for item in &items {
                assert_eq!(scene.trash.restore(item), Ok(()));
            }

            assert_eq!(std::fs::read_to_string(&alpha).unwrap(), "alpha");
            assert_eq!(
                std::fs::read_to_string(docs.join("inner.txt")).unwrap(),
                "inner"
            );
            assert!(scene.items_of_ours("alpha.txt").is_empty());
            assert!(scene.items_of_ours("docs").is_empty());
            scene.finish();
        },
    );
}
