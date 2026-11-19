//! A model file pinned by size and checksum, downloaded into `<name>.part`
//! next to its target and renamed into place only when it matches. Used by every
//! model download, so the checks (link refusal, size cap, hash, resume) exist once.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest as _, Sha256};

/// What a model file must be: its name and the size and checksum nothing is
/// trusted before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

const COPY_CHUNK: usize = 64 * 1024;
const PART_SUFFIX: &str = ".part";

/// Why a model file could not be put in place. No secrets; paths are the
/// user's own data folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelFileError {
    /// The user stopped the download; the partial file stays for the next try.
    Cancelled,
    /// The source ended before the pinned size; the partial file stays.
    Incomplete,
    /// The source delivers more than the pinned size; the partial file is gone.
    TooLarge,
    WrongSize,
    WrongHash,
    /// Reading the source failed (for example the connection broke off); the
    /// partial file stays.
    Interrupted(String),
    WriteFailed {
        path: PathBuf,
        reason: String,
    },
    NotARegularFile,
}

impl fmt::Display for ModelFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => write!(f, "stopped"),
            Self::Incomplete => write!(f, "the download ended too early"),
            Self::TooLarge => write!(f, "larger than expected"),
            Self::WrongSize => write!(f, "the file has the wrong size"),
            Self::WrongHash => write!(f, "the file does not match its checksum"),
            Self::Interrupted(reason) => write!(f, "the download broke off: {reason}"),
            Self::WriteFailed { path, reason } => {
                write!(f, "could not write {}: {reason}", path.display())
            }
            Self::NotARegularFile => write!(f, "a part of the model path is not a regular file"),
        }
    }
}

impl std::error::Error for ModelFileError {}

/// Whether an earlier download left bytes for `pin` in `dir`.
pub fn has_partial_download(dir: &Path, pin: impl Into<PinnedFile>) -> bool {
    fs::symlink_metadata(part_path(dir, &pin.into()))
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

fn part_path(dir: &Path, pin: &PinnedFile) -> PathBuf {
    dir.join(format!("{}{PART_SUFFIX}", pin.name))
}

/// The model file while it is downloaded: `<name>.part` next to the target,
/// hashed as it grows. A download that broke off or was stopped continues from
/// the bytes already there; the file is renamed into place only when size and
/// checksum match.
#[derive(Debug)]
pub struct PartialDownload {
    pin: PinnedFile,
    part_path: PathBuf,
    target_path: PathBuf,
    file: File,
    hasher: Sha256,
    written: u64,
}

impl PartialDownload {
    /// Keeps a regular `.part` that is not longer than the pin; anything else
    /// (a link, a file that is too long) is removed and the download starts at
    /// zero. A link is never written through.
    pub fn open(dir: &Path, pinned: impl Into<PinnedFile>) -> Result<Self, ModelFileError> {
        let pin = pinned.into();
        let part_path = part_path(dir, &pin);
        let write_failed = |target: &Path| {
            let path = target.to_path_buf();
            move |err: io::Error| ModelFileError::WriteFailed {
                path,
                reason: err.to_string(),
            }
        };
        fs::create_dir_all(dir).map_err(write_failed(dir))?;
        let folder = fs::symlink_metadata(dir).map_err(write_failed(dir))?;
        if !folder.is_dir() {
            return Err(ModelFileError::WriteFailed {
                path: dir.to_path_buf(),
                reason: "the model folder is a link, not a folder".to_owned(),
            });
        }
        let keep = match fs::symlink_metadata(&part_path) {
            Ok(metadata) if metadata.is_file() && metadata.len() <= pin.size => true,
            Ok(metadata) if metadata.is_dir() => return Err(ModelFileError::NotARegularFile),
            Ok(_) => {
                fs::remove_file(&part_path).map_err(write_failed(&part_path))?;
                false
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => false,
            Err(err) => return Err(write_failed(&part_path)(err)),
        };
        let mut file = OpenOptions::new()
            .read(true)
            // Not `append`: on Windows an append-only handle may not shorten
            // the file, and `restart` does. Reading the kept bytes below leaves
            // the cursor at the end, where the next bytes go.
            .write(true)
            .create_new(!keep)
            .open(&part_path)
            .map_err(write_failed(&part_path))?;
        let mut hasher = Sha256::new();
        let mut written: u64 = 0;
        if keep {
            let mut buffer = vec![u8::MIN; COPY_CHUNK];
            loop {
                let read = file.read(&mut buffer).map_err(write_failed(&part_path))?;
                if read == 0 {
                    break;
                }
                hasher.update(buffer.get(..read).unwrap_or_default());
                written = written.saturating_add(read as u64);
            }
        }
        let mut partial = Self {
            pin,
            part_path,
            target_path: dir.join(pin.name),
            file,
            hasher,
            written,
        };
        if partial.written > pin.size {
            partial.restart()?;
        }
        Ok(partial)
    }

    /// The bytes already there, which the next ones continue.
    pub fn offset(&self) -> u64 {
        self.written
    }

    /// Throws away what is there and starts at zero.
    pub fn restart(&mut self) -> Result<(), ModelFileError> {
        self.file
            .set_len(0)
            .map_err(|err| self.write_failed(&err))?;
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|err| self.write_failed(&err))?;
        self.hasher = Sha256::new();
        self.written = 0;
        Ok(())
    }

    /// Appends what `source` delivers, `COPY_CHUNK` bytes at a time, and looks
    /// at `cancel` before each chunk. More than the pin allows removes the
    /// partial file.
    pub fn append(
        &mut self,
        mut source: impl Read,
        cancel: &AtomicBool,
    ) -> Result<(), ModelFileError> {
        let mut buffer = vec![u8::MIN; COPY_CHUNK];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ModelFileError::Cancelled);
            }
            let read = source
                .read(&mut buffer)
                .map_err(|err| ModelFileError::Interrupted(err.to_string()))?;
            if read == 0 {
                break;
            }
            if self.written.saturating_add(read as u64) > self.pin.size {
                let _ = fs::remove_file(&self.part_path);
                return Err(ModelFileError::TooLarge);
            }
            let chunk = buffer.get(..read).unwrap_or_default();
            self.file
                .write_all(chunk)
                .map_err(|err| self.write_failed(&err))?;
            self.hasher.update(chunk);
            self.written = self.written.saturating_add(read as u64);
        }
        self.file.sync_all().map_err(|err| self.write_failed(&err))
    }

    /// Checks size and checksum and moves the file into place. A file that
    /// fails the checksum is removed; one that is too short stays to be
    /// continued.
    pub fn finish(self) -> Result<(), ModelFileError> {
        if self.written != self.pin.size {
            return Err(ModelFileError::Incomplete);
        }
        if hex_of(&self.hasher.clone().finalize()) != self.pin.sha256 {
            let _ = fs::remove_file(&self.part_path);
            return Err(ModelFileError::WrongHash);
        }
        fs::rename(&self.part_path, &self.target_path).map_err(|err| self.write_failed(&err))
    }

    fn write_failed(&self, err: &io::Error) -> ModelFileError {
        ModelFileError::WriteFailed {
            path: self.part_path.clone(),
            reason: err.to_string(),
        }
    }
}

pub fn hex_of(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
