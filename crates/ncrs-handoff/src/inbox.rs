//! The folder a second start leaves its paths in for the running ncrs.
//!
//! One small file per call, written under a name the reader skips and then
//! renamed, so the reader never sees half a file. The running ncrs looks into
//! the folder a few times a second and removes what it read.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::Request;

const FOLDER: &str = "handoff";
const FILE_END: &str = ".req";
/// A file nobody read in this time belongs to an ncrs that is gone.
const STALE_AFTER: Duration = Duration::from_secs(60);
/// Between the records of a file; no path can contain it.
const SEPARATOR: u8 = 0;
const OPEN: u8 = b'o';
const REVEAL: u8 = b'r';

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn folder(data_dir: &Path) -> PathBuf {
    data_dir.join(FOLDER)
}

/// Leaves `requests` for the ncrs that holds the lock of `data_dir`.
pub fn send(data_dir: &Path, requests: &[Request]) -> io::Result<()> {
    let folder = folder(data_dir);
    fs::create_dir_all(&folder)?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    // Zero-padded so that the names sort in the order they were sent.
    let name = format!(
        "{nanos:039}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let writing = folder.join(format!(".{name}.tmp"));
    fs::write(&writing, encode(requests))?;
    fs::rename(&writing, folder.join(format!("{name}{FILE_END}")))
}

/// Everything sent so far, oldest first; the files are removed.
pub fn take(data_dir: &Path) -> Vec<Request> {
    let Ok(entries) = fs::read_dir(folder(data_dir)) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_request_file(path))
        .collect();
    files.sort();
    files
        .iter()
        .filter_map(|file| {
            let content = fs::read(file).ok()?;
            fs::remove_file(file).ok()?;
            Some(decode(&content))
        })
        .flatten()
        .collect()
}

/// Removes what an earlier run left behind and nobody read. Called before the
/// first start puts its own paths in.
pub fn discard_stale(data_dir: &Path) {
    let Ok(entries) = fs::read_dir(folder(data_dir)) else {
        return;
    };
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        let age = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok());
        if age.is_some_and(|age| age > STALE_AFTER) {
            drop(fs::remove_file(&path));
        }
    }
}

fn is_request_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| !name.starts_with('.') && name.ends_with(FILE_END))
}

fn encode(requests: &[Request]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for request in requests {
        if !bytes.is_empty() {
            bytes.push(SEPARATOR);
        }
        bytes.push(match request {
            Request::Open(_) => OPEN,
            Request::Reveal(_) => REVEAL,
        });
        bytes.extend(bytes_of(request.path().as_os_str().to_os_string()));
    }
    bytes
}

fn decode(content: &[u8]) -> Vec<Request> {
    content
        .split(|byte| *byte == SEPARATOR)
        .filter_map(|record| {
            let (kind, path) = record.split_first()?;
            let path = PathBuf::from(os_string_of(path.to_vec()));
            match *kind {
                OPEN => Some(Request::Open(path)),
                REVEAL => Some(Request::Reveal(path)),
                _ => None,
            }
        })
        .collect()
}

#[cfg(unix)]
fn bytes_of(text: OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStringExt as _;
    text.into_vec()
}

#[cfg(unix)]
fn os_string_of(bytes: Vec<u8>) -> OsString {
    use std::os::unix::ffi::OsStringExt as _;
    OsString::from_vec(bytes)
}

#[cfg(not(unix))]
fn bytes_of(text: OsString) -> Vec<u8> {
    text.to_string_lossy().into_owned().into_bytes()
}

#[cfg(not(unix))]
fn os_string_of(bytes: Vec<u8>) -> OsString {
    OsString::from(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(path: &str) -> Request {
        Request::Open(PathBuf::from(path))
    }

    #[test]
    fn what_is_sent_comes_out_once_in_the_order_it_went_in() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        send(
            scratch.path(),
            &[open("/a"), Request::Reveal("/b c".into())],
        )
        .expect("first");
        send(scratch.path(), &[open("/d")]).expect("second");
        assert_eq!(
            take(scratch.path()),
            vec![open("/a"), Request::Reveal("/b c".into()), open("/d")]
        );
        assert!(take(scratch.path()).is_empty());
    }

    #[test]
    fn taking_from_a_folder_nobody_sent_to_is_empty() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        assert!(take(&scratch.path().join("none")).is_empty());
    }

    #[test]
    fn a_file_still_being_written_is_not_read() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let inbox = folder(scratch.path());
        fs::create_dir_all(&inbox).expect("inbox");
        fs::write(inbox.join(".half.tmp"), "oHalf").expect("half file");
        assert!(take(scratch.path()).is_empty());
        assert!(inbox.join(".half.tmp").exists());
    }

    #[test]
    fn fresh_files_survive_the_cleanup() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        send(scratch.path(), &[open("/a")]).expect("sent");
        discard_stale(scratch.path());
        assert_eq!(take(scratch.path()), vec![open("/a")]);
    }

    #[test]
    fn records_of_an_unknown_kind_are_ignored() {
        assert_eq!(decode(b"x/a\0o/b"), vec![open("/b")]);
    }
}
