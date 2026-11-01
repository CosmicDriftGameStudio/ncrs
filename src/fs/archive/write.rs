//! Writing archives: packing files into a new one, adding to an existing one
//! and deleting from it.
//!
//! No format can be edited in place, so `add` and `delete` write a complete
//! new archive into a hidden file next to the old one and replace the old one
//! by renaming it over. The file is created by `tempfile` (random name,
//! exclusive create) in the archive's own folder and removed again on every
//! error and cancel; the original is not touched before the rename.

use std::collections::HashSet;
use std::fs::{File, Metadata};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Datelike as _, Timelike as _};
use tempfile::NamedTempFile;

use super::extract::CHUNK;
use super::index::{open_tar, open_zip, zip_error};
use super::names::safe_components;
use super::sevenz;
use super::{forget, index_of, locate_in_archive, ArchiveFormat, ArchiveIndex};
use crate::fs::remote::RemotePath;
use crate::fs::transfer::{OnConflict, Progress};

/// What a write did besides the entries it wrote.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WriteReport {
    /// Entries now in the archive that this call wrote or kept.
    pub entries: usize,
    /// Links and special files (and names that are not UTF-8) left out of the
    /// selection.
    pub skipped: usize,
    /// Sources not added because the name was already in the archive.
    pub kept_existing: usize,
}

/// A new archive at `archive` holding `sources`, each under its own name.
/// Fails with `AlreadyExists` when `archive` is there; nothing is created
/// unless the whole archive was written.
pub fn pack<P: Progress>(
    sources: &[PathBuf],
    archive: &Path,
    progress: &mut P,
) -> io::Result<WriteReport> {
    let mut report = WriteReport::default();
    let items = named_by_file_name(sources, &mut report)?;
    pack_planned(&items, archive, progress, report)
}

/// Like [`pack`], but every source gets the entry name it is paired with:
/// `/`-joined components, each one passing [`safe_components`]. Folders the
/// names run through are written as entries of their own.
pub fn pack_named<P: Progress>(
    items: &[(PathBuf, String)],
    archive: &Path,
    progress: &mut P,
) -> io::Result<WriteReport> {
    pack_planned(items, archive, progress, WriteReport::default())
}

fn pack_planned<P: Progress>(
    items: &[(PathBuf, String)],
    archive: &Path,
    progress: &mut P,
    mut report: WriteReport,
) -> io::Result<WriteReport> {
    refuse_nested_or_remote(archive)?;
    let format = format_of(archive)?;
    if std::fs::symlink_metadata(archive).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", archive.display()),
        ));
    }
    let parent = parent_of(archive)?;
    let planned = plan_named(items, &mut report, progress)?;
    let mut job = Job::new(progress, planned.len());

    let temp = new_temp(parent)?;
    let mut sink = Sink::create(format, temp.as_file().try_clone()?)?;
    let mut folders: HashSet<String> = HashSet::new();
    for item in &planned {
        let mut prefix = String::new();
        for part in item.name.split('/').take(item.name.matches('/').count()) {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if folders.insert(prefix.clone()) {
                sink.dir(&prefix, None, None)?;
            }
        }
        if item.meta.is_dir() {
            folders.insert(item.name.clone());
        }
        job.write_planned(&mut sink, item, &[])?;
    }
    report.entries = job.done;
    sink.finish()?.sync_all()?;
    temp.persist_noclobber(archive)
        .map_err(|failed| failed.error)?;
    forget(archive);
    Ok(report)
}

/// Adds `sources` to `archive` below the folder `inner_dir` (empty: its root).
pub fn add<P: Progress>(
    archive: &Path,
    inner_dir: &[String],
    sources: &[PathBuf],
    conflict: OnConflict,
    progress: &mut P,
) -> io::Result<WriteReport> {
    let (format, index) = open_existing(archive)?;
    if !index.node(inner_dir).is_some_and(|node| node.is_dir) {
        return Err(not_found("the folder is not in the archive"));
    }
    let mut report = WriteReport::default();
    let items = named_by_file_name(sources, &mut report)?;
    let mut planned = plan_named(&items, &mut report, progress)?;

    let mut exists_already = |item: &Planned| {
        let top = item.name.split('/').next().unwrap_or_default();
        let mut path = inner_dir.to_vec();
        path.push(top.to_owned());
        index.node(&path).is_some()
    };
    match conflict {
        OnConflict::Overwrite => {}
        OnConflict::Fail => {
            if let Some(item) = planned.iter().find(|item| exists_already(item)) {
                return Err(already_there(top_name(item)));
            }
        }
        OnConflict::Skip => {
            let taken = names_taken(&planned, &mut exists_already);
            planned.retain(|item| !taken.contains(top_name(item)));
            report.kept_existing = taken.len();
        }
    }

    let mut selector = Selector::new(inner_dir, &planned, conflict);
    let total = index.totals(&[]).map_or(0, |totals| totals.entries()) + planned.len();
    rewrite(
        archive,
        format,
        progress,
        total,
        &mut selector,
        |sink, job, seen| {
            if conflict != OnConflict::Overwrite {
                if let Some(name) = planned
                    .iter()
                    .map(top_name)
                    .find(|name| seen.taken.contains(*name))
                {
                    return Err(if conflict == OnConflict::Fail {
                        already_there(name)
                    } else {
                        already_there_hidden(name)
                    });
                }
            }
            for item in &planned {
                job.write_planned(sink, item, inner_dir)?;
            }
            Ok(())
        },
    )
    .map(|entries| {
        report.entries = entries;
        report
    })
}

/// Removes `entries` from `archive`, with everything below each of them.
pub fn delete<P: Progress>(
    archive: &Path,
    entries: &[Vec<String>],
    progress: &mut P,
) -> io::Result<WriteReport> {
    let (format, index) = open_existing(archive)?;
    for entry in entries {
        if entry.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the archive itself is not an entry in it",
            ));
        }
        if index.node(entry).is_none() {
            return Err(not_found("the entry is not in the archive"));
        }
    }
    let total = index.totals(&[]).map_or(0, |totals| totals.entries());
    let mut selector = Selector {
        inner: &[],
        dropped: entries.to_vec(),
        watched: HashSet::new(),
        taken: HashSet::new(),
    };
    let written = rewrite(
        archive,
        format,
        progress,
        total,
        &mut selector,
        |_, _, _| Ok(()),
    )?;
    Ok(WriteReport {
        entries: written,
        ..WriteReport::default()
    })
}

fn top_name(item: &Planned) -> &str {
    item.name.split('/').next().unwrap_or_default()
}

fn names_taken(planned: &[Planned], exists: &mut impl FnMut(&Planned) -> bool) -> HashSet<String> {
    planned
        .iter()
        .filter(|item| exists(item))
        .map(|item| top_name(item).to_owned())
        .collect()
}

fn already_there(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{name} already exists in the archive"),
    )
}

fn already_there_hidden(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{name} already exists in the archive as a link or special file"),
    )
}

fn not_found(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, what)
}

fn invalid(what: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, what)
}

fn format_of(archive: &Path) -> io::Result<ArchiveFormat> {
    archive
        .file_name()
        .and_then(ArchiveFormat::of_name)
        .ok_or_else(|| invalid("the name is not that of an archive format".to_owned()))
}

fn parent_of(archive: &Path) -> io::Result<&Path> {
    archive
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| invalid("the archive has no folder to be written in".to_owned()))
}

