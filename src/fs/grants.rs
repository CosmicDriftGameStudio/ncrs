//! Folder grants of the App Store build. Inside the App Sandbox the app may read
//! only what the user allowed in the open panel, and that allowance ends with
//! the process unless a security-scoped bookmark of the folder is kept. This
//! module keeps them: one file per grant in a folder of the container, the raw
//! bookmark bytes in it, and brings them back at the next start.
//!
//! The bookmarks themselves are made and resolved by [`Bookmarks`], so the
//! store works, and is tested, without the sandbox or macOS.

use std::fmt;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use tempfile::Builder;

const GRANT_EXTENSION: &str = "bookmark";

/// Why the system could not make or resolve a bookmark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarkFailure {
    /// The bytes are no bookmark and never will be.
    Corrupt(String),
    /// Not right now, for instance because the volume is unplugged.
    Unavailable(String),
}

impl fmt::Display for BookmarkFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt(reason) | Self::Unavailable(reason) => f.write_str(reason),
        }
    }
}

/// What a bookmark resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub path: PathBuf,
    /// The folder moved; the bookmark should be made again.
    pub stale: bool,
    /// Whether the access to `path` is open now.
    pub access_started: bool,
}

/// The system's way to turn a folder into bookmark bytes and back. Resolving
/// opens the access to the folder for the rest of the run.
pub trait Bookmarks {
    fn create(&self, path: &Path) -> Result<Vec<u8>, BookmarkFailure>;
    fn resolve(&self, bytes: &[u8]) -> Result<Resolution, BookmarkFailure>;
    /// The folder the bytes name, without opening the access to it: for
    /// finding the grant a folder has already, which must not start another.
    fn path_of(&self, bytes: &[u8]) -> Result<PathBuf, BookmarkFailure>;
}

/// What went wrong when a grant could not be saved.
#[derive(Debug)]
pub enum GrantError {
    Bookmark(BookmarkFailure),
    Io(io::Error),
}

impl fmt::Display for GrantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bookmark(failure) => write!(f, "bookmark: {failure}"),
            Self::Io(error) => write!(f, "grants folder: {error}"),
        }
    }
}

/// What [`GrantStore::restore_all`] did, for the log.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RestoreReport {
    /// Folders whose access is open again.
    pub restored: Vec<PathBuf>,
    /// Of those, the ones whose bookmark was stale and has been made again.
    pub refreshed: usize,
    /// Files that held no bookmark and were deleted.
    pub removed: usize,
    /// Files kept because their folder is out of reach for now.
    pub unavailable: usize,
    /// Trouble with the files themselves, one line each.
    pub problems: Vec<String>,
}

impl fmt::Display for RestoreReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "folder grants: {} restored ({} renewed), {} unavailable, {} removed",
            self.restored.len(),
            self.refreshed,
            self.unavailable,
            self.removed
        )?;
        for problem in &self.problems {
            write!(f, "; {problem}")?;
        }
        Ok(())
    }
}

/// The folder grants of one data folder.
#[derive(Debug, Clone)]
pub struct GrantStore {
    dir: PathBuf,
}

