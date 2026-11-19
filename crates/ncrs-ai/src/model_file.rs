//! The one model the built-in provider runs, pinned by size and checksum. Not
//! behind the `llama` feature: a build without the runtime still has to know
//! which file to download and how to check it.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

use sha2::{Digest as _, Sha256};

use crate::error::AiError;

/// A model file as it must be: where it comes from, how big it is and its
/// checksum. Nothing from the file is trusted before it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelPin {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub url: &'static str,
}

/// Qwen3.5 4B (Apache-2.0), 4-bit (Q4_K_M, about 2.7 GB), at a fixed revision of
/// its Hugging Face repository. scripts/fetch-ai-model.sh carries the same
/// values; change both together.
pub const LOCAL_MODEL: ModelPin = ModelPin {
    name: "Qwen3.5-4B-Q4_K_M.gguf",
    size: 2_740_937_888,
    sha256: "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4",
    url: "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/e87f176479d0855a907a41277aca2f8ee7a09523/Qwen3.5-4B-Q4_K_M.gguf",
};

/// The folder inside the data folder that holds the model.
pub const LOCAL_MODEL_FOLDER: &str = "ai-models";

/// The files of the models earlier versions shipped.
#[cfg(any(test, feature = "llama"))]
const PREVIOUS_MODEL_FILES: [&str; 2] = ["Qwen3-4B-Q4_K_M.gguf", "Qwen3-0.6B-Q8_0.gguf"];

/// Deletes the files of earlier built-in models from `dir`. Call it only once
/// the current model there is verified: until then an older version may still
/// need its file. Links and folders with those names stay.
#[cfg(any(test, feature = "llama"))]
pub fn remove_previous_models(dir: &Path) {
    for name in PREVIOUS_MODEL_FILES {
        let path = dir.join(name);
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            // A file that cannot be deleted stays; the next load tries again.
            let _ = fs::remove_file(&path);
        }
    }
}

/// What tells a file from another one on the same path: if none of this
/// changed, the bytes that were verified are still the bytes there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    device_and_inode: (u64, u64),
    /// Set by the system on every write, and not by resetting the mtime.
    #[cfg(unix)]
    changed: (i64, i64),
    #[cfg(windows)]
    created: u64,
}

impl FileIdentity {
    fn of(metadata: &fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            device_and_inode: {
                use std::os::unix::fs::MetadataExt as _;
                (metadata.dev(), metadata.ino())
            },
            #[cfg(unix)]
            changed: {
                use std::os::unix::fs::MetadataExt as _;
                (metadata.ctime(), metadata.ctime_nsec())
            },
            #[cfg(windows)]
            created: {
                use std::os::windows::fs::MetadataExt as _;
                metadata.creation_time()
            },
        }
    }
}

/// Files whose checksum matched in this process, with the identity they had.
/// Hashing 2.7 GB takes seconds, and the model is checked at every load.
fn verified_files() -> &'static Mutex<HashMap<PathBuf, FileIdentity>> {
    static VERIFIED: OnceLock<Mutex<HashMap<PathBuf, FileIdentity>>> = OnceLock::new();
    VERIFIED.get_or_init(Mutex::default)
}

#[cfg(test)]
thread_local! {
    /// Per thread, so tests that run side by side do not count each other.
    static HASHED_FILES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Checks that `path` is a regular file (not a link) with the size and the
/// checksum of `pin`. The file is hashed as a stream, never held in memory,
/// and only when it is not the file already verified at this path.
pub fn verify_model_file(path: &Path, pin: &ModelPin) -> Result<(), AiError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_unreadable| AiError::Model("the model file is not there"))?;
    if !metadata.is_file() {
        return Err(AiError::Model("the model file is not a regular file"));
    }
    if metadata.len() != pin.size {
        return Err(AiError::Model("the model file has the wrong size"));
    }
    let identity = FileIdentity::of(&metadata);
    let known = verified_files()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(path)
        == Some(&identity);
    if known {
        return Ok(());
    }
    hash_matches(path, pin, &identity)?;
    verified_files()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(path.to_path_buf(), identity);
    Ok(())
}

