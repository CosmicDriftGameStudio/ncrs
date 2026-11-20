//! The notes folder on disk: reading it, naming files, writing them.

use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use crate::crypt::{self, is_encrypted_path, NoteKey, ENCRYPTED_SUFFIX};
use crate::frontmatter::{self, FrontMatter};
use crate::state::Write;

/// The folder of the notes on files; never shown as a folder.
pub const FILES_DIR: &str = ".files";

const MARKDOWN: &str = "md";
const MAX_DEPTH: usize = 16;
const MAX_TITLE_CHARS: usize = 80;
/// File systems cap a name at 255 bytes; a title leaves room for a number and the extension.
const MAX_TITLE_BYTES: usize = 200;
const MAX_NAME_BYTES: usize = 255;
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A note of its own, in `folder` below the notes folder.
    Topic,
    /// A note on a file.
    File {
        file: PathBuf,
        document: Option<i64>,
        hash: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub path: PathBuf,
    pub title: String,
    /// Below the notes folder; empty at its top and for notes on files.
    pub folder: PathBuf,
    pub kind: Kind,
    /// Without the header of a note on a file.
    pub text: String,
    pub modified: Option<SystemTime>,
    /// The hash of the bytes this note was read from or last written as;
    /// `None` for a note made in the tab and not written yet.
    pub base: Option<u64>,
}

impl Note {
    /// What goes into the file.
    pub fn file_text(&self) -> String {
        match &self.kind {
            Kind::Topic => self.text.clone(),
            Kind::File {
                file,
                document,
                hash,
            } => frontmatter::compose(
                &FrontMatter {
                    file: file.clone(),
                    document: *document,
                    hash: hash.clone(),
                },
                &self.text,
            ),
        }
    }

    /// The first line that says something, without Markdown heading marks.
    pub fn first_line(&self) -> Option<&str> {
        self.text
            .lines()
            .map(|line| line.trim_start_matches('#').trim())
            .find(|line| !line.is_empty())
    }
}

/// The notes folder as read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scan {
    pub root: PathBuf,
    pub notes: Vec<Note>,
    /// Below the notes folder, `.files` and dot folders left out.
    pub folders: Vec<PathBuf>,
    /// How many writes had been asked for when the read started; a read that
    /// started before a later write may not have seen it.
    pub epoch: u64,
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(MARKDOWN))
        || is_encrypted_path(path)
}

