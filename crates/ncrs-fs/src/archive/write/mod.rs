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
use crate::remote::RemotePath;
use crate::transfer::{OnConflict, Progress};

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
mod tests;