/// Hashes the file behind `path` and checks that the handle it read is the
/// file `identity` was taken from: a link or a file put in its place between
/// the check of the path and this open is not hashed as if it were the model.
fn hash_matches(path: &Path, pin: &ModelPin, identity: &FileIdentity) -> Result<(), AiError> {
    #[cfg(test)]
    HASHED_FILES.with(|count| count.set(count.get() + 1));
    let unreadable = |_err: std::io::Error| AiError::Model("the model file could not be read");
    let mut file = File::open(path).map_err(unreadable)?;
    let opened = file.metadata().map_err(unreadable)?;
    if FileIdentity::of(&opened) != *identity {
        return Err(AiError::Model(
            "the model file changed while it was checked",
        ));
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![u8::MIN; 1 << 20];
    let mut read_total: u64 = 0;
    loop {
        let read = file.read(&mut buffer).map_err(unreadable)?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
        read_total = read_total.saturating_add(read as u64);
        if read_total > pin.size {
            return Err(AiError::Model("the model file has the wrong size"));
        }
    }
    if hex_of(&hasher.finalize()) == pin.sha256 {
        Ok(())
    } else {
        Err(AiError::Model("the model file is not the expected one"))
    }
}

pub use ncrs_model_file::{has_partial_download, hex_of, ModelFileError, PartialDownload};

impl From<&ModelPin> for ncrs_model_file::PinnedFile {
    fn from(pin: &ModelPin) -> Self {
        Self {
            name: pin.name,
            size: pin.size,
            sha256: pin.sha256,
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
    use std::fs::OpenOptions;
    use std::io::Write as _;
    use std::sync::atomic::AtomicBool;

    use tempfile::TempDir;

    use super::*;

    fn pin_of(bytes: &[u8]) -> ModelPin {
        let sha256: &'static str = Box::leak(hex_of(&Sha256::digest(bytes)).into_boxed_str());
        ModelPin {
            name: "model.gguf",
            size: bytes.len() as u64,
            sha256,
            url: "https://example.invalid/model.gguf",
        }
    }

    fn write(dir: &TempDir, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.path().join("model.gguf");
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn the_right_bytes_are_accepted() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, b"model bytes");

        assert_eq!(verify_model_file(&path, &pin_of(b"model bytes")), Ok(()));
    }

    #[test]
    fn a_wrong_size_is_refused() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, b"model bytes and more");

        assert_eq!(
            verify_model_file(&path, &pin_of(b"model bytes")),
            Err(AiError::Model("the model file has the wrong size"))
        );
    }

    #[test]
    fn a_wrong_hash_is_refused() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, b"model BYTES");

        assert_eq!(
            verify_model_file(&path, &pin_of(b"model bytes")),
            Err(AiError::Model("the model file is not the expected one"))
        );
    }

    #[test]
    fn only_the_files_of_earlier_models_are_removed() {
        let dir = TempDir::new().unwrap();
        for name in PREVIOUS_MODEL_FILES {
            fs::write(dir.path().join(name), b"old").unwrap();
        }
        let current = dir.path().join(LOCAL_MODEL.name);
        let own = dir.path().join("my-model.gguf");
        fs::write(&current, b"new").unwrap();
        fs::write(&own, b"mine").unwrap();

        remove_previous_models(dir.path());

        for name in PREVIOUS_MODEL_FILES {
            assert!(!dir.path().join(name).exists(), "{name} is still there");
        }
        assert!(current.exists());
        assert!(own.exists());
    }

    #[test]
    fn a_missing_file_is_refused() {
        let dir = TempDir::new().unwrap();

        assert_eq!(
            verify_model_file(&dir.path().join("none.gguf"), &pin_of(b"x")),
            Err(AiError::Model("the model file is not there"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_the_right_bytes_is_refused() {
        let dir = TempDir::new().unwrap();
        let target = write(&dir, b"model bytes");
        let link = dir.path().join("link.gguf");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert_eq!(
            verify_model_file(&link, &pin_of(b"model bytes")),
            Err(AiError::Model("the model file is not a regular file"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_put_in_place_after_the_path_was_checked_is_not_hashed_as_the_model() {
        let dir = TempDir::new().unwrap();
        let checked = write(&dir, b"model bytes");
        let identity = FileIdentity::of(&fs::symlink_metadata(&checked).unwrap());
        let other = dir.path().join("other.gguf");
        fs::write(&other, b"model bytes").unwrap();
        fs::rename(&other, &checked).unwrap();

        assert_eq!(
            hash_matches(&checked, &pin_of(b"model bytes"), &identity),
            Err(AiError::Model(
                "the model file changed while it was checked"
            ))
        );
    }

    const BYTES: &[u8] = b"0123456789abcdefghij";

    fn never() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn part_in(dir: &TempDir) -> PathBuf {
        dir.path().join("model.gguf.part")
    }

    #[test]
    fn two_appends_with_a_reopen_between_them_give_the_right_file() {
        let dir = TempDir::new().unwrap();
        let pin = pin_of(BYTES);
        let mut first = PartialDownload::open(dir.path(), &pin).unwrap();
        first.append(&BYTES[..7], &never()).unwrap();
        assert_eq!(first.offset(), 7);
        drop(first);
        assert!(has_partial_download(dir.path(), &pin));

        let mut second = PartialDownload::open(dir.path(), &pin).unwrap();
        assert_eq!(second.offset(), 7);
        second.append(&BYTES[7..], &never()).unwrap();
        second.finish().unwrap();

        assert_eq!(fs::read(dir.path().join("model.gguf")).unwrap(), BYTES);
        assert!(!part_in(&dir).exists());
        assert_eq!(
            verify_model_file(&dir.path().join("model.gguf"), &pin),
            Ok(())
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_as_the_part_is_replaced_and_not_written_through() {
        let dir = TempDir::new().unwrap();
        let victim = dir.path().join("victim");
        fs::write(&victim, b"keep me").unwrap();
        std::os::unix::fs::symlink(&victim, part_in(&dir)).unwrap();
        let pin = pin_of(BYTES);

        let mut partial = PartialDownload::open(dir.path(), &pin).unwrap();
        partial.append(BYTES, &never()).unwrap();
        partial.finish().unwrap();

        assert_eq!(fs::read(&victim).unwrap(), b"keep me");
        assert_eq!(fs::read(dir.path().join("model.gguf")).unwrap(), BYTES);
    }

    #[cfg(unix)]
    #[test]
    fn a_model_folder_that_is_a_link_is_refused() {
        let dir = TempDir::new().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = dir.path().join("ai-models");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let opened = PartialDownload::open(&link, &pin_of(BYTES));

        assert!(matches!(opened, Err(ModelFileError::WriteFailed { .. })));
        assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_rewrite_in_place_with_the_mtime_put_back_is_hashed_again() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, b"model bytes");
        let pin = pin_of(b"model bytes");
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(verify_model_file(&path, &pin), Ok(()));

        // The change time of some file systems only moves every few
        // milliseconds.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        (&file).write_all(b"model BYTES").unwrap();
        file.set_modified(modified).unwrap();

        assert_eq!(
            verify_model_file(&path, &pin),
            Err(AiError::Model("the model file is not the expected one"))
        );
    }

    #[test]
    fn a_part_longer_than_the_pin_is_dropped() {
        let dir = TempDir::new().unwrap();
        fs::write(part_in(&dir), [b'x'; 64]).unwrap();

        let partial = PartialDownload::open(dir.path(), &pin_of(BYTES)).unwrap();

        assert_eq!(partial.offset(), 0);
        assert_eq!(fs::metadata(part_in(&dir)).unwrap().len(), 0);
    }

    #[test]
    fn a_wrong_hash_leaves_nothing() {
        let dir = TempDir::new().unwrap();
        let mut partial = PartialDownload::open(dir.path(), &pin_of(BYTES)).unwrap();
        partial
            .append(&b"0123456789abcdefghiX"[..], &never())
            .unwrap();

        assert_eq!(partial.finish(), Err(ModelFileError::WrongHash));

        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn more_than_the_pin_removes_the_part() {
        let dir = TempDir::new().unwrap();
        let mut partial = PartialDownload::open(dir.path(), &pin_of(BYTES)).unwrap();

        let more = [BYTES, b"!"].concat();
        assert_eq!(
            partial.append(more.as_slice(), &never()),
            Err(ModelFileError::TooLarge)
        );

        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_short_source_leaves_the_part_and_a_cancel_stops_before_the_next_chunk() {
        let dir = TempDir::new().unwrap();
        let mut partial = PartialDownload::open(dir.path(), &pin_of(BYTES)).unwrap();
        partial.append(&BYTES[..5], &never()).unwrap();
        assert_eq!(
            partial.append(&BYTES[5..], &AtomicBool::new(true)),
            Err(ModelFileError::Cancelled)
        );
        assert_eq!(partial.offset(), 5);

        assert_eq!(partial.finish(), Err(ModelFileError::Incomplete));

        assert_eq!(fs::read(part_in(&dir)).unwrap(), &BYTES[..5]);
    }

    #[test]
    fn restart_empties_the_part_and_the_hash() {
        let dir = TempDir::new().unwrap();
        let mut partial = PartialDownload::open(dir.path(), &pin_of(BYTES)).unwrap();
        partial.append(&b"wrong"[..], &never()).unwrap();
        partial.restart().unwrap();
        partial.append(BYTES, &never()).unwrap();

        assert_eq!(partial.finish(), Ok(()));
    }

    #[test]
    fn the_pinned_checksum_is_a_sha256_in_hex() {
        assert_eq!(LOCAL_MODEL.sha256.len(), 64);
        assert!(LOCAL_MODEL.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(LOCAL_MODEL.url.ends_with(LOCAL_MODEL.name));
    }

    #[test]
    fn the_fetch_script_pins_the_same_model_as_the_app() {
        let script = include_str!("../../../scripts/fetch-ai-model.sh");
        assert!(script.contains(&format!("NAME=\"{}\"", LOCAL_MODEL.name)));
        assert!(script.contains(&format!("SIZE={}", LOCAL_MODEL.size)));
        assert!(script.contains(&format!("SHA256=\"{}\"", LOCAL_MODEL.sha256)));
        let revision = LOCAL_MODEL
            .url
            .trim_end_matches(LOCAL_MODEL.name)
            .trim_end_matches('/');
        let pinned_to = revision.rsplit('/').next().unwrap_or_default();
        assert!(script.contains(pinned_to));
    }

    fn hashed_so_far() -> usize {
        HASHED_FILES.with(std::cell::Cell::get)
    }

    #[test]
    fn a_verified_file_is_hashed_once_and_a_changed_one_again() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, b"model bytes");
        let pin = pin_of(b"model bytes");
        let before = hashed_so_far();

        assert_eq!(verify_model_file(&path, &pin), Ok(()));
        assert_eq!(verify_model_file(&path, &pin), Ok(()));
        assert_eq!(hashed_so_far() - before, 1);

        let changed = OpenOptions::new().write(true).open(&path).unwrap();
        changed.set_len(0).unwrap();
        fs::write(&path, b"MODEL BYTES").unwrap();
        let later = fs::OpenOptions::new().write(true).open(&path).unwrap();
        later
            .set_modified(SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(
            verify_model_file(&path, &pin),
            Err(AiError::Model("the model file is not the expected one"))
        );
        assert_eq!(hashed_so_far() - before, 2);
    }
}