fn refuse_nested_or_remote(path: &Path) -> io::Result<()> {
    if RemotePath::parse(path).is_some() {
        return Err(invalid("archives on a server cannot be written".to_owned()));
    }
    if locate_in_archive(path).is_some_and(|location| !location.inner.is_empty()) {
        return Err(invalid(
            "an archive inside an archive cannot be written".to_owned(),
        ));
    }
    Ok(())
}

fn open_existing(archive: &Path) -> io::Result<(ArchiveFormat, std::sync::Arc<ArchiveIndex>)> {
    if RemotePath::parse(archive).is_some() {
        return Err(invalid("archives on a server cannot be written".to_owned()));
    }
    let format = format_of(archive)?;
    let location = locate_in_archive(archive)
        .ok_or_else(|| invalid("the archive does not exist".to_owned()))?;
    if location.archive != archive || !location.inner.is_empty() {
        return Err(invalid(
            "an archive inside an archive cannot be written".to_owned(),
        ));
    }
    let index = index_of(&location)?;
    Ok((format, index))
}

fn new_temp(parent: &Path) -> io::Result<NamedTempFile> {
    tempfile::Builder::new()
        .prefix(".ncrs-partial-")
        .tempfile_in(parent)
}

/// Writes a new archive next to `archive` with everything `selector` keeps
/// and then what `extend` adds, and renames it over `archive`.
fn rewrite<P: Progress>(
    archive: &Path,
    format: ArchiveFormat,
    progress: &mut P,
    total: usize,
    selector: &mut Selector,
    extend: impl FnOnce(&mut Sink, &mut Job, &Selector) -> io::Result<()>,
) -> io::Result<usize> {
    // Renaming over a symlink would replace the link and leave its target
    // unchanged, so the new archive goes next to the real file.
    let real = std::fs::canonicalize(archive)?;
    let parent = parent_of(&real)?;
    let original = std::fs::metadata(&real)?;
    let temp = new_temp(parent)?;
    let mut sink = Sink::create(format, temp.as_file().try_clone()?)?;
    let mut job = Job::new(progress, total);
    copy_original(&real, format, &mut sink, &mut job, selector)?;
    extend(&mut sink, &mut job, selector)?;
    let entries = job.done;
    sink.finish()?.sync_all()?;
    temp.as_file().set_permissions(original.permissions())?;
    temp.persist(&real).map_err(|failed| failed.error)?;
    forget(archive);
    forget(&real);
    Ok(entries)
}

/// Decides which entries of the old archive go into the new one.
struct Selector<'a> {
    inner: &'a [String],
    /// An entry at or below any of these is not copied.
    dropped: Vec<Vec<String>>,
    /// Names of the sources that must not meet an entry of the same name.
    watched: HashSet<String>,
    /// Watched names that were found, links and special files included.
    taken: HashSet<String>,
}

impl<'a> Selector<'a> {
    fn new(inner: &'a [String], planned: &[Planned], conflict: OnConflict) -> Self {
        let names = planned.iter().map(|item| top_name(item).to_owned());
        let (dropped, watched) = if conflict == OnConflict::Overwrite {
            let below = |name: String| {
                let mut path = inner.to_vec();
                path.push(name);
                path
            };
            (names.map(below).collect(), HashSet::new())
        } else {
            (Vec::new(), names.collect())
        };
        Self {
            inner,
            dropped,
            watched,
            taken: HashSet::new(),
        }
    }

    /// `None` is a name `safe_components` refuses: such an entry is kept as
    /// it is, because it cannot be told apart from anything we would drop.
    fn keeps(&mut self, safe: Option<&[String]>) -> bool {
        let Some(components) = safe else {
            return true;
        };
        let first_below = components
            .strip_prefix(self.inner)
            .and_then(|rest| rest.first());
        if let Some(name) = first_below.filter(|name| self.watched.contains(*name)) {
            self.taken.insert(name.clone());
        }
        !self
            .dropped
            .iter()
            .any(|prefix| components.starts_with(prefix))
    }
}

struct Job<'a> {
    progress: &'a mut dyn Progress,
    done: usize,
    total: usize,
}

impl<'a> Job<'a> {
    fn new(progress: &'a mut dyn Progress, total: usize) -> Self {
        Self {
            progress,
            done: 0,
            total,
        }
    }

    fn step(&mut self) -> io::Result<()> {
        self.done += 1;
        self.total = self.total.max(self.done);
        self.progress.advanced(self.done, self.total)
    }

    fn write_planned(
        &mut self,
        sink: &mut Sink,
        item: &Planned,
        under: &[String],
    ) -> io::Result<()> {
        self.progress.keep_going()?;
        let mut parts = under.to_vec();
        parts.extend(item.name.split('/').map(str::to_owned));
        let name = checked_name(&parts)?;
        if item.meta.is_dir() {
            sink.dir(&name, item.modified, item.mode)?;
        } else {
            let file = open_regular(item)?;
            let mut watched = Watched::new(file.take(item.size), &mut *self.progress);
            sink.file(&name, item.size, &mut watched, item.modified, item.mode)?;
            if watched.count != item.size {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} changed while it was being read", item.fs_path.display()),
                ));
            }
        }
        self.step()
    }
}

/// The last check before a name reaches an archive.
fn checked_name(parts: &[String]) -> io::Result<String> {
    let joined = parts.join("/");
    match safe_components(&joined) {
        Some(clean) if !clean.is_empty() && clean == parts => Ok(joined),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsafe name: {joined}"),
        )),
    }
}

/// Asks for a cancel every [`CHUNK`] bytes that pass through.
struct Watched<'a, R> {
    inner: R,
    progress: &'a mut dyn Progress,
    unchecked: u64,
    count: u64,
}

impl<'a, R> Watched<'a, R> {
    fn new(inner: R, progress: &'a mut dyn Progress) -> Self {
        Self {
            inner,
            progress,
            unchecked: CHUNK,
            count: 0,
        }
    }
}

impl<R: Read> Read for Watched<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.unchecked >= CHUNK {
            self.progress.keep_going()?;
            self.unchecked = 0;
        }
        let read = self.inner.read(buf)?;
        self.unchecked += read as u64;
        self.count += read as u64;
        Ok(read)
    }
}

struct Planned {
    /// Path inside the archive below the folder it goes into, `/` separated.
    name: String,
    fs_path: PathBuf,
    meta: Metadata,
    size: u64,
    modified: Option<SystemTime>,
    mode: Option<u32>,
}

/// Pairs every source with its file name. A name that is not UTF-8 is counted
/// as skipped, a source without a name is an error.
fn named_by_file_name(
    sources: &[PathBuf],
    report: &mut WriteReport,
) -> io::Result<Vec<(PathBuf, String)>> {
    let mut items = Vec::new();
    for source in sources {
        let Some(os_name) = source.file_name() else {
            return Err(invalid(format!("{} has no name", source.display())));
        };
        match os_name.to_str() {
            Some(name) => items.push((source.clone(), name.to_owned())),
            None => report.skipped += 1,
        }
    }
    Ok(items)
}

