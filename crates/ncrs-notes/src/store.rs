//! The notes folder on disk: reading it, naming files, writing them.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::frontmatter::{self, FrontMatter};

/// The folder of the notes on files; never shown as a folder.
pub const FILES_DIR: &str = ".files";

const MARKDOWN: &str = "md";
const MAX_DEPTH: usize = 16;
const MAX_TITLE_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A note of its own, in `folder` below the notes folder.
    Topic,
    /// A note on a file.
    File {
        file: PathBuf,
        document: Option<i64>,
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
    /// False for a note made in the tab and not written yet.
    pub on_disk: bool,
}

impl Note {
    /// What goes into the file.
    pub fn file_text(&self) -> String {
        match &self.kind {
            Kind::Topic => self.text.clone(),
            Kind::File { file, document } => frontmatter::compose(
                &FrontMatter {
                    file: file.clone(),
                    document: *document,
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
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(MARKDOWN))
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn read_note(path: &Path, folder: PathBuf, files_dir: bool) -> Option<Note> {
    let text = fs::read_to_string(path).ok()?;
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
            },
            text: body.to_owned(),
            modified,
            on_disk: true,
        });
    }
    Some(Note {
        path: path.to_owned(),
        title: stem_of(path),
        folder,
        kind: Kind::Topic,
        text,
        modified,
        on_disk: true,
    })
}

fn walk(root: &Path, relative: &Path, depth: usize, scan: &mut Scan) {
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
                walk(root, &below, depth + 1, scan);
            }
        } else if !kind.is_dir() && is_markdown(&entry.path()) {
            scan.notes
                .extend(read_note(&entry.path(), relative.to_owned(), false));
        }
    }
}

/// Reads the notes folder. A folder that is not there yet is an empty one.
pub fn scan(root: &Path) -> Result<Scan, String> {
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
    walk(root, Path::new(""), 0, &mut scan);
    if let Ok(entries) = fs::read_dir(root.join(FILES_DIR)) {
        for entry in entries.flatten() {
            let path = entry.path();
            if is_markdown(&path) {
                scan.notes.extend(read_note(&path, PathBuf::new(), true));
            }
        }
    }
    Ok(scan)
}

/// FNV-1a, so the name of a note on a file stays the same across versions of
/// the program and of Rust.
fn short_hash(path: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.to_string_lossy().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:08x}", hash >> 32)
}

/// Where the note on `file` lives.
pub fn file_note_path(root: &Path, file: &Path) -> PathBuf {
    let name = file.file_name().map_or_else(
        || "file".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    root.join(FILES_DIR)
        .join(format!("{name}-{}.{MARKDOWN}", short_hash(file)))
}

/// A title that is a safe file name.
pub fn clean_title(title: &str) -> String {
    let cleaned: String = title
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
    cleaned.trim().trim_start_matches('.').trim().to_owned()
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

/// Writes `content` next to `path` and moves it into place, so a crash leaves
/// the old file or the new one.
pub fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let describe = |err: std::io::Error| format!("{}: {err}", path.display());
    let folder = path.parent().ok_or_else(|| path.display().to_string())?;
    fs::create_dir_all(folder).map_err(describe)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let partial = folder.join(format!(".{name}.partial"));
    let mut file = fs::File::create(&partial).map_err(describe)?;
    file.write_all(content.as_bytes()).map_err(describe)?;
    file.sync_all().map_err(describe)?;
    fs::rename(&partial, path).map_err(describe)
}

/// What the blocking task of a write does: the file, then removing the one it
/// replaces when a note was renamed.
pub fn write_note(path: &Path, content: &str, replaces: Option<&Path>) -> Result<(), String> {
    write_atomic(path, content)?;
    if let Some(old) = replaces.filter(|old| *old != path) {
        match fs::remove_file(old) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(format!("{}: {err}", old.display())),
        }
    }
    Ok(())
}

pub fn make_folder(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|err| format!("{}: {err}", path.display()))
}
