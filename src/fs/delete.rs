//! Removing entries: to the trash by default, for good on request.
//!
//! Nothing here is async; the caller runs it on the blocking pool, one entry
//! at a time, so the job can be stopped between two entries.

#[cfg(not(target_os = "macos"))]
use std::ffi::OsStr;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(target_os = "macos")]
use super::trash_journal::{JournaledTrash, TrashBackend};
#[cfg(target_os = "macos")]
use objc2_foundation::{
    NSFileManager, NSInteger, NSSearchPathDirectory, NSSearchPathDomainMask, NSString, NSURL,
};

/// What this machine's trash can show and give back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrashReach {
    /// Everything in it, whoever put it there (Windows, Linux).
    // reason: only the trash of Windows and Linux says so
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Everything,
    /// Only what ncrs trashed itself, from a record it keeps (macOS).
    // reason: only the trash of macOS says so
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    OwnDeletions,
    /// Nothing: the Finder is the only way to look into it.
    // reason: only the sandboxed macOS build says so
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    FinderOnly,
}

/// One entry in the trash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashedItem {
    /// Where it was before it was trashed.
    pub original: PathBuf,
    /// Seconds since the Unix epoch.
    pub deleted_at: i64,
    /// What the trash knows it by: the `.trashinfo` path on Linux, the shell
    /// parsing name on Windows, the path inside the trash on macOS.
    pub key: OsString,
}

/// Why an entry did not come back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreError {
    /// Something else is at the original place now.
    Taken,
    Failed(String),
}

/// Where "delete" puts an entry that can still be got back.
///
/// A trait so the end-to-end tests can hand the app a trash of their own. The
/// system trash is the developer's real one, and a test must never fill it.
pub trait Trash: Send + Sync {
    /// Moves `path` to the trash. The error is already text: the platform
    /// errors have no common type the app could match on.
    fn trash(&self, path: &Path) -> Result<(), String>;

    fn reach(&self) -> TrashReach;

    /// Newest first.
    fn list(&self) -> Result<Vec<TrashedItem>, String>;

    /// Puts `item` back at `item.original`. Never replaces anything there: a
    /// taken name is `Taken`. A missing original folder is created.
    fn restore(&self, item: &TrashedItem) -> Result<(), RestoreError>;
}

/// The system trash for this platform. `journal` is where macOS records what
/// ncrs trashed: the system gives no list of a trash folder to an app without
/// Full Disk Access, so the record is the only list there is.
///
/// The App Store build stays Finder-only: nobody has verified yet that the
/// sandbox may move an item back out of `~/.Trash`.
pub fn system_trash(journal: Option<PathBuf>) -> Arc<dyn Trash> {
    system_trash_with(journal)
}

#[cfg(not(target_os = "macos"))]
fn system_trash_with(_journal: Option<PathBuf>) -> Arc<dyn Trash> {
    Arc::new(OsTrash)
}

#[cfg(target_os = "macos")]
fn system_trash_with(journal: Option<PathBuf>) -> Arc<dyn Trash> {
    match journal {
        Some(record) if !super::sandbox_active() => {
            Arc::new(JournaledTrash::new(record, FileManagerBackend))
        }
        _ => Arc::new(FinderOnlyTrash),
    }
}

#[cfg(not(target_os = "macos"))]
struct OsTrash;