impl GrantStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The grants folder inside the data folder that holds the index.
    pub fn dir_beside(index_path: &Path) -> Option<PathBuf> {
        index_path.parent().map(|data| data.join("grants"))
    }

    /// Resolves every saved bookmark, which opens the access to its folder. A
    /// stale one is made again and replaced; one that is no bookmark is
    /// deleted; one whose folder cannot be reached now stays for the next
    /// start.
    pub fn restore_all(&self, bookmarks: &dyn Bookmarks) -> RestoreReport {
        let mut report = RestoreReport::default();
        let files = match self.grant_files() {
            Ok(files) => files,
            Err(error) => {
                report
                    .problems
                    .push(format!("{}: {error}", self.dir.display()));
                return report;
            }
        };
        for file in files {
            let bytes = match fs::read(&file) {
                Ok(bytes) => bytes,
                Err(error) => {
                    report.problems.push(format!("{}: {error}", file.display()));
                    continue;
                }
            };
            match bookmarks.resolve(&bytes) {
                Ok(resolution) if !resolution.access_started => report.unavailable += 1,
                Ok(resolution) => {
                    if resolution.stale {
                        match self.renew(bookmarks, &file, &resolution.path) {
                            Ok(()) => report.refreshed += 1,
                            Err(error) => report.problems.push(format!(
                                "{}: not renewed: {error}",
                                resolution.path.display()
                            )),
                        }
                    }
                    report.restored.push(resolution.path);
                }
                Err(BookmarkFailure::Corrupt(_)) => match fs::remove_file(&file) {
                    Ok(()) => report.removed += 1,
                    Err(error) => report.problems.push(format!("{}: {error}", file.display())),
                },
                Err(BookmarkFailure::Unavailable(_)) => report.unavailable += 1,
            }
        }
        report
    }

    /// Saves a bookmark of `folder`, made now: it works only while the access
    /// the open panel gave is fresh. A folder that is granted already has its
    /// file replaced, so there is never a second one.
    pub fn add(&self, bookmarks: &dyn Bookmarks, folder: &Path) -> Result<(), GrantError> {
        let bytes = bookmarks.create(folder).map_err(GrantError::Bookmark)?;
        let existing = self.file_granting(bookmarks, folder);
        self.write(&bytes, existing.as_deref())
            .map(drop)
            .map_err(GrantError::Io)
    }

    fn renew(&self, bookmarks: &dyn Bookmarks, file: &Path, folder: &Path) -> Result<(), String> {
        let bytes = bookmarks
            .create(folder)
            .map_err(|error| error.to_string())?;
        self.write(&bytes, Some(file))
            .map(drop)
            .map_err(|error| error.to_string())
    }

    /// The file of the grant for `folder`, if there is one.
    fn file_granting(&self, bookmarks: &dyn Bookmarks, folder: &Path) -> Option<PathBuf> {
        let files = self.grant_files().ok()?;
        files.into_iter().find(|file| {
            fs::read(file)
                .ok()
                .and_then(|bytes| bookmarks.path_of(&bytes).ok())
                .is_some_and(|named| same_folder(&named, folder))
        })
    }

    /// The `.bookmark` files, in name order. A missing folder holds none.
    fn grant_files(&self) -> io::Result<Vec<PathBuf>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == GRANT_EXTENSION))
            .collect();
        files.sort();
        Ok(files)
    }

    /// Writes `bytes` to a temporary file in the grants folder and renames it
    /// over `replace`, or to a new random name. A crash leaves the old file
    /// whole or the new one whole, never half of one.
    fn write(&self, bytes: &[u8], replace: Option<&Path>) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.dir)?;
        let mut temp = Builder::new()
            .prefix("grant-")
            .suffix(".tmp")
            .rand_bytes(16)
            .tempfile_in(&self.dir)?;
        temp.write_all(bytes)?;
        temp.as_file().sync_all()?;
        match replace {
            Some(target) => {
                temp.persist(target).map_err(|error| error.error)?;
                Ok(target.to_path_buf())
            }
            None => {
                let target = temp.path().with_extension(GRANT_EXTENSION);
                temp.persist_noclobber(&target)
                    .map_err(|error| error.error)?;
                Ok(target)
            }
        }
    }
}

