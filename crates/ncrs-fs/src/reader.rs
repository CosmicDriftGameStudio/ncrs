use std::io;
use std::path::{Path, PathBuf};

use super::{archive, sort_entries, FileEntry, SortKey};

/// Why a directory read failed, without any user-facing text. The message is
/// formatted by the UI layer, which owns the language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The OS refused to read the directory. The inner string is the OS message.
    CannotRead { reason: String },
    /// macOS privacy protection (TCC) refused access; ordinary Unix
    /// permissions give EACCES instead, which stays `CannotRead`. The inner
    /// string is the OS message.
    AccessBlocked { reason: String },
    /// The background task itself failed, not the filesystem operation.
    TaskFailed { reason: String },
    /// An archive was listed without some entries: paths that would leave the
    /// folder they are copied to, links and special files.
    EntriesLeftOut { count: usize },
    /// An archive whose content or header is encrypted, which ncrs cannot open.
    ArchiveEncrypted,
    /// A 7z archive whose dictionary is larger than ncrs unpacks.
    ArchiveDictionaryTooLarge,
    /// The connection to the server of a panel is gone; opening the folder
    /// again connects anew.
    RemoteDisconnected { server: super::remote::ServerId },
    /// The server did not answer in time.
    RemoteTimeout { host: String },
    /// The server refused to list the folder. The inner string is its message.
    Remote { reason: String },
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::CannotRead { reason }
            | ReadError::AccessBlocked { reason }
            | ReadError::TaskFailed { reason }
            | ReadError::Remote { reason } => f.write_str(reason),
            ReadError::EntriesLeftOut { count } => {
                write!(f, "{count} entries left out of the archive")
            }
            ReadError::ArchiveEncrypted => f.write_str("the archive is encrypted"),
            ReadError::ArchiveDictionaryTooLarge => {
                f.write_str("the archive needs too much memory to unpack")
            }
            ReadError::RemoteDisconnected { server } => write!(f, "connection to {server} lost"),
            ReadError::RemoteTimeout { host } => write!(f, "{host} did not answer in time"),
        }
    }
}

/// Result of reading a directory. Reading never "fails" from the caller's
/// perspective: on error, `entries` contains only `..` (if any) and `error`
/// holds a structured cause the UI renders in the current language.
#[derive(Debug, Clone)]
pub struct Listing {
    pub path: PathBuf,
    pub entries: Vec<FileEntry>,
    pub error: Option<ReadError>,
    /// The order `entries` was sorted in, so a panel can tell whether its own
    /// key has changed since the read began.
    pub sorted_by: SortKey,
    /// The archive file, when `path` lies inside one.
    pub archive: Option<PathBuf>,
}

/// Reads `path` on tokio's blocking thread pool so the UI never blocks.
pub async fn read_directory(path: PathBuf, sorted_by: SortKey) -> Listing {
    let target = path.clone();
    let result = tokio::task::spawn_blocking(move || read_sync(&target, sorted_by)).await;

    match result {
        Ok(read) => Listing {
            path,
            entries: read.entries,
            error: read.error,
            sorted_by,
            archive: read.archive,
        },
        Err(join_err) => Listing {
            entries: parent_only(&path),
            error: Some(ReadError::TaskFailed {
                reason: join_err.to_string(),
            }),
            path,
            sorted_by,
            archive: None,
        },
    }
}

/// `EPERM`, the code macOS answers with when privacy protection refuses a
/// read. `ErrorKind::PermissionDenied` cannot tell it from `EACCES`, so the
/// raw code is compared.
const EPERM: i32 = 1;

fn read_error(err: &io::Error) -> ReadError {
    let reason = err.to_string();
    if cfg!(target_os = "macos") && err.raw_os_error() == Some(EPERM) {
        ReadError::AccessBlocked { reason }
    } else {
        ReadError::CannotRead { reason }
    }
}