fn stem_of(path: &Path) -> String {
    if is_encrypted_path(path) {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        return name
            .get(..name.len().saturating_sub(ENCRYPTED_SUFFIX.len()))
            .unwrap_or_default()
            .to_owned();
    }
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// An encrypted note read without the key has no text; one the key does not
/// open is left out, so nothing can overwrite it by mistake.
fn read_note(path: &Path, folder: PathBuf, files_dir: bool, key: Option<&NoteKey>) -> Option<Note> {
    let bytes = fs::read(path).ok()?;
    let base = Some(content_hash(&bytes));
    let text = if is_encrypted_path(path) {
        match key {
            Some(note_key) => crypt::decrypt(note_key, &bytes).ok()?,
            None => String::new(),
        }
    } else {
        String::from_utf8(bytes).ok()?
    };
    let modified = fs::metadata(path).and_then(|meta| meta.modified()).ok();
    if files_dir {
        let (parsed, body) = frontmatter::split(&text);
        let front = parsed?;
        let title = front
            .file
            .file_name()
            .map_or_else(|| stem_of(path), |name| name.to_string_lossy().into_owned());
        return Some(Note {
            path: path.to_owned(),
            title,
            folder: PathBuf::new(),
            kind: Kind::File {
                file: front.file,
                document: front.document,
                hash: front.hash,
            },
            text: body.to_owned(),
            modified,
            base,
        });
    }
    Some(Note {
        path: path.to_owned(),
        title: stem_of(path),
        folder,
        kind: Kind::Topic,
        text,
        modified,
        base,
    })
}

fn walk(root: &Path, relative: &Path, depth: usize, scan: &mut Scan, key: Option<&NoteKey>) {
    let Ok(entries) = fs::read_dir(root.join(relative)) else {
        return;
    };
    let mut listed: Vec<_> = entries.flatten().collect();
    listed.sort_by_key(fs::DirEntry::file_name);
    for entry in listed {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let below = relative.join(&name);
        if kind.is_dir() {
            if depth < MAX_DEPTH {
                scan.folders.push(below.clone());
                walk(root, &below, depth + 1, scan, key);
            }
        } else if !kind.is_dir() && is_markdown(&entry.path()) {
            scan.notes
                .extend(read_note(&entry.path(), relative.to_owned(), false, key));
        }
    }
}

/// Reads the notes folder without the key: encrypted notes are listed with no text.
pub fn scan(root: &Path) -> Result<Scan, String> {
    scan_with(root, None)
}

/// Reads the notes folder. A folder that is not there yet is an empty one.
pub fn scan_with(root: &Path, key: Option<&NoteKey>) -> Result<Scan, String> {
    let mut scan = Scan {
        root: root.to_owned(),
        ..Scan::default()
    };
    if !root.exists() {
        return Ok(scan);
    }
    if !root.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    walk(root, Path::new(""), 0, &mut scan, key);
    if let Ok(entries) = fs::read_dir(root.join(FILES_DIR)) {
        for entry in entries.flatten() {
            let path = entry.path();
            if is_markdown(&path) {
                scan.notes
                    .extend(read_note(&path, PathBuf::new(), true, key));
            }
        }
    }
    Ok(scan)
}

/// FNV-1a, so the name of a note on a file stays the same across versions of
/// the program and of Rust.
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn short_hash(path: &Path) -> String {
    format!(
        "{:08x}",
        content_hash(path.to_string_lossy().as_bytes()) >> 32
    )
}

/// The longest start of `text` that is at most `bytes` long and ends at a character border.
fn cut_to_bytes(text: &str, bytes: usize) -> &str {
    let mut end = bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    text.get(..end).unwrap_or_default()
}

/// Where the note on `file` lives.
pub fn file_note_path(root: &Path, file: &Path) -> PathBuf {
    let full_name = file.file_name().map_or_else(
        || "file".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let suffix = format!("-{}.{MARKDOWN}", short_hash(file));
    let name = cut_to_bytes(&full_name, MAX_NAME_BYTES - suffix.len());
    root.join(FILES_DIR).join(format!("{name}{suffix}"))
}

/// A title that is a safe file name.
pub fn clean_title(title: &str) -> String {
    let limited: String = title
        .chars()
        .map(|c| {
            if "/\\:*?\"<>|".contains(c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .take(MAX_TITLE_CHARS)
        .collect();
    let cleaned = cut_to_bytes(&limited, MAX_TITLE_BYTES)
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .trim()
        .to_owned();
    // Windows cannot create these names, with or without an extension.
    let stem = cleaned.split('.').next().unwrap_or_default();
    if RESERVED_NAMES
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(stem))
    {
        return format!("{stem}_{}", &cleaned[stem.len()..]);
    }
    cleaned
}

/// `title` with a number when `taken` says the name is in use.
pub fn unique_title(title: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(title) {
        return title.to_owned();
    }
    (2..)
        .map(|number| format!("{title} {number}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| title.to_owned())
}

pub fn topic_path(root: &Path, folder: &Path, title: &str) -> PathBuf {
    root.join(folder).join(format!("{title}.{MARKDOWN}"))
}

/// As [`topic_path`], for an encrypted note when `sealed`.
pub fn note_path(root: &Path, folder: &Path, title: &str, sealed: bool) -> PathBuf {
    let path = topic_path(root, folder, title);
    if sealed {
        sealed_path(&path)
    } else {
        path
    }
}

/// The disk state a write is based on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// There is no file at the path.
    Absent,
    /// The file there has the content with this hash.
    Content(u64),
}

impl Expect {
    pub fn of(base: Option<u64>) -> Self {
        base.map_or(Self::Absent, Self::Content)
    }
}

/// The file a rename leaves behind, and what was last known to be in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replaced {
    pub path: PathBuf,
    pub base: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// The file is not what the write was based on; nothing was written.
    Conflict(PathBuf),
    Io(String),
}

/// Where a path that is a link leads; links are written through, not replaced,
/// but only when they lead inside the notes folder `root`.
fn followed(path: &Path, root: &Path) -> std::io::Result<PathBuf> {
    let is_link = fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink());
    if !is_link {
        return Ok(path.to_owned());
    }
    let Ok(target) = fs::canonicalize(path) else {
        return Ok(path.to_owned());
    };
    if target.starts_with(fs::canonicalize(root)?) {
        Ok(target)
    } else {
        Err(std::io::Error::new(
            ErrorKind::PermissionDenied,
            "the link leads out of the notes folder",
        ))
    }
}

#[cfg(unix)]
fn new_file_builder() -> tempfile::Builder<'static, 'static> {
    use std::os::unix::fs::PermissionsExt as _;
    let mut builder = tempfile::Builder::new();
    // tempfile defaults to 0600; asking for 0666 gets the umask applied, so a
    // new note is readable like any other new file.
    builder.permissions(fs::Permissions::from_mode(0o666));
    builder
}

#[cfg(not(unix))]
fn new_file_builder() -> tempfile::Builder<'static, 'static> {
    tempfile::Builder::new()
}

/// Writes `content` next to `path` and moves it into place, so a crash leaves
/// the old file or the new one. Refuses when the disk does not match `expect`.
/// Returns the hash of what was written.
fn write_atomic(
    path: &Path,
    root: &Path,
    content: &[u8],
    expect: Expect,
) -> Result<u64, WriteError> {
    let describe = |err: std::io::Error| WriteError::Io(format!("{}: {err}", path.display()));
    let target = followed(path, root).map_err(describe)?;
    let folder = target
        .parent()
        .ok_or_else(|| WriteError::Io(path.display().to_string()))?;
    fs::create_dir_all(folder).map_err(describe)?;
    let existing = match fs::read(&target) {
        Ok(bytes) => Some(bytes),
        Err(err) if err.kind() == ErrorKind::NotFound => None,
        Err(err) => return Err(describe(err)),
    };
    let expected = match (&existing, expect) {
        (None, _) => true,
        (Some(_), Expect::Absent) => false,
        (Some(bytes), Expect::Content(hash)) => content_hash(bytes) == hash,
    };
    if !expected {
        return Err(WriteError::Conflict(path.to_owned()));
    }
    let builder = if existing.is_some() {
        tempfile::Builder::new()
    } else {
        new_file_builder()
    };
    let mut file = builder.tempfile_in(folder).map_err(describe)?;
    file.write_all(content).map_err(describe)?;
    file.as_file().sync_all().map_err(describe)?;
    if existing.is_some() {
        let rights = fs::metadata(&target).map_err(describe)?.permissions();
        file.as_file().set_permissions(rights).map_err(describe)?;
        file.persist(&target).map_err(|err| describe(err.error))?;
    } else {
        persist_new(file, &target, path)?;
    }
    Ok(content_hash(content))
}

/// Moves the temporary file to a name nobody has taken; a file system that
/// cannot refuse an existing name is checked first, which leaves a small gap.
fn persist_new(
    file: tempfile::NamedTempFile,
    target: &Path,
    shown: &Path,
) -> Result<(), WriteError> {
    let describe = |err: std::io::Error| WriteError::Io(format!("{}: {err}", shown.display()));
    match file.persist_noclobber(target) {
        Ok(_) => Ok(()),
        Err(err) if err.error.kind() == ErrorKind::AlreadyExists => {
            Err(WriteError::Conflict(shown.to_owned()))
        }
        Err(err) if err.error.kind() == ErrorKind::Unsupported => {
            if fs::symlink_metadata(target).is_ok() {
                return Err(WriteError::Conflict(shown.to_owned()));
            }
            err.file
                .persist(target)
                .map(drop)
                .map_err(|failed| describe(failed.error))
        }
        Err(err) => Err(describe(err.error)),
    }
}

/// Whether both names lead to one file: on a case-insensitive disk a title that
/// only changes its case names the note's own file.
#[cfg(unix)]
pub fn is_same_file(first: &Path, second: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    match (fs::metadata(first), fs::metadata(second)) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => false,
    }
}

/// Windows has no inode; its canonical path carries the case stored on disk.
#[cfg(not(unix))]
pub fn is_same_file(first: &Path, second: &Path) -> bool {
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// What was last written to a note file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Written {
    pub generation: u64,
    pub hash: u64,
}

/// The newest write done per note file, shared with the blocking tasks so a
/// write that was slow cannot land over a newer one, and so a write is checked
/// against what this program wrote last rather than against a stale base.
pub type Ledger = Arc<Mutex<HashMap<PathBuf, Written>>>;

/// What goes into the file of `write`: the text, or for an encrypted note the
/// sealed text. An encrypted note is never written without the key, so the
/// plaintext cannot reach the disk under that name.
fn bytes_of(write: &Write) -> Result<Vec<u8>, WriteError> {
    if !is_encrypted_path(&write.path) {
        return Ok(write.text.clone().into_bytes());
    }
    let describe = |reason: String| WriteError::Io(format!("{}: {reason}", write.path.display()));
    let key = write
        .key
        .as_deref()
        .ok_or_else(|| describe("locked".to_owned()))?;
    crypt::encrypt(key, &write.text).map_err(|err| describe(err.to_string()))
}

/// The path of `path` as an encrypted note: `Name.md` is `Name.md.enc`.
pub fn sealed_path(path: &Path) -> PathBuf {
    if is_encrypted_path(path) {
        return path.to_owned();
    }
    let mut name = path.as_os_str().to_owned();
    name.push(".enc");
    PathBuf::from(name)
}

/// The path of `path` as a plain note: `Name.md.enc` is `Name.md`.
pub fn unsealed_path(path: &Path) -> PathBuf {
    if !is_encrypted_path(path) {
        return path.to_owned();
    }
    let text = path.to_string_lossy();
    let kept = text.len().saturating_sub(".enc".len());
    PathBuf::from(text.get(..kept).unwrap_or_default())
}

/// What the blocking task of a write does: the file, then removing the one it
/// replaces when a note was renamed. A write older than one already done for
/// the same file is not written, but the removal still happens. Returns the
/// hash of what the file holds now.
pub fn write_note(write: &Write, ledger: &Ledger) -> Result<u64, WriteError> {
    let mut done = ledger.lock().unwrap_or_else(PoisonError::into_inner);
    let path = write.path.as_path();
    // A case-only rename names the file it replaces.
    let other_name_of_path = write
        .replaces
        .as_ref()
        .filter(|old| old.path != path && is_same_file(&old.path, path));
    let hash = match done.get(path).copied() {
        Some(last) if write.generation <= last.generation => last.hash,
        known => {
            let expect = known
                .or_else(|| done.get(&other_name_of_path?.path).copied())
                .map_or(write.expect, |last| Expect::Content(last.hash));
            let content = bytes_of(write)?;
            let hash = write_atomic(path, &write.root, &content, expect)?;
            done.insert(
                path.to_owned(),
                Written {
                    generation: write.generation,
                    hash,
                },
            );
            hash
        }
    };
    if let Some(old) = write
        .replaces
        .as_ref()
        .filter(|old| old.path != path && other_name_of_path.is_none())
    {
        remove_replaced(old, &mut done)?;
    }
    Ok(hash)
}

/// Removes the file a rename left behind, unless it changed since this program
/// last saw it: that version stays.
fn remove_replaced(old: &Replaced, done: &mut HashMap<PathBuf, Written>) -> Result<(), WriteError> {
    let describe = |err: std::io::Error| WriteError::Io(format!("{}: {err}", old.path.display()));
    let known = done.get(&old.path).map(|last| last.hash).or(old.base);
    match fs::read(&old.path) {
        Ok(bytes) if Some(content_hash(&bytes)) == known => {}
        Ok(_) => return Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(describe(err)),
    }
    match fs::remove_file(&old.path) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(describe(err)),
    }
    done.remove(&old.path);
    Ok(())
}

/// Whether something is at `path`, a link that leads nowhere included.
pub fn is_taken(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

pub fn make_folder(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|err| format!("{}: {err}", path.display()))
}