fn same_folder(a: &Path, b: &Path) -> bool {
    a == b || same_file::is_same_file(a, b).unwrap_or(false)
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    /// Bookmarks that are the folder path as text, so a test says what a file
    /// resolves to by what it wrote into it. The answers for a given content
    /// can be overridden.
    #[derive(Default)]
    struct Fake {
        answers: HashMap<Vec<u8>, Result<Resolution, BookmarkFailure>>,
        created: RefCell<Vec<PathBuf>>,
        /// How often a bookmark was resolved with access, which starts it.
        resolutions: RefCell<usize>,
        fail_create: bool,
    }

    impl Fake {
        fn answer(&mut self, bytes: &[u8], answer: Result<Resolution, BookmarkFailure>) {
            self.answers.insert(bytes.to_vec(), answer);
        }
    }

    impl Bookmarks for Fake {
        fn create(&self, path: &Path) -> Result<Vec<u8>, BookmarkFailure> {
            if self.fail_create {
                return Err(BookmarkFailure::Unavailable("refused".to_owned()));
            }
            self.created.borrow_mut().push(path.to_path_buf());
            Ok(format!("new:{}", path.display()).into_bytes())
        }

        fn path_of(&self, bytes: &[u8]) -> Result<PathBuf, BookmarkFailure> {
            self.answer_for(bytes).map(|found| found.path)
        }

        fn resolve(&self, bytes: &[u8]) -> Result<Resolution, BookmarkFailure> {
            *self.resolutions.borrow_mut() += 1;
            self.answer_for(bytes)
        }
    }

    impl Fake {
        fn answer_for(&self, bytes: &[u8]) -> Result<Resolution, BookmarkFailure> {
            if let Some(answer) = self.answers.get(bytes) {
                return answer.clone();
            }
            let text = String::from_utf8_lossy(bytes);
            let path = text.trim_start_matches("new:");
            Ok(Resolution {
                path: PathBuf::from(path),
                stale: false,
                access_started: true,
            })
        }
    }

    fn granted(path: &Path) -> Result<Resolution, BookmarkFailure> {
        Ok(Resolution {
            path: path.to_path_buf(),
            stale: false,
            access_started: true,
        })
    }

    fn store_in(dir: &tempfile::TempDir) -> GrantStore {
        GrantStore::new(dir.path().join("grants"))
    }

    fn put(store: &GrantStore, name: &str, bytes: &[u8]) -> PathBuf {
        fs::create_dir_all(&store.dir).unwrap();
        let file = store.dir.join(format!("{name}.{GRANT_EXTENSION}"));
        fs::write(&file, bytes).unwrap();
        file
    }

    fn names(store: &GrantStore) -> Vec<String> {
        let mut found: Vec<String> = fs::read_dir(&store.dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        found
    }

    #[test]
    fn the_grants_folder_sits_beside_the_index() {
        let index = Path::new("data").join("index.db");
        assert_eq!(
            GrantStore::dir_beside(&index),
            Some(Path::new("data").join("grants"))
        );
        assert_eq!(GrantStore::dir_beside(Path::new("")), None);
    }

    #[test]
    fn without_a_grants_folder_nothing_is_restored_and_nothing_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);

        let report = store.restore_all(&Fake::default());

        assert_eq!(report, RestoreReport::default());
        assert!(!store.dir.exists());
    }

    #[test]
    fn every_saved_grant_is_resolved_and_leaves_its_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let one = dir.path().join("one");
        let two = dir.path().join("two");
        put(&store, "a", one.to_string_lossy().as_bytes());
        put(&store, "b", two.to_string_lossy().as_bytes());
        fs::write(store.dir.join("note.txt"), "not a grant").unwrap();

        let report = store.restore_all(&Fake::default());

        assert_eq!(report.restored, vec![one.clone(), two.clone()]);
        assert_eq!(
            (report.refreshed, report.removed, report.unavailable),
            (0, 0, 0)
        );
        assert_eq!(
            fs::read(store.dir.join("a.bookmark")).unwrap(),
            one.to_string_lossy().as_bytes()
        );
        assert!(store.dir.join("note.txt").exists());
    }

    #[test]
    fn a_stale_grant_is_made_again_and_the_old_bytes_are_gone() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let moved = dir.path().join("moved-to");
        let file = put(&store, "a", b"old bookmark");
        let mut fake = Fake::default();
        fake.answer(
            b"old bookmark",
            Ok(Resolution {
                path: moved.clone(),
                stale: true,
                access_started: true,
            }),
        );

        let report = store.restore_all(&fake);

        assert_eq!(report.restored, vec![moved.clone()]);
        assert_eq!(report.refreshed, 1);
        assert_eq!(*fake.created.borrow(), vec![moved.clone()]);
        assert_eq!(
            fs::read(&file).unwrap(),
            format!("new:{}", moved.display()).into_bytes()
        );
        assert_eq!(
            names(&store),
            vec!["a.bookmark".to_owned()],
            "no temp file left"
        );
    }

    #[test]
    fn a_stale_grant_whose_access_did_not_start_is_not_renewed() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let file = put(&store, "a", b"old bookmark");
        let mut fake = Fake::default();
        fake.answer(
            b"old bookmark",
            Ok(Resolution {
                path: dir.path().join("x"),
                stale: true,
                access_started: false,
            }),
        );

        let report = store.restore_all(&fake);

        assert!(report.restored.is_empty());
        assert_eq!(report.unavailable, 1);
        assert!(fake.created.borrow().is_empty());
        assert_eq!(fs::read(&file).unwrap(), b"old bookmark");
    }

    #[test]
    fn a_stale_grant_that_cannot_be_renewed_keeps_its_file_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let file = put(&store, "a", b"old bookmark");
        let mut fake = Fake {
            fail_create: true,
            ..Fake::default()
        };
        fake.answer(
            b"old bookmark",
            Ok(Resolution {
                path: dir.path().join("x"),
                stale: true,
                access_started: true,
            }),
        );

        let report = store.restore_all(&fake);

        assert_eq!(report.restored.len(), 1);
        assert_eq!(report.refreshed, 0);
        assert_eq!(report.problems.len(), 1);
        assert!(report.problems[0].contains("not renewed"), "{report}");
        assert_eq!(fs::read(&file).unwrap(), b"old bookmark");
    }

    #[test]
    fn a_corrupt_grant_is_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let broken = put(&store, "broken", b"garbage");
        let fine = put(
            &store,
            "fine",
            dir.path().join("f").to_string_lossy().as_bytes(),
        );
        let mut fake = Fake::default();
        fake.answer(b"garbage", Err(BookmarkFailure::Corrupt("no".to_owned())));

        let report = store.restore_all(&fake);

        assert_eq!(report.removed, 1);
        assert!(!broken.exists());
        assert!(fine.exists());
    }

    #[test]
    fn a_grant_that_cannot_be_resolved_now_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let file = put(&store, "usb", b"unplugged");
        let mut fake = Fake::default();
        fake.answer(
            b"unplugged",
            Err(BookmarkFailure::Unavailable("no volume".to_owned())),
        );

        let report = store.restore_all(&fake);

        assert_eq!(report.unavailable, 1);
        assert_eq!(report.removed, 0);
        assert_eq!(fs::read(&file).unwrap(), b"unplugged");
    }

    #[test]
    fn a_new_grant_is_saved_under_its_own_name() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let docs = dir.path().join("docs");
        let other = dir.path().join("other");

        store.add(&Fake::default(), &docs).unwrap();
        store.add(&Fake::default(), &other).unwrap();

        let mut saved: Vec<Vec<u8>> = store
            .grant_files()
            .unwrap()
            .iter()
            .map(|file| fs::read(file).unwrap())
            .collect();
        saved.sort();
        let mut expected = vec![
            format!("new:{}", docs.display()).into_bytes(),
            format!("new:{}", other.display()).into_bytes(),
        ];
        expected.sort();
        assert_eq!(saved, expected);
        assert_eq!(names(&store).len(), 2, "no temp file left");
    }

    #[test]
    fn granting_a_folder_again_replaces_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let folder = dir.path().join("docs");
        let file = put(&store, "first", b"older bytes");
        let mut fake = Fake::default();
        fake.answer(b"older bytes", granted(&folder));

        store.add(&fake, &folder).unwrap();

        assert_eq!(
            *fake.resolutions.borrow(),
            0,
            "no access was started to compare"
        );
        assert_eq!(store.grant_files().unwrap(), vec![file.clone()]);
        assert_eq!(
            fs::read(&file).unwrap(),
            format!("new:{}", folder.display()).into_bytes()
        );
    }

    #[test]
    fn a_failed_bookmark_leaves_the_store_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let file = put(&store, "first", b"kept");
        let fake = Fake {
            fail_create: true,
            ..Fake::default()
        };

        let error = store.add(&fake, &dir.path().join("docs")).unwrap_err();

        assert!(matches!(error, GrantError::Bookmark(_)), "{error}");
        assert_eq!(names(&store), vec!["first.bookmark".to_owned()]);
        assert_eq!(fs::read(&file).unwrap(), b"kept");
    }

    #[test]
    fn the_report_reads_as_one_line_for_the_log() {
        let report = RestoreReport {
            restored: vec![PathBuf::from("a")],
            refreshed: 1,
            removed: 2,
            unavailable: 3,
            problems: vec!["x: broke".to_owned()],
        };
        assert_eq!(
            report.to_string(),
            "folder grants: 1 restored (1 renewed), 3 unavailable, 2 removed; x: broke"
        );
    }
}