#[cfg(not(target_os = "macos"))]
impl Trash for OsTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        trash::delete(path).map_err(|err| err.to_string())
    }

    fn reach(&self) -> TrashReach {
        TrashReach::Everything
    }

    fn list(&self) -> Result<Vec<TrashedItem>, String> {
        let mut items: Vec<TrashedItem> = trash::os_limited::list()
            .map_err(|err| err.to_string())?
            .into_iter()
            .map(|item| TrashedItem {
                original: recycle_bin_original(&item.id).unwrap_or_else(|| item.original_path()),
                deleted_at: item.time_deleted,
                key: item.id,
            })
            .collect();
        items.sort_by_key(|item| std::cmp::Reverse(item.deleted_at));
        Ok(items)
    }

    fn restore(&self, item: &TrashedItem) -> Result<(), RestoreError> {
        let (Some(name), Some(parent)) = (item.original.file_name(), item.original.parent()) else {
            return Err(RestoreError::Failed(format!(
                "{} has no file name",
                item.original.display()
            )));
        };
        // Windows does not create the folder itself, Linux does.
        std::fs::create_dir_all(parent).map_err(|err| RestoreError::Failed(err.to_string()))?;
        if std::fs::symlink_metadata(&item.original).is_ok() {
            return Err(RestoreError::Taken);
        }
        let entry = trash::TrashItem {
            id: item.key.clone(),
            name: name.to_owned(),
            original_parent: parent.to_path_buf(),
            time_deleted: item.deleted_at,
        };
        match trash::os_limited::restore_all([entry]) {
            Ok(()) => Ok(()),
            Err(trash::Error::RestoreCollision { .. }) => Err(RestoreError::Taken),
            Err(err) => Err(RestoreError::Failed(err.to_string())),
        }
    }
}

/// The Windows listing takes the name from the shell's relative display name,
/// which loses the extension while Explorer hides extensions: `a.txt` would be
/// restored as `a`. The `$I` file next to the trashed `$R` file holds the true
/// original path.
#[cfg(windows)]
fn recycle_bin_original(id: &OsStr) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt as _;

    let trashed = Path::new(id);
    let name = trashed.file_name()?.to_str()?;
    let rest = name.strip_prefix("$R")?;
    let bytes = std::fs::read(trashed.with_file_name(format!("$I{rest}"))).ok()?;
    let units = recycle_info_original(&bytes)?;
    Some(PathBuf::from(OsString::from_wide(&units)))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn recycle_bin_original(_id: &OsStr) -> Option<PathBuf> {
    None
}

/// The original path, as UTF-16 units, from the bytes of a `$I` file of the
/// Windows Recycle Bin. Little-endian: version (u64, 1 or 2), size, deletion
/// time, then the path. Version 1 has a fixed 260 units; version 2 gives the
/// number of units, null included, in a u32.
// reason: the `$I` file is little-endian by definition, whatever the CPU
#[allow(clippy::little_endian_bytes)]
#[cfg(any(windows, test))]
fn recycle_info_original(bytes: &[u8]) -> Option<Vec<u16>> {
    const PATH_UNITS_V1: usize = 260;
    let version = u64::from_le_bytes(bytes.get(0..8)?.try_into().ok()?);
    let (start, unit_count) = match version {
        1 => (24, PATH_UNITS_V1),
        2 => {
            let count = u32::from_le_bytes(bytes.get(24..28)?.try_into().ok()?);
            (28, usize::try_from(count).ok()?)
        }
        _ => return None,
    };
    let raw = bytes.get(start..)?.get(..unit_count.checked_mul(2)?)?;
    let units: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .collect();
    // Without a terminating null inside the stated length the record is cut off.
    if units.is_empty() || units.len() == unit_count {
        return None;
    }
    Some(units)
}

/// The three calls the journal needs, through `NSFileManager` directly. The
/// `trash` crate asks the Finder by AppleEvent, which the App Sandbox forbids,
/// and it can neither say where an item landed nor put one back.
#[cfg(target_os = "macos")]
struct FileManagerBackend;

/// `NSFileWriteFileExistsError` in `NSCocoaErrorDomain`.
#[cfg(target_os = "macos")]
const FILE_EXISTS_ERROR_CODE: NSInteger = 516;

#[cfg(target_os = "macos")]
fn file_url(path: &Path) -> Result<impl std::ops::Deref<Target = NSURL>, String> {
    let text = path
        .to_str()
        .ok_or_else(|| format!("{} is not valid UTF-8", path.display()))?;
    Ok(NSURL::fileURLWithPath(&NSString::from_str(text)))
}