fn plan_named(
    items: &[(PathBuf, String)],
    report: &mut WriteReport,
    progress: &mut dyn Progress,
) -> io::Result<Vec<Planned>> {
    let mut planned = Vec::new();
    let mut tops = HashSet::new();
    let mut parents: HashSet<String> = HashSet::new();
    for (source, name) in items {
        refuse_nested_or_remote(source)?;
        let clean =
            safe_components(name).filter(|parts| !parts.is_empty() && parts.join("/") == *name);
        if clean.is_none() {
            return Err(invalid(format!("{name} cannot be a name in an archive")));
        }
        let ancestors: Vec<String> = name
            .match_indices('/')
            .map(|(at, _)| name.split_at(at).0.to_owned())
            .collect();
        if !tops.insert(name.to_owned())
            || parents.contains(name)
            || ancestors.iter().any(|ancestor| tops.contains(ancestor))
        {
            return Err(invalid(format!("two sources are named {name}")));
        }
        parents.extend(ancestors);
        let mut pending = vec![(source.clone(), name.to_owned())];
        while let Some((path, archive_name)) = pending.pop() {
            progress.keep_going()?;
            let meta = std::fs::symlink_metadata(&path)?;
            if meta.is_dir() {
                let mut children = Vec::new();
                for dir_entry in std::fs::read_dir(&path)? {
                    let child = dir_entry?;
                    let child_name = child.file_name();
                    match child_name.to_str() {
                        Some(text)
                            if safe_components(text).as_deref() == Some(&[text.to_owned()]) =>
                        {
                            children.push((child.path(), format!("{archive_name}/{text}")));
                        }
                        _ => report.skipped += 1,
                    }
                }
                children.sort_by(|a, b| b.1.cmp(&a.1));
                planned.push(describe(path, archive_name, meta));
                pending.extend(children);
            } else if meta.is_file() {
                planned.push(describe(path, archive_name, meta));
            } else {
                report.skipped += 1;
            }
        }
    }
    Ok(planned)
}

fn describe(fs_path: PathBuf, name: String, meta: Metadata) -> Planned {
    Planned {
        name,
        fs_path,
        size: if meta.is_dir() { 0 } else { meta.len() },
        modified: meta.modified().ok(),
        mode: mode_of(&meta),
        meta,
    }
}

#[cfg(unix)]
fn mode_of(meta: &Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt as _;
    Some(meta.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn mode_of(_meta: &Metadata) -> Option<u32> {
    None
}

/// Opens what the walk saw as a regular file and checks it is still the same
/// file: a link put in its place meanwhile is not followed.
fn open_regular(item: &Planned) -> io::Result<File> {
    let file = File::open(&item.fs_path)?;
    let now = file.metadata()?;
    if !now.is_file() || !same_file(&item.meta, &now) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} changed while it was being read", item.fs_path.display()),
        ));
    }
    Ok(file)
}

#[cfg(unix)]
fn same_file(before: &Metadata, now: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    before.dev() == now.dev() && before.ino() == now.ino()
}

#[cfg(not(unix))]
fn same_file(_before: &Metadata, _now: &Metadata) -> bool {
    true
}

/// The file behind a tar stream; finishing flushes compressors to the end.
enum TarOut {
    Plain(BufWriter<File>),
    Gzip(flate2::write::GzEncoder<BufWriter<File>>),
    Zstd(zstd::Encoder<'static, BufWriter<File>>),
}

impl Write for TarOut {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(out) => out.write(buf),
            Self::Gzip(out) => out.write(buf),
            Self::Zstd(out) => out.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(out) => out.flush(),
            Self::Gzip(out) => out.flush(),
            Self::Zstd(out) => out.flush(),
        }
    }
}

impl TarOut {
    fn finish(self) -> io::Result<File> {
        let buffered = match self {
            Self::Plain(out) => out,
            Self::Gzip(out) => out.finish()?,
            Self::Zstd(out) => out.finish()?,
        };
        buffered
            .into_inner()
            .map_err(io::IntoInnerError::into_error)
    }
}

enum Sink {
    Zip(Box<zip::ZipWriter<BufWriter<File>>>),
    Tar(Box<tar::Builder<TarOut>>),
    SevenZip(Box<sevenz_rust2::ArchiveWriter<BufWriter<File>>>),
}

const ZSTD_LEVEL: i32 = 3;
const DEFAULT_FILE_MODE: u32 = 0o644;
const DEFAULT_DIR_MODE: u32 = 0o755;
const UNIX_REGULAR: u32 = 0o100_000;
const UNIX_DIRECTORY: u32 = 0o040_000;
const FILE_ATTRIBUTE_UNIX_EXTENSION: u32 = 0x8000;

impl Sink {
    fn create(format: ArchiveFormat, file: File) -> io::Result<Self> {
        let out = BufWriter::new(file);
        Ok(match format {
            ArchiveFormat::Zip => Self::Zip(Box::new(zip::ZipWriter::new(out))),
            ArchiveFormat::Tar => Self::Tar(Box::new(tar::Builder::new(TarOut::Plain(out)))),
            ArchiveFormat::TarGz => Self::Tar(Box::new(tar::Builder::new(TarOut::Gzip(
                flate2::write::GzEncoder::new(out, flate2::Compression::default()),
            )))),
            ArchiveFormat::TarZst => Self::Tar(Box::new(tar::Builder::new(TarOut::Zstd(
                zstd::Encoder::new(out, ZSTD_LEVEL)?,
            )))),
            ArchiveFormat::SevenZip => Self::SevenZip(Box::new(
                sevenz_rust2::ArchiveWriter::new(out).map_err(sevenz::io_error)?,
            )),
        })
    }

    fn dir(
        &mut self,
        name: &str,
        modified: Option<SystemTime>,
        mode: Option<u32>,
    ) -> io::Result<()> {
        match self {
            Self::Zip(writer) => writer
                .add_directory(name, zip_options(0, modified, mode))
                .map_err(zip_error),
            Self::Tar(builder) => {
                let mut header = new_tar_header(tar::EntryType::Directory, 0, modified, mode);
                builder.append_data(&mut header, name, io::empty())
            }
            Self::SevenZip(writer) => {
                let mut entry = sevenz_rust2::ArchiveEntry::new_directory(name);
                describe_sevenz(&mut entry, modified, mode, UNIX_DIRECTORY);
                writer
                    .push_archive_entry(entry, None::<&[u8]>)
                    .map(drop)
                    .map_err(sevenz::io_error)
            }
        }
    }

    fn file(
        &mut self,
        name: &str,
        size: u64,
        content: &mut dyn Read,
        modified: Option<SystemTime>,
        mode: Option<u32>,
    ) -> io::Result<()> {
        match self {
            Self::Zip(writer) => {
                writer
                    .start_file(name, zip_options(size, modified, mode))
                    .map_err(zip_error)?;
                io::copy(content, writer).map(drop)
            }
            Self::Tar(builder) => {
                let mut header = new_tar_header(tar::EntryType::Regular, size, modified, mode);
                builder.append_data(&mut header, name, content)
            }
            Self::SevenZip(writer) => {
                let mut entry = sevenz_rust2::ArchiveEntry::new_file(name);
                describe_sevenz(&mut entry, modified, mode, UNIX_REGULAR);
                let data = (size > 0).then_some(content);
                writer
                    .push_archive_entry(entry, data)
                    .map(drop)
                    .map_err(sevenz::io_error)
            }
        }
    }

    fn finish(self) -> io::Result<File> {
        match self {
            Self::Zip(writer) => (*writer)
                .finish()
                .map_err(zip_error)?
                .into_inner()
                .map_err(io::IntoInnerError::into_error),
            Self::Tar(builder) => (*builder).into_inner()?.finish(),
            Self::SevenZip(writer) => (*writer)
                .finish()?
                .into_inner()
                .map_err(io::IntoInnerError::into_error),
        }
    }
}

fn zip_options(
    size: u64,
    modified: Option<SystemTime>,
    mode: Option<u32>,
) -> zip::write::SimpleFileOptions {
    let mut options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(size >= u64::from(u32::MAX));
    if let Some(stamp) = modified.and_then(zip_datetime) {
        options = options.last_modified_time(stamp);
    }
    if let Some(bits) = mode {
        options = options.unix_permissions(bits);
    }
    options
}

