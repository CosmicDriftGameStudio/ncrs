//! The folder structure of an archive, read once and then answered from memory.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use zip::result::ZipError;
use zip::ZipArchive;

use super::names::safe_components;
use super::sevenz;
use super::ArchiveFormat;

/// An archive with more entries than this is refused rather than indexed: the
/// index lives in memory.
pub const MAX_ARCHIVE_ENTRIES: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// What an entry is, as far as unpacking cares. Links and special files are
/// never created, so they all fall into `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    File,
    Other,
}

impl EntryKind {
    pub fn of_tar(kind: tar::EntryType) -> Self {
        match kind {
            tar::EntryType::Regular | tar::EntryType::Continuous | tar::EntryType::GNUSparse => {
                Self::File
            }
            tar::EntryType::Directory => Self::Dir,
            _ => Self::Other,
        }
    }
}

/// Files, folders and bytes at and below one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    pub files: usize,
    pub folders: usize,
    pub bytes: u64,
}

impl Totals {
    pub fn entries(&self) -> usize {
        self.files + self.folders
    }

    fn add(&mut self, node: &Node) {
        if node.is_dir {
            self.folders += 1;
        } else {
            self.files += 1;
            self.bytes = self.bytes.saturating_add(node.size);
        }
    }
}

#[derive(Debug, Default)]
pub struct ArchiveIndex {
    /// Folder key (`a/b`, the root is `""`) to the entries inside it. Names
    /// never contain `/`, so a key is unambiguous.
    children: HashMap<String, BTreeMap<String, Node>>,
    /// Entries refused for an unsafe path, a link or a special file.
    left_out: usize,
    seen: usize,
}

impl ArchiveIndex {
    pub const fn left_out(&self) -> usize {
        self.left_out
    }

    pub fn build(path: &Path, format: ArchiveFormat) -> io::Result<Self> {
        match format {
            ArchiveFormat::Zip => build_zip(path),
            ArchiveFormat::SevenZip => build_sevenz(path),
            ArchiveFormat::Tar | ArchiveFormat::TarGz | ArchiveFormat::TarZst => {
                build_tar(path, format)
            }
        }
    }

    pub fn children_of(&self, inner: &[String]) -> Option<&BTreeMap<String, Node>> {
        self.children.get(&inner.join("/"))
    }

    pub fn node(&self, inner: &[String]) -> Option<Node> {
        let Some((name, parents)) = inner.split_last() else {
            return Some(Node {
                is_dir: true,
                size: 0,
                modified: None,
            });
        };
        self.children.get(&parents.join("/"))?.get(name).copied()
    }

    /// The counts for `inner` and everything below it, itself included.
    pub fn totals(&self, inner: &[String]) -> Option<Totals> {
        let node = self.node(inner)?;
        let mut totals = Totals::default();
        totals.add(&node);
        let mut pending = if node.is_dir {
            vec![inner.join("/")]
        } else {
            Vec::new()
        };
        while let Some(key) = pending.pop() {
            let Some(entries) = self.children.get(&key) else {
                continue;
            };
            for (name, child) in entries {
                totals.add(child);
                if child.is_dir {
                    pending.push(if key.is_empty() {
                        name.clone()
                    } else {
                        format!("{key}/{name}")
                    });
                }
            }
        }
        Some(totals)
    }

    fn count_entry(&mut self) -> io::Result<()> {
        self.seen += 1;
        if self.seen > MAX_ARCHIVE_ENTRIES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("the archive has more than {MAX_ARCHIVE_ENTRIES} entries"),
            ));
        }
        Ok(())
    }

    fn leave_out(&mut self) -> io::Result<()> {
        self.count_entry()?;
        self.left_out += 1;
        Ok(())
    }

    /// Adds an entry and the folders above it. The later of two entries with
    /// one name wins, as when unpacking in order.
    fn add(&mut self, components: &[String], node: Node) -> io::Result<()> {
        self.count_entry()?;
        let Some((name, parents)) = components.split_last() else {
            return Ok(());
        };
        let mut key = String::new();
        for parent in parents {
            self.children
                .entry(key.clone())
                .or_default()
                .entry(parent.clone())
                .and_modify(|existing| existing.is_dir = true)
                .or_insert(Node {
                    is_dir: true,
                    size: 0,
                    modified: None,
                });
            if !key.is_empty() {
                key.push('/');
            }
            key.push_str(parent);
        }
        self.children
            .entry(key)
            .or_default()
            .insert(name.clone(), node);
        Ok(())
    }
}