#[cfg(target_os = "macos")]
impl TrashBackend for FileManagerBackend {
    fn move_to_trash(&self, path: &Path) -> Result<PathBuf, String> {
        let url = file_url(path)?;
        let mut landed_url = None;
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, Some(&mut landed_url))
            .map_err(|err| err.localizedDescription().to_string())?;
        landed_url
            .and_then(|landed| landed.path())
            .map(|landed| PathBuf::from(landed.to_string()))
            .ok_or_else(|| "the file manager did not say where it went".to_string())
    }

    fn trash_folder_for(&self, original: &Path) -> Option<PathBuf> {
        let existing = original
            .parent()?
            .ancestors()
            .find(|folder| folder.is_dir())?;
        let url = file_url(existing).ok()?;
        let folder = NSFileManager::defaultManager()
            .URLForDirectory_inDomain_appropriateForURL_create_error(
                NSSearchPathDirectory::TrashDirectory,
                NSSearchPathDomainMask::UserDomainMask,
                Some(&url),
                false,
            )
            .ok()?
            .path()?;
        Some(PathBuf::from(folder.to_string()))
    }

    fn move_back(&self, from: &Path, to: &Path) -> Result<(), RestoreError> {
        let from_url = file_url(from).map_err(RestoreError::Failed)?;
        let to_url = file_url(to).map_err(RestoreError::Failed)?;
        NSFileManager::defaultManager()
            .moveItemAtURL_toURL_error(&from_url, &to_url)
            .map_err(|err| {
                if err.code() == FILE_EXISTS_ERROR_CODE {
                    RestoreError::Taken
                } else {
                    RestoreError::Failed(err.localizedDescription().to_string())
                }
            })
    }
}

/// The journaled trash of the plain build, also where `system_trash` would
/// pick the Finder-only one because the process runs in the App Sandbox.
#[cfg(test)]
#[cfg(target_os = "macos")]
pub(in crate::fs) fn file_manager_trash(journal: PathBuf) -> Arc<dyn Trash> {
    Arc::new(JournaledTrash::new(journal, FileManagerBackend))
}

/// Trashes through the file manager and keeps no record: the sandbox build,
/// and a run without a journal path.
#[cfg(target_os = "macos")]
struct FinderOnlyTrash;

#[cfg(target_os = "macos")]
impl Trash for FinderOnlyTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        FileManagerBackend.move_to_trash(path).map(|_| ())
    }

    fn reach(&self) -> TrashReach {
        TrashReach::FinderOnly
    }

    fn list(&self) -> Result<Vec<TrashedItem>, String> {
        Ok(Vec::new())
    }

    fn restore(&self, _item: &TrashedItem) -> Result<(), RestoreError> {
        Err(RestoreError::Failed(
            "this build can only open the trash in the Finder".to_string(),
        ))
    }
}

/// Whether `listed`, an original as the trash lists it, is the place
/// `trashed` was moved from. The system may spell it differently from the
/// panel ncrs deleted in: links resolved on Linux, the long name without the
/// `\\?\` prefix on Windows. So the folders are compared resolved, which
/// needs both to exist; the names must match as they are.
pub fn is_same_place(listed: &Path, trashed: &Path) -> bool {
    if listed == trashed {
        return true;
    }
    let (Some(listed_folder), Some(trashed_folder)) = (listed.parent(), trashed.parent()) else {
        return false;
    };
    listed.file_name() == trashed.file_name()
        && matches!(
            (std::fs::canonicalize(listed_folder), std::fs::canonicalize(trashed_folder)),
            (Ok(listed_real), Ok(trashed_real)) if listed_real == trashed_real
        )
}