fn zip_datetime(time: SystemTime) -> Option<zip::DateTime> {
    let local = chrono::DateTime::<chrono::Local>::from(time);
    zip::DateTime::from_date_and_time(
        u16::try_from(local.year()).ok()?,
        u8::try_from(local.month()).ok()?,
        u8::try_from(local.day()).ok()?,
        u8::try_from(local.hour()).ok()?,
        u8::try_from(local.minute()).ok()?,
        u8::try_from(local.second()).ok()?,
    )
    .ok()
}

fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn new_tar_header(
    kind: tar::EntryType,
    size: u64,
    modified: Option<SystemTime>,
    mode: Option<u32>,
) -> tar::Header {
    let default_mode = if kind == tar::EntryType::Directory {
        DEFAULT_DIR_MODE
    } else {
        DEFAULT_FILE_MODE
    };
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_mode(mode.unwrap_or(default_mode));
    header.set_mtime(modified.map_or(0, unix_seconds));
    header
}

fn describe_sevenz(
    entry: &mut sevenz_rust2::ArchiveEntry,
    modified: Option<SystemTime>,
    mode: Option<u32>,
    unix_kind: u32,
) {
    if let Some(stamp) = modified.and_then(|time| sevenz_rust2::NtTime::try_from(time).ok()) {
        entry.has_last_modified_date = true;
        entry.last_modified_date = stamp;
    }
    if let Some(bits) = mode {
        entry.has_windows_attributes = true;
        entry.windows_attributes = FILE_ATTRIBUTE_UNIX_EXTENSION | ((unix_kind | bits) << 16);
    }
}

fn copy_original(
    archive: &Path,
    format: ArchiveFormat,
    sink: &mut Sink,
    job: &mut Job,
    selector: &mut Selector,
) -> io::Result<()> {
    match sink {
        Sink::Zip(writer) => copy_zip(archive, writer, job, selector),
        Sink::Tar(builder) => copy_tar(archive, format, builder, job, selector),
        Sink::SevenZip(writer) => copy_sevenz(archive, writer, job, selector),
    }
}

/// Entries are copied raw, compressed as they are; a very large one is not
/// interrupted by a cancel until it is through.
fn copy_zip(
    archive: &Path,
    writer: &mut zip::ZipWriter<BufWriter<File>>,
    job: &mut Job,
    selector: &mut Selector,
) -> io::Result<()> {
    let mut zip = open_zip(archive)?;
    for position in 0..zip.len() {
        job.progress.keep_going()?;
        let entry = zip.by_index_raw(position).map_err(zip_error)?;
        let components = entry.name().ok().and_then(|name| safe_components(&name));
        if selector.keeps(components.as_deref()) {
            writer.raw_copy_file(entry).map_err(zip_error)?;
            job.step()?;
        }
    }
    Ok(())
}

fn copy_tar(
    archive: &Path,
    format: ArchiveFormat,
    builder: &mut tar::Builder<TarOut>,
    job: &mut Job,
    selector: &mut Selector,
) -> io::Result<()> {
    for item in open_tar(archive, format)?.entries()? {
        job.progress.keep_going()?;
        let mut entry = item?;
        let kind = entry.header().entry_type();
        let mut header = entry.header().clone();
        let path = entry.path()?.into_owned();
        let components = safe_components(&String::from_utf8_lossy(&entry.path_bytes()));
        if kind == tar::EntryType::XGlobalHeader {
            let mut watched = Watched::new(&mut entry, &mut *job.progress);
            builder.append(&header, &mut watched)?;
            continue;
        }
        if components.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "the archive has an entry with an unsafe name: {}",
                    path.display()
                ),
            ));
        }
        if !selector.keeps(components.as_deref()) {
            continue;
        }
        match kind {
            tar::EntryType::GNUSparse => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{} is a sparse entry, which ncrs cannot rewrite",
                        path.display()
                    ),
                ));
            }
            tar::EntryType::Symlink | tar::EntryType::Link => {
                let target = entry
                    .link_name()?
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "a link without a target")
                    })?
                    .into_owned();
                builder.append_link(&mut header, &path, &target)?;
            }
            _ => {
                let size = header.size()?;
                let mut watched = Watched::new((&mut entry).take(size), &mut *job.progress);
                builder.append_data(&mut header, &path, &mut watched)?;
            }
        }
        job.step()?;
    }
    Ok(())
}

fn copy_sevenz(
    archive: &Path,
    writer: &mut sevenz_rust2::ArchiveWriter<BufWriter<File>>,
    job: &mut Job,
    selector: &mut Selector,
) -> io::Result<()> {
    let mut reader = sevenz::open(archive)?;
    reader
        .for_each_entries(|entry, content| {
            copy_sevenz_entry(entry, content, writer, job, selector)
                .map_err(sevenz_rust2::Error::from)?;
            Ok(true)
        })
        .map_err(sevenz::io_error)
}