/// What one blocking read found, before it is put into a [`Listing`].
struct Read {
    entries: Vec<FileEntry>,
    error: Option<ReadError>,
    archive: Option<PathBuf>,
}

fn read_sync(path: &Path, sorted_by: SortKey) -> Read {
    match std::fs::read_dir(path) {
        Ok(dir) => Read {
            entries: with_parent(
                dir.filter_map(Result::ok)
                    .map(|e| FileEntry::from_dir_entry(&e))
                    .collect(),
                path,
                sorted_by,
            ),
            error: None,
            archive: None,
        },
        Err(err) => match archive::locate_in_archive(path) {
            Some(location) => read_archive(path, &location, sorted_by),
            None => Read {
                entries: parent_only(path),
                error: Some(read_error(&err)),
                archive: None,
            },
        },
    }
}

/// An archive that cannot be read still gets its `..`, so the panel can leave.
fn read_archive(path: &Path, location: &archive::ArchiveLocation, sorted_by: SortKey) -> Read {
    let archive = Some(location.archive.clone());
    match archive::list(location) {
        Ok(listing) => Read {
            entries: with_parent(listing.entries, path, sorted_by),
            error: (listing.left_out > 0).then_some(ReadError::EntriesLeftOut {
                count: listing.left_out,
            }),
            archive,
        },
        Err(err) => Read {
            entries: parent_only(path),
            error: Some(match archive::refusal_of(&err) {
                Some(archive::SevenZipRefusal::Encrypted) => ReadError::ArchiveEncrypted,
                Some(archive::SevenZipRefusal::DictionaryTooLarge) => {
                    ReadError::ArchiveDictionaryTooLarge
                }
                None => ReadError::CannotRead {
                    reason: err.to_string(),
                },
            }),
            archive,
        },
    }
}

fn with_parent(mut entries: Vec<FileEntry>, path: &Path, sorted_by: SortKey) -> Vec<FileEntry> {
    if let Some(parent) = path.parent() {
        entries.push(FileEntry::parent(parent));
    }
    sort_entries(&mut entries, sorted_by);
    entries
}

fn parent_only(path: &Path) -> Vec<FileEntry> {
    path.parent().map(FileEntry::parent).into_iter().collect()
}

/// The user's home directory, falling back to the current dir or `/`. In the
/// App Sandbox it is the real home, not the container `$HOME` names.
pub fn home_dir() -> PathBuf {
    home_dir_with(super::sandbox_home(), |name| std::env::var_os(name))
}