pub fn zip_error(error: ZipError) -> io::Error {
    match error {
        ZipError::Io(inner) => inner,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

pub fn open_zip(path: &Path) -> io::Result<ZipArchive<BufReader<File>>> {
    ZipArchive::new(BufReader::new(File::open(path)?)).map_err(zip_error)
}

/// The tar stream behind any of the tar formats, decompressed on the fly.
pub fn open_tar(path: &Path, format: ArchiveFormat) -> io::Result<tar::Archive<Box<dyn Read>>> {
    let file = BufReader::new(File::open(path)?);
    let stream: Box<dyn Read> = match format {
        ArchiveFormat::TarGz => Box::new(flate2::read::MultiGzDecoder::new(file)),
        ArchiveFormat::TarZst => Box::new(zstd::stream::read::Decoder::with_buffer(file)?),
        ArchiveFormat::Tar | ArchiveFormat::Zip | ArchiveFormat::SevenZip => Box::new(file),
    };
    Ok(tar::Archive::new(stream))
}

/// DOS times carry no zone, so they are read as local time.
pub fn zip_time(stamp: Option<zip::DateTime>) -> Option<SystemTime> {
    let time = stamp?;
    let local = chrono::NaiveDate::from_ymd_opt(
        i32::from(time.year()),
        u32::from(time.month()),
        u32::from(time.day()),
    )?
    .and_hms_opt(
        u32::from(time.hour()),
        u32::from(time.minute()),
        u32::from(time.second()),
    )?
    .and_local_timezone(chrono::Local)
    .earliest()?;
    Some(SystemTime::from(local))
}

pub fn tar_time(stored: io::Result<u64>) -> Option<SystemTime> {
    stored
        .ok()
        .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds))
}

fn build_zip(path: &Path) -> io::Result<ArchiveIndex> {
    let mut zip = open_zip(path)?;
    let mut index = ArchiveIndex::default();
    for position in 0..zip.len() {
        let entry = zip.by_index_raw(position).map_err(zip_error)?;
        let safe = entry
            .name()
            .ok()
            .and_then(|name| safe_components(&name))
            .filter(|_| !entry.is_symlink());
        match safe {
            Some(components) => index.add(
                &components,
                Node {
                    is_dir: entry.is_dir(),
                    size: if entry.is_dir() { 0 } else { entry.size() },
                    modified: zip_time(entry.last_modified()),
                },
            )?,
            None => index.leave_out()?,
        }
    }
    Ok(index)
}

fn build_sevenz(path: &Path) -> io::Result<ArchiveIndex> {
    let reader = sevenz::open(path)?;
    let mut index = ArchiveIndex::default();
    for entry in &reader.archive().files {
        let kind = sevenz::kind_of(entry);
        match safe_components(entry.name()) {
            Some(components) if kind != EntryKind::Other => index.add(
                &components,
                Node {
                    is_dir: kind == EntryKind::Dir,
                    size: if kind == EntryKind::Dir {
                        0
                    } else {
                        entry.size()
                    },
                    modified: sevenz::modified(entry),
                },
            )?,
            _ => index.leave_out()?,
        }
    }
    Ok(index)
}

fn build_tar(path: &Path, format: ArchiveFormat) -> io::Result<ArchiveIndex> {
    let mut archive = open_tar(path, format)?;
    let mut index = ArchiveIndex::default();
    for item in archive.entries()? {
        let entry = item?;
        let header = entry.header();
        if header.entry_type() == tar::EntryType::XGlobalHeader {
            continue;
        }
        let kind = EntryKind::of_tar(header.entry_type());
        let safe = safe_components(&String::from_utf8_lossy(&entry.path_bytes()));
        match safe {
            Some(components) if kind != EntryKind::Other => index.add(
                &components,
                Node {
                    is_dir: kind == EntryKind::Dir,
                    size: if kind == EntryKind::Dir {
                        0
                    } else {
                        header.size()?
                    },
                    modified: tar_time(header.mtime()),
                },
            )?,
            _ => index.leave_out()?,
        }
    }
    Ok(index)
}