fn copy_sevenz_entry(
    entry: &sevenz_rust2::ArchiveEntry,
    content: &mut dyn Read,
    writer: &mut sevenz_rust2::ArchiveWriter<BufWriter<File>>,
    job: &mut Job,
    selector: &mut Selector,
) -> io::Result<()> {
    job.progress.keep_going()?;
    let components = safe_components(entry.name());
    let mut watched = Watched::new(content, &mut *job.progress);
    if !selector.keeps(components.as_deref()) {
        io::copy(&mut watched, &mut io::sink())?;
        return Ok(());
    }
    let data = (entry.has_stream && !entry.is_directory).then_some(&mut watched);
    writer
        .push_archive_entry(entry.clone(), data)
        .map_err(sevenz::io_error)?;
    job.step()
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::super::fixtures::{scratch, write_archive, Item, ALL_FORMATS, ENCRYPTED_7Z};
    use super::super::{copy_out, list, refusal_of, ArchiveLocation, SevenZipRefusal};
    use super::*;
    use crate::fs::transfer::Counting;

    const BIG: usize = 2_621_440;

    fn big_bytes() -> Vec<u8> {
        let mut state: u32 = 0x2545_f491;
        std::iter::repeat_with(|| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            u8::try_from(state >> 24).unwrap()
        })
        .take(BIG)
        .collect()
    }

    fn at(archive: &Path, inner: &[&str]) -> ArchiveLocation {
        ArchiveLocation {
            archive: archive.to_path_buf(),
            format: ArchiveFormat::of_name(archive.file_name().unwrap()).unwrap(),
            inner: inner.iter().map(|part| (*part).to_owned()).collect(),
        }
    }

    fn listing(archive: &Path, inner: &[&str]) -> Vec<String> {
        list(&at(archive, inner))
            .unwrap()
            .entries
            .iter()
            .map(|entry| {
                let name = entry.name.to_string_lossy().into_owned();
                if entry.is_dir {
                    format!("{name}/")
                } else {
                    name
                }
            })
            .collect()
    }

    fn bytes_of(archive: &Path, inner: &[&str]) -> Vec<u8> {
        let out = scratch("bytes-of");
        let source = inner
            .iter()
            .fold(archive.to_path_buf(), |path, part| path.join(part));
        copy_out(&source, out.path(), &mut Counting, OnConflict::Fail).unwrap();
        std::fs::read(out.path().join(inner.last().unwrap())).unwrap()
    }

    fn inner(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    fn leftovers(dir: &Path) -> Vec<OsString> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().starts_with(".ncrs-partial-"))
            .collect()
    }

    fn names_in_file(archive: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            match ArchiveFormat::of_name(archive.file_name().unwrap()).unwrap() {
                ArchiveFormat::Zip => {
                    let mut zip = open_zip(archive).unwrap();
                    (0..zip.len())
                        .map(|position| {
                            zip.by_index_raw(position)
                                .unwrap()
                                .name()
                                .unwrap()
                                .into_owned()
                        })
                        .collect()
                }
                ArchiveFormat::SevenZip => sevenz::open(archive)
                    .unwrap()
                    .archive()
                    .files
                    .iter()
                    .map(|entry| entry.name().to_owned())
                    .collect(),
                format => open_tar(archive, format)
                    .unwrap()
                    .entries()
                    .unwrap()
                    .map(|entry| {
                        entry
                            .unwrap()
                            .path()
                            .unwrap()
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect(),
            };
        for name in &mut names {
            name.truncate(name.trim_end_matches('/').len());
        }
        names.sort();
        names
    }

    fn make_tree(root: &Path) -> PathBuf {
        let tree = root.join("pack-src");
        std::fs::create_dir_all(tree.join("sub")).unwrap();
        std::fs::create_dir(tree.join("empty")).unwrap();
        std::fs::write(tree.join("a.txt"), b"alpha").unwrap();
        std::fs::write(tree.join("sub").join("b.txt"), b"beta").unwrap();
        std::fs::write(tree.join("sub").join("Gr\u{fc}\u{df}e.txt"), b"hallo").unwrap();
        std::fs::write(tree.join("empty.txt"), b"").unwrap();
        std::fs::write(tree.join("big.bin"), big_bytes()).unwrap();
        tree
    }

    fn sample_base(archive: &Path) {
        write_archive(
            archive,
            &[
                Item::File("top.txt", b"top"),
                Item::File("2024/jan/a.jpg", b"aaaa"),
                Item::File("2024/b.jpg", b"bb"),
                Item::Dir("empty"),
            ],
        );
    }

    struct CancelAfter {
        calls: usize,
        limit: usize,
    }

    impl CancelAfter {
        fn new(limit: usize) -> Self {
            Self { calls: 0, limit }
        }

        fn tick(&mut self) -> io::Result<()> {
            self.calls += 1;
            if self.calls >= self.limit {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "cancelled",
                ));
            }
            Ok(())
        }
    }

    impl Progress for CancelAfter {
        fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
            self.tick()
        }

        fn keep_going(&mut self) -> io::Result<()> {
            self.tick()
        }
    }

    #[cfg(unix)]
    fn link(target: &str, at: &Path) {
        std::os::unix::fs::symlink(target, at).unwrap();
    }

    #[test]
    fn every_format_packs_a_tree_that_reads_back_equal() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("pack");
            let tree = make_tree(dir.path());
            std::fs::write(dir.path().join("loose.txt"), b"loose").unwrap();
            #[cfg(unix)]
            link("a.txt", &tree.join("a-link"));
            let archive = dir.path().join(file);

            let report = pack(
                &[tree.clone(), dir.path().join("loose.txt")],
                &archive,
                &mut Counting,
            )
            .unwrap();

            assert_eq!(report.skipped, usize::from(cfg!(unix)), "{file}");
            assert_eq!(listing(&archive, &[]), ["loose.txt", "pack-src/"], "{file}");
            assert_eq!(
                listing(&archive, &["pack-src"]),
                ["a.txt", "big.bin", "empty/", "empty.txt", "sub/"],
                "{file}"
            );
            assert_eq!(
                listing(&archive, &["pack-src", "sub"]),
                ["Gr\u{fc}\u{df}e.txt", "b.txt"],
                "{file}"
            );
            assert_eq!(
                listing(&archive, &["pack-src", "empty"]),
                Vec::<String>::new()
            );
            assert_eq!(bytes_of(&archive, &["loose.txt"]), b"loose", "{file}");
            assert_eq!(bytes_of(&archive, &["pack-src", "a.txt"]), b"alpha");
            assert_eq!(bytes_of(&archive, &["pack-src", "empty.txt"]), b"");
            assert_eq!(bytes_of(&archive, &["pack-src", "big.bin"]), big_bytes());
            assert_eq!(
                bytes_of(&archive, &["pack-src", "sub", "Gr\u{fc}\u{df}e.txt"]),
                b"hallo"
            );
            assert!(leftovers(dir.path()).is_empty(), "{file}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn packing_keeps_the_permission_bits() {
        use std::os::unix::fs::PermissionsExt as _;
        for (file, _) in ALL_FORMATS {
            let dir = scratch("pack-mode");
            let script = dir.path().join("run.sh");
            std::fs::write(&script, b"#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o750)).unwrap();
            let archive = dir.path().join(file);
            pack(&[script], &archive, &mut Counting).unwrap();

            let out = scratch("pack-mode-out");
            copy_out(
                &archive.join("run.sh"),
                out.path(),
                &mut Counting,
                OnConflict::Fail,
            )
            .unwrap();
            let mode = std::fs::metadata(out.path().join("run.sh"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o750, "{file}");
        }
    }

    #[test]
    fn pack_refuses_an_existing_archive_a_bad_name_and_nested_places() {
        let dir = scratch("pack-refuse");
        let tree = make_tree(dir.path());
        let existing = dir.path().join("there.zip");
        std::fs::write(&existing, b"keep me").unwrap();
        let err = pack(std::slice::from_ref(&tree), &existing, &mut Counting).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&existing).unwrap(), b"keep me");

        let err_2 = pack(
            std::slice::from_ref(&tree),
            &dir.path().join("x.txt"),
            &mut Counting,
        )
        .unwrap_err();
        assert_eq!(err_2.kind(), io::ErrorKind::InvalidInput);

        let outer = dir.path().join("outer.zip");
        sample_base(&outer);
        let inside = outer.join("2024").join("new.zip");
        let err_3 = pack(std::slice::from_ref(&tree), &inside, &mut Counting).unwrap_err();
        assert_eq!(err_3.kind(), io::ErrorKind::InvalidInput);

        let err_4 = pack(
            &[outer.join("top.txt")],
            &dir.path().join("n.zip"),
            &mut Counting,
        )
        .unwrap_err();
        assert_eq!(err_4.kind(), io::ErrorKind::InvalidInput);
        assert!(!dir.path().join("n.zip").exists());

        let err_5 = pack(
            &[tree.clone(), tree],
            &dir.path().join("twice.zip"),
            &mut Counting,
        )
        .unwrap_err();
        assert_eq!(err_5.kind(), io::ErrorKind::InvalidInput);
        assert!(leftovers(dir.path()).is_empty());
    }

    fn two_invoices(dir: &Path) -> Vec<PathBuf> {
        let one = dir.join("firma");
        let two = dir.join("privat");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        std::fs::write(one.join("invoice.pdf"), b"from firma").unwrap();
        std::fs::write(two.join("invoice.pdf"), b"from privat").unwrap();
        vec![one.join("invoice.pdf"), two.join("invoice.pdf")]
    }

    #[test]
    fn pack_named_writes_flat_numbered_names_with_the_right_contents() {
        let dir = scratch("pack-named-flat");
        let sources = two_invoices(dir.path());
        let names = crate::fs::flat_names::flat_names(&sources);
        let items: Vec<(PathBuf, String)> = sources
            .into_iter()
            .zip(names)
            .map(|(path, name)| (path, name.unwrap().to_string_lossy().into_owned()))
            .collect();
        assert!(items.iter().all(|(_, name)| !name.contains('/')));
        for extension in ["zip", "tar", "tar.gz", "tar.zst", "7z"] {
            let archive = dir.path().join(format!("flat.{extension}"));
            pack_named(&items, &archive, &mut Counting).unwrap();
            assert_eq!(
                names_in_file(&archive),
                ["invoice (2).pdf", "invoice.pdf"],
                "{extension}"
            );
            assert_eq!(bytes_of(&archive, &["invoice.pdf"]), b"from firma");
            assert_eq!(bytes_of(&archive, &["invoice (2).pdf"]), b"from privat");
        }
    }

    #[test]
    fn pack_named_nested_names_read_back_as_folders() {
        let dir = scratch("pack-named-nested");
        let sources = two_invoices(dir.path());
        let items = vec![
            (sources[0].clone(), "firma/invoice.pdf".to_owned()),
            (sources[1].clone(), "privat/2024/invoice.pdf".to_owned()),
        ];
        for extension in ["zip", "tar", "7z"] {
            let archive = dir.path().join(format!("nested.{extension}"));
            pack_named(&items, &archive, &mut Counting).unwrap();
            assert_eq!(listing(&archive, &[]), ["firma/", "privat/"], "{extension}");
            assert_eq!(listing(&archive, &["privat"]), ["2024/"]);
            assert_eq!(listing(&archive, &["privat", "2024"]), ["invoice.pdf"]);
            assert_eq!(
                bytes_of(&archive, &["privat", "2024", "invoice.pdf"]),
                b"from privat"
            );
        }
    }

    #[test]
    fn pack_named_refuses_unsafe_empty_and_clashing_names_without_an_archive() {
        let dir = scratch("pack-named-refuse");
        let sources = two_invoices(dir.path());
        let source = sources[0].clone();
        for bad in [
            "../x.pdf",
            "/abs.pdf",
            "",
            "a//b.pdf",
            "a/../b.pdf",
            "./x.pdf",
        ] {
            let archive = dir.path().join("bad.zip");
            let err = pack_named(&[(source.clone(), bad.to_owned())], &archive, &mut Counting)
                .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{bad:?}");
            assert!(!archive.exists(), "{bad:?}");
        }
        let archive = dir.path().join("twice.zip");
        let clash = vec![
            (sources[0].clone(), "same.pdf".to_owned()),
            (sources[1].clone(), "same.pdf".to_owned()),
        ];
        assert!(pack_named(&clash, &archive, &mut Counting).is_err());
        let file_and_folder = vec![
            (sources[0].clone(), "a".to_owned()),
            (sources[1].clone(), "a/b.pdf".to_owned()),
        ];
        assert!(pack_named(&file_and_folder, &archive, &mut Counting).is_err());
        assert!(!archive.exists());
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn pack_named_keeps_folder_sources_below_their_entry_name() {
        let dir = scratch("pack-named-folder");
        let tree = make_tree(dir.path());
        let archive = dir.path().join("tree.zip");
        pack_named(
            &[(tree.clone(), "docs/kept".to_owned())],
            &archive,
            &mut Counting,
        )
        .unwrap();
        let names = names_in_file(&archive);
        assert!(names.contains(&"docs".to_owned()) && names.contains(&"docs/kept".to_owned()));
        assert!(names.iter().all(|name| name.starts_with("docs")));
    }

    #[test]
    fn an_archive_can_be_packed_into_an_archive() {
        let dir = scratch("pack-archive");
        let inner_archive = dir.path().join("inner.zip");
        sample_base(&inner_archive);
        let outer = dir.path().join("outer.tar");
        pack(std::slice::from_ref(&inner_archive), &outer, &mut Counting).unwrap();
        assert_eq!(listing(&outer, &[]), ["inner.zip"]);
    }

    #[test]
    fn add_puts_sources_into_the_root_and_into_a_folder_in_every_format() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("add");
            let archive = dir.path().join(file);
            sample_base(&archive);
            let photos = dir.path().join("photos");
            std::fs::create_dir(&photos).unwrap();
            std::fs::write(photos.join("x.jpg"), b"xx").unwrap();
            let note = dir.path().join("note.txt");
            std::fs::write(&note, b"note").unwrap();

            add(
                &archive,
                &[],
                &[photos.clone(), note.clone()],
                OnConflict::Fail,
                &mut Counting,
            )
            .unwrap();
            add(
                &archive,
                &inner(&["2024", "jan"]),
                &[note],
                OnConflict::Fail,
                &mut Counting,
            )
            .unwrap();

            assert_eq!(
                listing(&archive, &[]),
                ["2024/", "empty/", "note.txt", "photos/", "top.txt"],
                "{file}"
            );
            assert_eq!(bytes_of(&archive, &["photos", "x.jpg"]), b"xx", "{file}");
            assert_eq!(bytes_of(&archive, &["2024", "jan", "note.txt"]), b"note");
            assert_eq!(bytes_of(&archive, &["2024", "jan", "a.jpg"]), b"aaaa");
            assert_eq!(bytes_of(&archive, &["top.txt"]), b"top", "{file}");
            assert_eq!(listing(&archive, &["empty"]), Vec::<String>::new());
            assert!(leftovers(dir.path()).is_empty(), "{file}");
        }
    }

    #[test]
    fn add_follows_the_conflict_policy() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("add-conflict");
            let archive = dir.path().join(file);
            sample_base(&archive);
            let clash = dir.path().join("2024");
            std::fs::create_dir(&clash).unwrap();
            std::fs::write(clash.join("new.txt"), b"fresh").unwrap();
            let top = dir.path().join("top.txt");
            std::fs::write(&top, b"replacement").unwrap();
            let before = std::fs::read(&archive).unwrap();

            let err = add(
                &archive,
                &[],
                &[top.clone(), clash.clone()],
                OnConflict::Fail,
                &mut Counting,
            )
            .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{file}");
            assert_eq!(std::fs::read(&archive).unwrap(), before, "{file}");
            assert!(leftovers(dir.path()).is_empty(), "{file}");

            let report = add(
                &archive,
                &[],
                &[top.clone(), clash.clone()],
                OnConflict::Skip,
                &mut Counting,
            )
            .unwrap();
            assert_eq!(report.kept_existing, 2, "{file}");
            assert_eq!(bytes_of(&archive, &["top.txt"]), b"top", "{file}");
            assert_eq!(listing(&archive, &["2024"]), ["b.jpg", "jan/"], "{file}");

            add(
                &archive,
                &[],
                &[top, clash],
                OnConflict::Overwrite,
                &mut Counting,
            )
            .unwrap();
            assert_eq!(bytes_of(&archive, &["top.txt"]), b"replacement", "{file}");
            assert_eq!(listing(&archive, &["2024"]), ["new.txt"], "{file}");
            assert_eq!(listing(&archive, &[]).len(), 3, "{file}");
        }
    }

    #[test]
    fn add_refuses_a_missing_archive_folder_and_a_file_as_folder() {
        let dir = scratch("add-refuse");
        let archive = dir.path().join("a.zip");
        sample_base(&archive);
        let note = dir.path().join("note.txt");
        std::fs::write(&note, b"n").unwrap();
        let before = std::fs::read(&archive).unwrap();

        for folder in [inner(&["nope"]), inner(&["top.txt"])] {
            let err = add(
                &archive,
                &folder,
                std::slice::from_ref(&note),
                OnConflict::Fail,
                &mut Counting,
            )
            .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::NotFound, "{folder:?}");
        }
        let err_2 = add(
            &dir.path().join("gone.zip"),
            &[],
            std::slice::from_ref(&note),
            OnConflict::Fail,
            &mut Counting,
        )
        .unwrap_err();
        assert_eq!(err_2.kind(), io::ErrorKind::InvalidInput);
        let err_3 = add(
            &archive.join("2024").join("x.zip"),
            &[],
            std::slice::from_ref(&note),
            OnConflict::Fail,
            &mut Counting,
        )
        .unwrap_err();
        assert_eq!(err_3.kind(), io::ErrorKind::InvalidInput);
        let err_4 = add(
            &archive,
            &[],
            &[archive.join("top.txt")],
            OnConflict::Fail,
            &mut Counting,
        )
        .unwrap_err();
        assert_eq!(err_4.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(std::fs::read(&archive).unwrap(), before);
        assert!(leftovers(dir.path()).is_empty());
    }

    #[test]
    fn delete_removes_a_file_a_folder_with_children_and_everything() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("delete");
            let archive = dir.path().join(file);
            sample_base(&archive);

            delete(&archive, &[inner(&["top.txt"])], &mut Counting).unwrap();
            assert_eq!(listing(&archive, &[]), ["2024/", "empty/"], "{file}");

            delete(&archive, &[inner(&["2024"])], &mut Counting).unwrap();
            assert_eq!(listing(&archive, &[]), ["empty/"], "{file}");
            assert_eq!(names_in_file(&archive), ["empty"], "{file}");

            let err = delete(&archive, &[inner(&["2024"])], &mut Counting).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::NotFound, "{file}");
            let err_2 = delete(&archive, &[Vec::new()], &mut Counting).unwrap_err();
            assert_eq!(err_2.kind(), io::ErrorKind::InvalidInput, "{file}");

            delete(&archive, &[inner(&["empty"])], &mut Counting).unwrap();
            assert_eq!(listing(&archive, &[]), Vec::<String>::new(), "{file}");
            assert!(leftovers(dir.path()).is_empty(), "{file}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_archive_behind_a_symlink_is_changed_and_the_link_stays() {
        let dir = scratch("behind-link");
        let real = dir.path().join("real.zip");
        sample_base(&real);
        let through = dir.path().join("link.zip");
        link("real.zip", &through);

        delete(&through, &[inner(&["top.txt"])], &mut Counting).unwrap();

        assert!(std::fs::symlink_metadata(&through)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(listing(&real, &[]), ["2024/", "empty/"]);
        assert_eq!(listing(&through, &[]), ["2024/", "empty/"]);
    }

    #[test]
    fn a_folder_that_exists_only_through_its_children_is_deleted_with_them() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("delete-implicit");
            let archive = dir.path().join(file);
            write_archive(
                &archive,
                &[
                    Item::File("deep/er/f.txt", b"f"),
                    Item::File("keep.txt", b"k"),
                ],
            );
            delete(&archive, &[inner(&["deep"])], &mut Counting).unwrap();
            assert_eq!(listing(&archive, &[]), ["keep.txt"], "{file}");
            assert_eq!(names_in_file(&archive), ["keep.txt"], "{file}");
        }
    }

    #[test]
    fn links_the_index_leaves_out_survive_an_add_and_a_delete() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("hidden");
            let archive = dir.path().join(file);
            write_archive(
                &archive,
                &[
                    Item::File("a.txt", b"a"),
                    Item::File("b.txt", b"b"),
                    Item::Link("hidden-link", "a.txt"),
                ],
            );
            let note = dir.path().join("note.txt");
            std::fs::write(&note, b"n").unwrap();

            add(&archive, &[], &[note], OnConflict::Fail, &mut Counting).unwrap();
            let names = names_in_file(&archive);
            assert!(
                names.contains(&"hidden-link".to_owned()),
                "{file}: {names:?}"
            );
            assert_eq!(list(&at(&archive, &[])).unwrap().left_out, 1, "{file}");

            delete(&archive, &[inner(&["b.txt"])], &mut Counting).unwrap();
            let names_after_delete = names_in_file(&archive);
            assert_eq!(
                names_after_delete,
                ["a.txt", "hidden-link", "note.txt"],
                "{file}"
            );
        }
    }

    #[test]
    fn apple_double_entries_survive_a_delete() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("apple");
            let archive = dir.path().join(file);
            write_archive(
                &archive,
                &[
                    Item::File("a.txt", b"a"),
                    Item::File("b.txt", b"b"),
                    Item::File("__MACOSX/._a.txt", b"x"),
                    Item::File("._b.txt", b"x"),
                ],
            );

            delete(&archive, &[inner(&["a.txt"])], &mut Counting).unwrap();
            assert_eq!(
                names_in_file(&archive),
                ["._b.txt", "__MACOSX/._a.txt", "b.txt"],
                "{file}"
            );
            assert_eq!(listing(&archive, &[]), ["b.txt"], "{file}");
        }
    }

    #[test]
    fn a_link_does_not_block_a_name_it_shares_with_a_source() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("hidden-conflict");
            let archive = dir.path().join(file);
            write_archive(
                &archive,
                &[Item::File("a.txt", b"a"), Item::Link("l", "a.txt")],
            );
            let source = dir.path().join("l");
            std::fs::write(&source, b"file l").unwrap();
            let before = std::fs::read(&archive).unwrap();

            let err = add(
                &archive,
                &[],
                std::slice::from_ref(&source),
                OnConflict::Fail,
                &mut Counting,
            )
            .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{file}");
            assert_eq!(std::fs::read(&archive).unwrap(), before, "{file}");

            add(
                &archive,
                &[],
                &[source],
                OnConflict::Overwrite,
                &mut Counting,
            )
            .unwrap();
            assert_eq!(names_in_file(&archive), ["a.txt", "l"], "{file}");
            assert_eq!(bytes_of(&archive, &["l"]), b"file l", "{file}");
        }
    }

    #[test]
    fn an_entry_with_an_unsafe_name_is_never_written_by_a_tar_rewrite() {
        let dir = scratch("unsafe-tar");
        let archive = dir.path().join("evil.tar");
        write_archive(
            &archive,
            &[Item::File("ok.txt", b"ok"), Item::File("../evil.txt", b"x")],
        );
        let note = dir.path().join("note.txt");
        std::fs::write(&note, b"n").unwrap();
        let before = std::fs::read(&archive).unwrap();

        let err = add(
            &archive,
            &[],
            std::slice::from_ref(&note),
            OnConflict::Fail,
            &mut Counting,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(std::fs::read(&archive).unwrap(), before);
        assert!(leftovers(dir.path()).is_empty());

        let zipped = dir.path().join("evil.zip");
        write_archive(
            &zipped,
            &[Item::File("ok.txt", b"ok"), Item::File("../evil.txt", b"x")],
        );
        add(&zipped, &[], &[note], OnConflict::Fail, &mut Counting).unwrap();
        assert_eq!(listing(&zipped, &[]), ["note.txt", "ok.txt"]);
        assert!(names_in_file(&zipped).contains(&"../evil.txt".to_owned()));
        assert!(!dir.path().parent().unwrap().join("evil.txt").exists());
    }

    #[test]
    fn a_cancelled_write_changes_nothing_and_leaves_no_temporary_file() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("cancel");
            let archive = dir.path().join(file);
            sample_base(&archive);
            let note = dir.path().join("note.txt");
            std::fs::write(&note, b"n").unwrap();
            let before = std::fs::read(&archive).unwrap();
            let listed_before: Vec<_> = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();

            let err = add(
                &archive,
                &[],
                &[note],
                OnConflict::Fail,
                &mut CancelAfter::new(3),
            )
            .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::ConnectionAborted, "{file}");
            let err_2 =
                delete(&archive, &[inner(&["2024"])], &mut CancelAfter::new(3)).unwrap_err();
            assert_eq!(err_2.kind(), io::ErrorKind::ConnectionAborted, "{file}");

            assert_eq!(std::fs::read(&archive).unwrap(), before, "{file}");
            let mut listed_after: Vec<_> = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            let mut expected = listed_before;
            expected.sort();
            listed_after.sort();
            assert_eq!(listed_after, expected, "{file}");
        }
    }

    #[test]
    fn a_cancelled_pack_creates_no_archive_also_inside_a_large_file() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("cancel-pack");
            let tree = make_tree(dir.path());
            let archive = dir.path().join(file);
            for limit in [3, 8] {
                let err = pack(
                    std::slice::from_ref(&tree),
                    &archive,
                    &mut CancelAfter::new(limit),
                )
                .unwrap_err();
                assert_eq!(
                    err.kind(),
                    io::ErrorKind::ConnectionAborted,
                    "{file} {limit}"
                );
                assert!(!archive.exists(), "{file} {limit}");
                assert!(leftovers(dir.path()).is_empty(), "{file} {limit}");
            }

            let big = dir.path().join("big-only.bin");
            std::fs::write(&big, big_bytes()).unwrap();
            let err_2 = pack(&[big], &archive, &mut CancelAfter::new(4)).unwrap_err();
            assert_eq!(err_2.kind(), io::ErrorKind::ConnectionAborted, "{file}");
            assert!(!archive.exists(), "{file}");
            assert!(leftovers(dir.path()).is_empty(), "{file}");
        }
    }

    #[test]
    fn a_7z_rewrite_keeps_every_entry() {
        let dir = scratch("sevenz-rewrite");
        let archive = dir.path().join("keep.7z");
        write_archive(
            &archive,
            &[
                Item::File("a.txt", b"alpha"),
                Item::File("sub/b.txt", b"beta"),
                Item::File("empty.txt", b""),
                Item::Dir("void"),
                Item::Link("lnk", "a.txt"),
            ],
        );
        let note = dir.path().join("note.txt");
        std::fs::write(&note, b"n").unwrap();

        add(&archive, &[], &[note], OnConflict::Fail, &mut Counting).unwrap();

        assert_eq!(
            names_in_file(&archive),
            ["a.txt", "empty.txt", "lnk", "note.txt", "sub/b.txt", "void"]
        );
        assert_eq!(bytes_of(&archive, &["a.txt"]), b"alpha");
        assert_eq!(bytes_of(&archive, &["sub", "b.txt"]), b"beta");
        assert_eq!(bytes_of(&archive, &["empty.txt"]), b"");
        assert_eq!(
            listing(&archive, &[]),
            ["a.txt", "empty.txt", "note.txt", "sub/", "void/"]
        );
    }

    #[test]
    fn an_encrypted_7z_is_refused_and_left_unchanged() {
        for (file, bytes) in ENCRYPTED_7Z {
            let dir = scratch("sevenz-encrypted");
            let archive = dir.path().join(file);
            std::fs::write(&archive, bytes).unwrap();
            let note = dir.path().join("note.txt");
            std::fs::write(&note, b"n").unwrap();

            let added = add(&archive, &[], &[note], OnConflict::Fail, &mut Counting).unwrap_err();
            assert_eq!(
                refusal_of(&added),
                Some(SevenZipRefusal::Encrypted),
                "{file}"
            );
            let deleted = delete(&archive, &[inner(&["secret.txt"])], &mut Counting).unwrap_err();
            assert_eq!(
                refusal_of(&deleted),
                Some(SevenZipRefusal::Encrypted),
                "{file}"
            );

            assert_eq!(std::fs::read(&archive).unwrap(), bytes, "{file}");
            assert!(leftovers(dir.path()).is_empty(), "{file}");
        }
    }

    #[test]
    fn a_rewrite_keeps_the_permissions_of_the_archive_file() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let dir = scratch("keep-mode");
            let archive = dir.path().join("m.zip");
            sample_base(&archive);
            std::fs::set_permissions(&archive, std::fs::Permissions::from_mode(0o640)).unwrap();
            delete(&archive, &[inner(&["top.txt"])], &mut Counting).unwrap();
            let mode = std::fs::metadata(&archive).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o640);
        }
    }

    #[test]
    fn the_cached_listing_is_dropped_after_a_write() {
        let dir = scratch("forget");
        let archive = dir.path().join("f.tar");
        sample_base(&archive);
        assert_eq!(listing(&archive, &[]).len(), 3);
        delete(&archive, &[inner(&["top.txt"])], &mut Counting).unwrap();
        assert_eq!(listing(&archive, &[]).len(), 2);
    }

    #[test]
    fn a_tar_rewrite_keeps_long_names_the_root_entry_and_link_targets() {
        let dir = scratch("tar-long");
        let archive = dir.path().join("long.tar");
        let long_folder = "d".repeat(120);
        let long_file = format!("{long_folder}/{}.txt", "f".repeat(120));
        let mut builder = tar::Builder::new(File::create(&archive).unwrap());
        let mut root = tar::Header::new_gnu();
        root.set_entry_type(tar::EntryType::Directory);
        root.set_size(0);
        root.set_mode(0o755);
        builder.append_data(&mut root, "./", io::empty()).unwrap();
        let mut file = tar::Header::new_gnu();
        file.set_size(4);
        file.set_mode(0o644);
        builder
            .append_data(&mut file, &long_file, &b"long"[..])
            .unwrap();
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        let target = "t".repeat(130);
        builder.append_link(&mut link, "ln", &target).unwrap();
        let mut gone = tar::Header::new_gnu();
        gone.set_size(1);
        builder
            .append_data(&mut gone, "gone.txt", &b"g"[..])
            .unwrap();
        builder.finish().unwrap();
        drop(builder);

        delete(&archive, &[inner(&["gone.txt"])], &mut Counting).unwrap();

        let mut tar = open_tar(&archive, ArchiveFormat::Tar).unwrap();
        let kept: Vec<(String, Option<String>)> = tar
            .entries()
            .unwrap()
            .map(|result| {
                let entry = result.unwrap();
                (
                    entry.path().unwrap().to_string_lossy().into_owned(),
                    entry
                        .link_name()
                        .unwrap()
                        .map(|linked| linked.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert_eq!(kept.len(), 3, "{kept:?}");
        assert!(kept.iter().any(|(name, _)| *name == long_file));
        assert!(
            kept.iter()
                .any(|(name, link_name)| name == "ln"
                    && link_name.as_deref() == Some(target.as_str()))
        );
        assert_eq!(
            bytes_of(&archive, &long_file.split('/').collect::<Vec<_>>()),
            b"long"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_folder_is_skipped_not_followed() {
        let dir = scratch("link-dir");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"s").unwrap();
        let tree = dir.path().join("tree");
        std::fs::create_dir(&tree).unwrap();
        link("../outside", &tree.join("way-out"));
        let archive = dir.path().join("t.zip");

        let report = pack(
            &[tree.clone(), tree.join("way-out")],
            &archive,
            &mut Counting,
        )
        .unwrap();
        assert_eq!(report.skipped, 2);
        assert_eq!(listing(&archive, &["tree"]), Vec::<String>::new());
    }
}