/// `home_dir` with the real home of the sandbox and the environment given.
/// The real home is taken as it is: the sandbox may refuse to look at it before
/// the user has granted it, and a failed `is_dir` must not send the panels to
/// the working directory.
fn home_dir_with(
    sandbox_home: Option<PathBuf>,
    var: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> PathBuf {
    sandbox_home
        .or_else(|| {
            var("HOME")
                .or_else(|| var("USERPROFILE"))
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
        })
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Filesystem root of `path` (`/` on Unix, e.g. `C:\` on Windows).
///
/// Not used at startup any more — see [`start_dir`]. Kept because "go to the
/// filesystem root" is a standard file-manager action and will need it.
#[allow(dead_code)]
pub fn root_of(path: &Path) -> PathBuf {
    path.ancestors()
        .last()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// The directory the app should start in when the user has no better idea:
/// the working directory, falling back to the home directory.
pub fn start_dir() -> PathBuf {
    start_dir_from(std::env::current_dir().ok(), home_dir())
}

/// A launch from Finder or the Dock has `/` as working directory, which says
/// nothing about what the user wants to see, so it counts as no directory.
pub fn start_dir_from(cwd: Option<PathBuf>, home: PathBuf) -> PathBuf {
    cwd.filter(|dir| dir.parent().is_some() && dir.is_dir())
        .unwrap_or(home)
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory that removes itself, including when the test fails
    /// halfway. A hand-made path in `temp_dir()` survives a panic and makes the
    /// next run fail with `AlreadyExists` for no reason.
    fn scratch_dir(label: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("ncrs-{label}-"))
            .tempdir()
            .expect("a scratch directory")
    }

    #[tokio::test]
    async fn reads_directory_with_parent_first() {
        let scratch = scratch_dir("read-listing");
        let dir = scratch.path().to_path_buf();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("b.txt"), b"hello").unwrap();
        std::fs::write(dir.join("A.txt"), b"").unwrap();

        let listing = read_directory(dir.clone(), SortKey::default()).await;
        let names: Vec<_> = listing
            .entries
            .iter()
            .map(|e| e.name.to_string_lossy())
            .collect();
        assert_eq!(names, ["..", "sub", "A.txt", "b.txt"]);
        assert!(listing.error.is_none());
        assert_eq!(listing.entries[3].size, 5);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ordinary_permission_denied_stays_cannot_read() {
        assert!(matches!(
            read_error(&io::Error::from_raw_os_error(13)),
            ReadError::CannotRead { .. }
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn eperm_on_macos_means_access_blocked() {
        assert!(matches!(
            read_error(&io::Error::from_raw_os_error(EPERM)),
            ReadError::AccessBlocked { .. }
        ));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn eperm_elsewhere_stays_cannot_read() {
        assert!(matches!(
            read_error(&io::Error::from_raw_os_error(EPERM)),
            ReadError::CannotRead { .. }
        ));
    }

    #[tokio::test]
    async fn missing_directory_yields_error_not_panic() {
        let listing =
            read_directory(PathBuf::from("/definitely/not/here"), SortKey::default()).await;
        assert!(listing.error.is_some());
        assert_eq!(listing.entries.len(), 1);
        assert!(listing.entries[0].is_parent);
        assert_eq!(listing.archive, None);
    }

    fn names_of(listing: &Listing) -> Vec<String> {
        listing
            .entries
            .iter()
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect()
    }

    #[tokio::test]
    async fn a_path_inside_an_archive_lists_like_a_folder_with_its_parent() {
        use crate::archive::fixtures::{write_archive, Item};
        let scratch = scratch_dir("read-archive");
        let archive = scratch.path().join("box.tar.gz");
        write_archive(
            &archive,
            &[Item::File("2024/a.txt", b"a"), Item::File("top.txt", b"t")],
        );

        let root = read_directory(archive.clone(), SortKey::default()).await;
        assert_eq!(names_of(&root), ["..", "2024", "top.txt"]);
        assert_eq!(root.entries[0].path, scratch.path());
        assert_eq!(root.archive.as_deref(), Some(archive.as_path()));
        assert_eq!(root.error, None);

        let inside = read_directory(archive.join("2024"), SortKey::default()).await;
        assert_eq!(names_of(&inside), ["..", "a.txt"]);
        assert_eq!(inside.entries[0].path, archive);
        assert_eq!(inside.archive.as_deref(), Some(archive.as_path()));
    }

    #[tokio::test]
    async fn entries_an_archive_may_not_deliver_are_reported_not_hidden() {
        use crate::archive::fixtures::{write_archive, Item};
        let scratch = scratch_dir("read-archive-left-out");
        let archive = scratch.path().join("bad.zip");
        write_archive(
            &archive,
            &[Item::File("../evil.txt", b"e"), Item::File("ok.txt", b"o")],
        );

        let listing = read_directory(archive, SortKey::default()).await;

        assert_eq!(names_of(&listing), ["..", "ok.txt"]);
        assert_eq!(listing.error, Some(ReadError::EntriesLeftOut { count: 1 }));
    }

    #[tokio::test]
    async fn an_encrypted_archive_is_reported_as_such_and_keeps_the_way_out() {
        use crate::archive::fixtures::ENCRYPTED_7Z;
        let scratch = scratch_dir("read-archive-encrypted");
        for (file, bytes) in ENCRYPTED_7Z {
            let archive = scratch.path().join(file);
            std::fs::write(&archive, bytes).unwrap();

            let listing = read_directory(archive, SortKey::default()).await;

            assert_eq!(names_of(&listing), [".."], "{file}");
            assert_eq!(listing.error, Some(ReadError::ArchiveEncrypted), "{file}");
        }
    }

    #[tokio::test]
    async fn a_broken_or_missing_place_in_an_archive_keeps_only_the_way_out() {
        use crate::archive::fixtures::{write_archive, Item};
        let scratch = scratch_dir("read-archive-broken");
        let broken = scratch.path().join("broken.zip");
        std::fs::write(&broken, b"not an archive").unwrap();
        let good = scratch.path().join("good.zip");
        write_archive(&good, &[Item::File("a.txt", b"a")]);

        let listing = read_directory(broken.clone(), SortKey::default()).await;
        assert_eq!(names_of(&listing), [".."]);
        assert!(matches!(listing.error, Some(ReadError::CannotRead { .. })));
        assert_eq!(listing.archive, Some(broken));

        let missing = read_directory(good.join("nope"), SortKey::default()).await;
        assert_eq!(names_of(&missing), [".."]);
        assert!(matches!(missing.error, Some(ReadError::CannotRead { .. })));
        assert_eq!(missing.archive, Some(good));
    }

    #[test]
    fn the_sandbox_home_wins_over_a_home_variable_that_names_the_container() {
        let container = scratch_dir("container");
        let real = scratch_dir("real-home").path().join("not-listable-yet");
        let var = |name: &str| (name == "HOME").then(|| container.path().as_os_str().to_owned());

        assert_eq!(home_dir_with(Some(real.clone()), var), real);
    }

    #[test]
    fn without_a_sandbox_home_the_variable_decides() {
        let home = scratch_dir("home");
        let var = |name: &str| (name == "HOME").then(|| home.path().as_os_str().to_owned());

        assert_eq!(home_dir_with(None, var), home.path());
    }

    #[test]
    fn a_home_variable_that_is_no_directory_falls_back_to_the_working_directory() {
        let var = |_: &str| Some(std::ffi::OsString::from("no-such-home-folder"));

        assert_eq!(home_dir_with(None, var), std::env::current_dir().unwrap());
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod start_dir_tests {
    use super::*;

    /// Regression: the app opened its right panel at the filesystem root
    /// instead of a useful directory, because `root_of` was used where the
    /// working directory was meant.
    #[test]
    fn start_dir_is_the_working_directory() {
        let start = start_dir();
        assert!(
            start.is_dir(),
            "start_dir {:?} is not a directory",
            start.display()
        );
        // Falls back to home rather than the filesystem root.
        assert_ne!(
            start.parent(),
            None,
            "start_dir should not be the filesystem root"
        );
    }

    #[test]
    fn start_dir_from_uses_home_when_launched_from_the_filesystem_root() {
        let home = PathBuf::from("/home-stand-in");
        assert_eq!(start_dir_from(Some(PathBuf::from("/")), home.clone()), home);
    }

    #[test]
    fn start_dir_from_uses_home_without_a_working_directory() {
        let home = PathBuf::from("/home-stand-in");
        assert_eq!(start_dir_from(None, home.clone()), home);
    }

    #[test]
    fn start_dir_from_keeps_a_real_working_directory() {
        let cwd = std::env::temp_dir();
        assert_eq!(start_dir_from(Some(cwd.clone()), PathBuf::from("/h")), cwd);
    }

    #[test]
    fn root_of_finds_the_filesystem_root() {
        let root = root_of(Path::new("/a/b/c"));
        assert_eq!(root, PathBuf::from("/"));
    }

    /// `start_dir` must never hand out a path that cannot be listed, so a
    /// fresh panel shows entries rather than an error.
    #[tokio::test]
    async fn start_dir_is_readable() {
        let listing = read_directory(start_dir(), SortKey::default()).await;
        assert!(
            listing.error.is_none(),
            "start_dir could not be read: {:?}",
            listing.error
        );
    }
}
