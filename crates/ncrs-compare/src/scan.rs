//! The reading of the two folders: their listings, as the panels read them,
//! and the content of files where it is asked for. Runs off the UI thread.

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ncrs_core::{CompareMark, FileEntry};
use ncrs_fs::remote::session::Pool;
use ncrs_fs::remote::RemotePath;
use ncrs_fs::transfer::Progress;
use ncrs_fs::{read_directory, ReadError, SortKey};

use crate::core::{judge_files, needs_content, Choices, Compared, Facts};

/// Why a comparison produced nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanError {
    /// "Done" or a newer comparison stopped it; nobody wants to hear of it.
    Cancelled,
    /// A folder or a file could not be read; the text names why.
    Failed(String),
}

impl From<io::Error> for ScanError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::Interrupted {
            Self::Cancelled
        } else {
            Self::Failed(error.to_string())
        }
    }
}

/// What one comparison reads with.
#[derive(Debug, Clone)]
pub struct Scan {
    pub pool: Arc<Pool>,
    pub choices: Choices,
    pub stop: Arc<AtomicBool>,
}

impl Scan {
    fn check_stop(&self) -> Result<(), ScanError> {
        if self.stop.load(Ordering::Relaxed) {
            Err(ScanError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// The marks of the two sides of one folder, and whether anything differs.
struct Level {
    compared: Compared,
    differs: bool,
}

type Pending<'a> = Pin<Box<dyn std::future::Future<Output = Result<Level, ScanError>> + Send + 'a>>;

/// Compares the folders `left` and `right`; the result is what their own
/// rows are marked with.
pub async fn compare(scan: Scan, left: PathBuf, right: PathBuf) -> Result<Compared, ScanError> {
    let level = compare_folders(&scan, left, right).await?;
    Ok(level.compared)
}

fn compare_folders(scan: &Scan, left: PathBuf, right: PathBuf) -> Pending<'_> {
    Box::pin(async move {
        scan.check_stop()?;
        let left_rows = list(scan, left).await?;
        let right_rows = list(scan, right).await?;
        let mut compared = Compared::default();
        let mut differs = false;
        let by_name = |rows: &[FileEntry]| -> BTreeMap<OsString, FileEntry> {
            rows.iter()
                .map(|entry| (entry.name.clone(), entry.clone()))
                .collect()
        };
        let right_by_name = by_name(&right_rows);
        let left_names: HashSet<&OsString> = left_rows.iter().map(|entry| &entry.name).collect();
        for left_entry in &left_rows {
            let Some(right_entry) = right_by_name.get(&left_entry.name) else {
                compared
                    .left
                    .insert(left_entry.name.clone(), CompareMark::OnlyHere);
                differs = true;
                continue;
            };
            let pair = compare_pair(scan, left_entry, right_entry).await?;
            differs |= pair.is_some_and(|(left_mark, _)| left_mark.is_difference());
            if let Some((left_mark, right_mark)) = pair {
                compared.left.insert(left_entry.name.clone(), left_mark);
                compared.right.insert(left_entry.name.clone(), right_mark);
            }
        }
        for right_entry in &right_rows {
            if !compared.right.contains_key(&right_entry.name)
                && !left_names.contains(&right_entry.name)
            {
                compared
                    .right
                    .insert(right_entry.name.clone(), CompareMark::OnlyHere);
                differs = true;
            }
        }
        compared.left_rows = left_rows;
        compared.right_rows = right_rows;
        Ok(Level { compared, differs })
    })
}

/// The marks of the left and the right entry of one name. `None` for two
/// folders that are not looked into: with subfolders off, and links to
/// folders, which can lead back into the folder they are in.
async fn compare_pair(
    scan: &Scan,
    left: &FileEntry,
    right: &FileEntry,
) -> Result<Option<(CompareMark, CompareMark)>, ScanError> {
    match (left.is_dir, right.is_dir) {
        (true, true) if scan.choices.subfolders && !left.is_symlink && !right.is_symlink => {
            let below = compare_folders(scan, left.path.clone(), right.path.clone()).await?;
            let mark = if below.differs {
                CompareMark::Different
            } else {
                CompareMark::Same
            };
            Ok(Some((mark, mark)))
        }
        (true, true) => Ok(None),
        (false, false) => {
            let (left_facts, right_facts) = (Facts::of(left), Facts::of(right));
            let same_content = if needs_content(scan.choices, left_facts, right_facts) {
                same_content(scan, left.path.clone(), right.path.clone()).await?
            } else {
                true
            };
            Ok(Some(
                judge_files(scan.choices, left_facts, right_facts, same_content).marks(),
            ))
        }
        (true, false) | (false, true) => Ok(Some((CompareMark::Different, CompareMark::Different))),
    }
}

/// The rows of a folder as the panel shows them, without `..`.
async fn list(scan: &Scan, folder: PathBuf) -> Result<Vec<FileEntry>, ScanError> {
    let listing = match RemotePath::parse(&folder) {
        Some(remote) => ncrs_fs::remote::list::list(&scan.pool, remote, SortKey::default()).await,
        None => read_directory(folder, SortKey::default()).await,
    };
    scan.check_stop()?;
    match listing.error {
        Some(ReadError::EntriesLeftOut { .. }) | None => Ok(listing
            .entries
            .into_iter()
            .filter(|entry| !entry.is_parent)
            .collect()),
        Some(error) => Err(ScanError::Failed(error.to_string())),
    }
}

/// Reads both files, a server's or an archive's into a scratch folder first.
async fn same_content(scan: &Scan, left: PathBuf, right: PathBuf) -> Result<bool, ScanError> {
    let (pool, stop) = (Arc::clone(&scan.pool), Arc::clone(&scan.stop));
    tokio::task::spawn_blocking(move || files_have_same_content(&pool, &stop, &left, &right))
        .await
        .map_err(|error| ScanError::Failed(error.to_string()))?
}

struct StopCheck(Arc<AtomicBool>);

impl StopCheck {
    fn check(&self) -> io::Result<()> {
        if self.0.load(Ordering::Relaxed) {
            Err(io::Error::from(io::ErrorKind::Interrupted))
        } else {
            Ok(())
        }
    }
}

impl Progress for StopCheck {
    fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
        self.check()
    }

    fn keep_going(&mut self) -> io::Result<()> {
        self.check()
    }
}

/// A path of this computer for `path`: itself, or a copy of what a server or
/// an archive holds, made in `slot`.
fn local_file(
    pool: &Pool,
    stop: &Arc<AtomicBool>,
    path: &Path,
    slot: &Path,
) -> io::Result<PathBuf> {
    let mut progress = StopCheck(Arc::clone(stop));
    if let Some(remote) = RemotePath::parse(path) {
        std::fs::create_dir_all(slot)?;
        return ncrs_fs::remote::transfer::download_for_view(pool, &remote, slot, &mut progress);
    }
    if ncrs_fs::archive::locate_in_archive(path).is_some_and(|place| !place.inner.is_empty()) {
        std::fs::create_dir_all(slot)?;
        return ncrs_fs::archive::extract_for_view(path, slot, &mut progress).map(|copy| copy.path);
    }
    Ok(path.to_owned())
}

fn files_have_same_content(
    pool: &Pool,
    stop: &Arc<AtomicBool>,
    left: &Path,
    right: &Path,
) -> Result<bool, ScanError> {
    let scratch = tempfile::tempdir()?;
    let left_file = local_file(pool, stop, left, &scratch.path().join("left"))?;
    let right_file = local_file(pool, stop, right, &scratch.path().join("right"))?;
    Ok(same_bytes(
        &mut File::open(left_file)?,
        &mut File::open(right_file)?,
        &StopCheck(Arc::clone(stop)),
    )?)
}

const BLOCK: usize = 64 * 1024;

/// Reads until `buffer` is full or the file ends; how much it holds.
fn fill(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut held = 0;
    while let Some(room) = buffer.get_mut(held..).filter(|room| !room.is_empty()) {
        let read = reader.read(room)?;
        if read == 0 {
            break;
        }
        held += read;
    }
    Ok(held)
}

fn same_bytes(left: &mut impl Read, right: &mut impl Read, stop: &StopCheck) -> io::Result<bool> {
    let mut left_block: Vec<u8> = vec![0; BLOCK];
    let mut right_block: Vec<u8> = vec![0; BLOCK];
    loop {
        stop.check()?;
        let left_held = fill(left, &mut left_block)?;
        let right_held = fill(right, &mut right_block)?;
        if left_block.get(..left_held) != right_block.get(..right_held) {
            return Ok(false);
        }
        if left_held < BLOCK {
            return Ok(true);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::time::{Duration, SystemTime};

    use ncrs_core::CompareMarks;
    use ncrs_fs::remote::KnownHostsFiles;

    use super::*;

    fn pool() -> Arc<Pool> {
        Arc::new(Pool::over_tcp(KnownHostsFiles::default()))
    }

    fn scan(choices: Choices) -> Scan {
        Scan {
            pool: pool(),
            choices,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    fn write(folder: &Path, name: &str, content: &str, age_seconds: u64) {
        let path = folder.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        let date = SystemTime::now() - Duration::from_secs(age_seconds);
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(date)
            .unwrap();
    }

    fn mark(compared: &Compared, side_is_left: bool, name: &str) -> Option<CompareMark> {
        let marks: &CompareMarks = if side_is_left {
            &compared.left
        } else {
            &compared.right
        };
        marks.get(&OsString::from(name)).copied()
    }

    #[tokio::test]
    async fn two_folders_are_marked_by_name_size_and_date() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        write(left.path(), "only.txt", "a", 100);
        write(right.path(), "gone.txt", "b", 100);
        write(left.path(), "new.txt", "same size", 10);
        write(right.path(), "new.txt", "same size", 1000);
        write(left.path(), "size.txt", "short", 500);
        write(right.path(), "size.txt", "longer text", 500);
        write(left.path(), "same.txt", "equal", 500);
        write(right.path(), "same.txt", "equal", 500);

        let compared = compare(
            scan(Choices::default()),
            left.path().to_owned(),
            right.path().to_owned(),
        )
        .await
        .unwrap();

        assert_eq!(
            mark(&compared, true, "only.txt"),
            Some(CompareMark::OnlyHere)
        );
        assert_eq!(
            mark(&compared, false, "gone.txt"),
            Some(CompareMark::OnlyHere)
        );
        assert_eq!(
            mark(&compared, true, "new.txt"),
            Some(CompareMark::NewerHere)
        );
        assert_eq!(
            mark(&compared, false, "new.txt"),
            Some(CompareMark::OlderHere)
        );
        assert_eq!(
            mark(&compared, true, "size.txt"),
            Some(CompareMark::Different)
        );
        assert_eq!(mark(&compared, true, "same.txt"), Some(CompareMark::Same));
    }

    #[tokio::test]
    async fn equal_size_with_other_content_differs_only_when_content_is_asked() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        write(left.path(), "a.txt", "aaaa", 500);
        write(right.path(), "a.txt", "bbbb", 500);
        let by_content = Choices {
            content: true,
            ..Choices::default()
        };

        let plain = compare(
            scan(Choices::default()),
            left.path().to_owned(),
            right.path().to_owned(),
        )
        .await
        .unwrap();
        let read = compare(
            scan(by_content),
            left.path().to_owned(),
            right.path().to_owned(),
        )
        .await
        .unwrap();

        assert_eq!(mark(&plain, true, "a.txt"), Some(CompareMark::Same));
        assert_eq!(mark(&read, true, "a.txt"), Some(CompareMark::Different));
    }

    #[tokio::test]
    async fn a_folder_is_marked_when_something_below_it_differs() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        write(left.path(), "deep/er/file.txt", "left", 500);
        write(right.path(), "deep/er/file.txt", "right side", 500);
        write(left.path(), "quiet/file.txt", "same", 500);
        write(right.path(), "quiet/file.txt", "same", 500);

        let compared = compare(
            scan(Choices::default()),
            left.path().to_owned(),
            right.path().to_owned(),
        )
        .await
        .unwrap();
        let shallow = compare(
            scan(Choices {
                subfolders: false,
                ..Choices::default()
            }),
            left.path().to_owned(),
            right.path().to_owned(),
        )
        .await
        .unwrap();

        assert_eq!(mark(&compared, true, "deep"), Some(CompareMark::Different));
        assert_eq!(mark(&compared, true, "quiet"), Some(CompareMark::Same));
        assert_eq!(mark(&shallow, true, "deep"), None);
    }

    #[tokio::test]
    async fn a_folder_that_cannot_be_read_ends_the_comparison_with_the_error() {
        let left = tempfile::tempdir().unwrap();
        let missing = left.path().join("nowhere");

        let result = compare(scan(Choices::default()), left.path().to_owned(), missing).await;

        assert!(matches!(result, Err(ScanError::Failed(_))), "{result:?}");
    }

    #[tokio::test]
    async fn a_stopped_comparison_reports_nothing() {
        let left = tempfile::tempdir().unwrap();
        let stopped = scan(Choices::default());
        stopped.stop.store(true, Ordering::Relaxed);

        let result = compare(stopped, left.path().to_owned(), left.path().to_owned()).await;

        assert_eq!(result, Err(ScanError::Cancelled));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_link_back_to_the_parent_on_both_sides_is_not_followed() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        for side in [left.path(), right.path()] {
            write(side, "sub/file.txt", "same", 500);
            std::os::unix::fs::symlink("..", side.join("sub/loop")).unwrap();
        }

        let compared = compare(
            scan(Choices::default()),
            left.path().to_owned(),
            right.path().to_owned(),
        )
        .await
        .unwrap();

        assert_eq!(mark(&compared, true, "sub"), Some(CompareMark::Same));
    }
}