/// Deletes `path` for good. A symlink is removed itself and never followed: a
/// link to a directory elsewhere must not take that directory along.
pub fn remove_permanently(path: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
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
    use super::*;

    #[test]
    fn a_file_and_a_tree_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        let tree = dir.path().join("tree");
        std::fs::create_dir_all(tree.join("inner")).unwrap();
        std::fs::write(tree.join("inner/b.txt"), "y").unwrap();

        remove_permanently(&file).unwrap();
        remove_permanently(&tree).unwrap();

        assert!(!file.exists() && !tree.exists());
    }

    /// `symlink_metadata` is the guard: `metadata` would call the link a
    /// directory and `remove_dir_all` would then empty the target.
    #[cfg(unix)]
    #[test]
    fn a_link_to_a_directory_leaves_the_target_alone() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), "k").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        remove_permanently(&link).unwrap();

        assert!(std::fs::symlink_metadata(&link).is_err(), "link remains");
        assert!(outside.join("keep.txt").is_file(), "target was touched");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_file_manager_never_moves_onto_an_occupied_name() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from.txt");
        let taken = dir.path().join("taken.txt");
        let free = dir.path().join("free.txt");
        std::fs::write(&from, "moving").unwrap();
        std::fs::write(&taken, "occupant").unwrap();

        assert_eq!(
            FileManagerBackend.move_back(&from, &taken),
            Err(RestoreError::Taken)
        );
        assert_eq!(std::fs::read_to_string(&taken).unwrap(), "occupant");
        assert!(from.is_file());

        FileManagerBackend.move_back(&from, &free).unwrap();
        assert_eq!(std::fs::read_to_string(&free).unwrap(), "moving");
        assert!(!from.exists());
    }

    // reason: the `$I` file is little-endian by definition, whatever the CPU
    #[allow(clippy::little_endian_bytes)]
    fn info_bytes(version: u64, count_field: Option<u32>, path: &str, units: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(version.to_le_bytes());
        let size_and_time: [u64; 2] = [0, 0];
        bytes.extend(size_and_time.iter().flat_map(|field| field.to_le_bytes()));
        if let Some(count) = count_field {
            bytes.extend(count.to_le_bytes());
        }
        let mut written: Vec<u16> = path.encode_utf16().collect();
        written.resize(units, 0);
        bytes.extend(written.iter().flat_map(|unit| unit.to_le_bytes()));
        bytes
    }

    #[test]
    fn a_version_2_recycle_record_gives_the_path() {
        let path = r"C:\Users\a\work\alpha.txt";
        let units = path.encode_utf16().count() + 1;
        let bytes = info_bytes(2, Some(units as u32), path, units);
        assert_eq!(
            recycle_info_original(&bytes),
            Some(path.encode_utf16().collect())
        );
    }

    #[test]
    fn a_version_1_recycle_record_gives_the_path() {
        let path = r"C:\work\b.txt";
        let bytes = info_bytes(1, None, path, 260);
        assert_eq!(
            recycle_info_original(&bytes),
            Some(path.encode_utf16().collect())
        );
    }

    #[test]
    fn a_recycle_record_shorter_than_it_says_is_refused() {
        let path = r"C:\work\c.txt";
        let units = path.encode_utf16().count() + 1;
        let mut bytes = info_bytes(2, Some(units as u32 + 5), path, units);
        assert_eq!(recycle_info_original(&bytes), None);
        bytes.truncate(26);
        assert_eq!(recycle_info_original(&bytes), None);
        assert_eq!(recycle_info_original(&info_bytes(1, None, path, 100)), None);
    }

    #[test]
    fn a_recycle_record_of_an_unknown_version_is_refused() {
        let bytes = info_bytes(3, Some(4), "abc", 4);
        assert_eq!(recycle_info_original(&bytes), None);
    }

    #[test]
    fn a_recycle_record_without_a_path_is_refused() {
        assert_eq!(recycle_info_original(&info_bytes(2, Some(3), "", 3)), None);
        assert_eq!(recycle_info_original(&info_bytes(1, None, "", 260)), None);
    }

    #[test]
    fn the_same_spelling_is_the_same_place_even_when_gone() {
        let gone = Path::new("/nowhere/at/all/alpha.txt");
        assert!(is_same_place(gone, gone));
        assert!(!is_same_place(gone, Path::new("/nowhere/at/all/beta.txt")));
    }

    /// The Linux trash records the resolved path; the panel may show the
    /// folder through a link.
    #[cfg(unix)]
    #[test]
    fn a_folder_reached_through_a_link_is_the_same_place() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let other = dir.path().join("other");
        std::fs::create_dir(&other).unwrap();

        assert!(is_same_place(&real.join("a.txt"), &link.join("a.txt")));
        assert!(!is_same_place(&real.join("a.txt"), &link.join("b.txt")));
        assert!(!is_same_place(&other.join("a.txt"), &link.join("a.txt")));
    }

    #[test]
    fn a_missing_entry_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(remove_permanently(&dir.path().join("gone")).is_err());
    }
}
