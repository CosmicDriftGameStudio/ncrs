//! Walking a folder and spreading work over threads, for the chat and for
//! sorting.

use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use ncrs_index::FileStamp;
use ncrs_intake::{is_eml_file, is_mail_body};
use ncrs_pipeline::Load;
use ncrs_priority::{set_current_thread_priority, ThreadPriority};

/// Most files read in one folder.
pub const FILE_LIMIT: usize = 20_000;

/// How far a running read is: the files found while walking, the number to
/// read once the walk is done, and the files read so far.
#[derive(Debug, Default)]
pub struct ReadProgress {
    pub found: AtomicUsize,
    pub total: AtomicUsize,
    pub done: AtomicUsize,
}

impl ReadProgress {
    pub fn found(&self) -> usize {
        self.found.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }

    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    NotADirectory,
    /// The folder is not there: deleted, or its drive is gone.
    Gone,
    TooMany {
        limit: usize,
    },
    Io(String),
}

pub struct Walked {
    pub relative: PathBuf,
    pub stamp: FileStamp,
}

pub fn read_error(folder: &Path, err: &io::Error) -> ReadError {
    if err.kind() == io::ErrorKind::NotFound {
        ReadError::Gone
    } else {
        ReadError::Io(format!("{}: {err}", folder.display()))
    }
}

/// A folder or entry that vanishes below the root while it is walked is
/// skipped; only the root being gone ends the read.
fn walk_error(folder: &Path, relative: &Path, err: &io::Error) -> Result<(), ReadError> {
    if err.kind() == io::ErrorKind::NotFound && !relative.as_os_str().is_empty() {
        Ok(())
    } else {
        Err(read_error(folder, err))
    }
}

pub fn walk(
    base: &Path,
    relative: &Path,
    omitted: &[PathBuf],
    found: &mut Vec<Walked>,
    progress: &ReadProgress,
) -> Result<(), ReadError> {
    let folder = base.join(relative);
    let entries = match fs::read_dir(&folder) {
        Ok(entries) => entries,
        Err(err) => return walk_error(&folder, relative, &err),
    };
    let mut dir_entries = Vec::new();
    for entry in entries {
        match entry {
            Ok(dir_entry) => dir_entries.push(dir_entry),
            Err(err) => walk_error(&folder, relative, &err)?,
        }
    }
    let has_eml = dir_entries
        .iter()
        .any(|dir_entry| is_eml_file(&dir_entry.file_name()));
    for dir_entry in dir_entries {
        let name = dir_entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(file_type) = dir_entry.file_type() else {
            continue;
        };
        let path = relative.join(&name);
        if file_type.is_dir() {
            if !omitted.contains(&base.join(&path)) {
                walk(base, &path, omitted, found, progress)?;
            }
        } else if !file_type.is_symlink() && !is_eml_file(&name) && !is_mail_body(&name, has_eml) {
            // A file that cannot be stat'ed cannot be read or filed either.
            let Some(stamp) = dir_entry.metadata().ok().and_then(|metadata| {
                Some(FileStamp {
                    size: metadata.len(),
                    mtime: metadata.modified().ok()?,
                })
            }) else {
                continue;
            };
            found.push(Walked {
                relative: path,
                stamp,
            });
            progress.found.store(found.len(), Ordering::Relaxed);
            if found.len() > FILE_LIMIT {
                return Err(ReadError::TooMany { limit: FILE_LIMIT });
            }
        }
    }
    Ok(())
}

/// The files below `folder` by relative path; the folders in `omitted` are not
/// entered.
pub fn walk_sorted(
    folder: &Path,
    omitted: &[PathBuf],
    progress: &ReadProgress,
) -> Result<Vec<Walked>, ReadError> {
    match fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(ReadError::NotADirectory),
        Err(err) => return Err(read_error(folder, &err)),
    }
    let mut walked = Vec::new();
    walk(folder, Path::new(""), omitted, &mut walked, progress)?;
    walked.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(walked)
}

pub fn is_stopped(stop: Option<&AtomicBool>) -> bool {
    stop.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// `work` on every item, spread over the threads the load allows, each at the
/// load's priority; the results keep the order.
pub fn in_parallel<T: Sync, R: Send>(
    items: &[T],
    load: Load,
    work: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    let cores = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let threads = usize::try_from(load.ai_threads(cores).get())
        .unwrap_or(1)
        .min(8);
    in_threads(items, threads, load.thread_priority(), work)
}

fn in_threads<T: Sync, R: Send>(
    items: &[T],
    threads: usize,
    priority: ThreadPriority,
    work: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    let chunk = items.len().div_ceil(threads).max(1);
    let work_ref = &work;
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    set_current_thread_priority(priority);
                    part.iter().map(work_ref).collect::<Vec<R>>()
                })
            })
            .collect();
        handles
            .into_iter()
            // A lost chunk would shift every later zip of results and inputs.
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vanished_subfolder_is_skipped_but_a_vanished_root_ends_the_read() {
        let gone = io::Error::from(io::ErrorKind::NotFound);
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        let folder = Path::new("/archive/sub");

        assert_eq!(walk_error(folder, Path::new("sub"), &gone), Ok(()));
        assert_eq!(
            walk_error(Path::new("/archive"), Path::new(""), &gone),
            Err(ReadError::Gone)
        );
        assert!(matches!(
            walk_error(folder, Path::new("sub"), &denied),
            Err(ReadError::Io(_))
        ));
    }

    #[test]
    fn in_threads_keeps_the_order_for_any_thread_count() {
        let items: Vec<usize> = (0..50).collect();
        for threads in [1, 2, 7] {
            let doubled = in_threads(&items, threads, ThreadPriority::Utility, |item| {
                item.saturating_mul(2)
            });
            let expected: Vec<usize> = items.iter().map(|item| item.saturating_mul(2)).collect();
            assert_eq!(doubled, expected, "{threads} threads");
        }
    }
}
