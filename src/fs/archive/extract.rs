//! Copying an entry, or a whole folder of one, out of an archive.
//!
//! Nothing here uses an archive library's own unpack: every format goes
//! through the same name rule and the same writer, so there is one place that
//! decides what may be created.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use super::index::{open_tar, open_zip, tar_time, zip_error, zip_time, EntryKind};
use super::names::safe_components;
use super::sevenz;
use super::{index_of, locate_in_archive, ArchiveFormat, ArchiveLocation};
use crate::fs::transfer::{transfer, Counting, Move, OnConflict, Progress};

/// How much of one file is written between two checks for a cancel. Small
/// enough that Escape stops a huge file within a moment, never the whole file
/// in memory.
const CHUNK: u64 = 1 << 20;

/// Copies the archive entry at `source` into `target_dir`, keeping its name,
/// and returns how many entries it wrote.
///
/// The entry is unpacked into a hidden folder next to the target first and
/// renamed into place at the end, so a cancel or a failure leaves nothing
/// half-written under the real name.
pub fn copy_out<P: Progress>(
    source: &Path,
    target_dir: &Path,
    progress: &mut P,
    conflict: OnConflict,
) -> io::Result<usize> {
    let location = locate_in_archive(source)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not inside an archive"))?;
    let Some((name, _)) = location.inner.split_last() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the archive itself is not an entry in it",
        ));
    };
    let index = index_of(&location)?;
    let node = index
        .node(&location.inner)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not in the archive"))?;
    let total = index
        .totals(&location.inner)
        .map_or(0, |totals| totals.entries());

    if !resolve_conflict(&target_dir.join(name), conflict)? {
        return Ok(0);
    }

    let staging = tempfile::Builder::new()
        .prefix(".ncrs-partial-")
        .tempdir_in(target_dir)?;
    let plan = Plan {
        inner: &location.inner,
        wants_folder: node.is_dir,
        root_out: staging.path().join(name),
        total,
    };
    let written = unpack_into_staging(&location, &plan, progress)?;
    let when_taken = if conflict == OnConflict::Overwrite {
        OnConflict::Overwrite
    } else {
        OnConflict::Fail
    };
    transfer(&plan.root_out, target_dir, &Move, &mut Counting, when_taken)?;
    // Folders no entry names are made on the way and were counted in `total`.
    // The copy is placed by now, so a cancel arriving here changes nothing.
    let finished = total.max(written);
    let _best_effort = progress.advanced(finished, finished);
    Ok(written)
}

fn unpack_into_staging<P: Progress>(
    location: &ArchiveLocation,
    plan: &Plan,
    progress: &mut P,
) -> io::Result<usize> {
    if plan.wants_folder {
        std::fs::create_dir(&plan.root_out)?;
    }
    let written = unpack(location, plan, progress)?;
    if std::fs::symlink_metadata(&plan.root_out).is_err() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the entry is not in the archive",
        ));
    }
    Ok(written)
}

/// `false` when the item is to be skipped.
fn resolve_conflict(target: &Path, conflict: OnConflict) -> io::Result<bool> {
    match std::fs::symlink_metadata(target) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
        Ok(_) => match conflict {
            OnConflict::Fail => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", target.display()),
            )),
            OnConflict::Skip => Ok(false),
            OnConflict::Overwrite => Ok(true),
        },
    }
}

struct Plan<'a> {
    inner: &'a [String],
    /// The entry asked for is a folder, so its contents come along.
    wants_folder: bool,
    root_out: PathBuf,
    total: usize,
}

impl Plan<'_> {
    /// Where an archive entry goes in the staging folder, or `None` when it
    /// is not part of what was asked for or is not to be created.
    fn destination(&self, components: &[String], kind: EntryKind) -> io::Result<Option<PathBuf>> {
        let Some(rest) = components.strip_prefix(self.inner) else {
            return Ok(None);
        };
        let wanted = match kind {
            EntryKind::Dir => self.wants_folder,
            EntryKind::File => rest.is_empty() != self.wants_folder,
            EntryKind::Other => false,
        };
        if !wanted {
            return Ok(None);
        }
        join_inside(&self.root_out, rest).map(Some)
    }
}

/// Second line of defence after `safe_components`: every part must be a plain
/// name, so the result cannot leave `root` however the part came about.
fn join_inside(root: &Path, parts: &[String]) -> io::Result<PathBuf> {
    let mut path = root.to_path_buf();
    for part in parts {
        let mut components = Path::new(part).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) => path.push(name),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsafe name in the archive: {part}"),
                ))
            }
        }
    }
    Ok(path)
}

fn unpack<P: Progress>(
    location: &ArchiveLocation,
    plan: &Plan,
    progress: &mut P,
) -> io::Result<usize> {
    let mut writer = Writer {
        plan,
        progress,
        done: 0,
    };
    match location.format {
        ArchiveFormat::Zip => unpack_zip(&location.archive, &mut writer)?,
        ArchiveFormat::SevenZip => unpack_sevenz(&location.archive, &mut writer)?,
        ArchiveFormat::Tar | ArchiveFormat::TarGz | ArchiveFormat::TarZst => {
            unpack_tar(&location.archive, location.format, &mut writer)?;
        }
    }
    Ok(writer.done)
}

fn unpack_zip<P: Progress>(archive: &Path, writer: &mut Writer<P>) -> io::Result<()> {
    let mut zip = open_zip(archive)?;
    for position in 0..zip.len() {
        writer.progress.keep_going()?;
        // The raw entry is read first: a password-protected entry elsewhere in
        // the archive must not stop the one that was asked for.
        let (safe, kind) = {
            let raw = zip.by_index_raw(position).map_err(zip_error)?;
            let kind = if raw.is_dir() {
                EntryKind::Dir
            } else if raw.is_symlink() {
                EntryKind::Other
            } else {
                EntryKind::File
            };
            let safe = raw.name().ok().and_then(|name| safe_components(&name));
            (safe, kind)
        };
        let Some(components) = safe else {
            continue;
        };
        let Some(dest) = writer.plan.destination(&components, kind)? else {
            continue;
        };
        if kind == EntryKind::Dir {
            writer.folder(&dest)?;
        } else {
            let mut entry = zip.by_index(position).map_err(zip_error)?;
            let (mode, modified) = (entry.unix_mode(), zip_time(entry.last_modified()));
            writer.file(&dest, &mut entry, mode, modified)?;
        }
    }
    Ok(())
}

/// Solid blocks are decoded in order and cannot be entered halfway, so every
/// entry is read to its end, and only the wanted ones are written.
fn unpack_sevenz<P: Progress>(archive: &Path, writer: &mut Writer<P>) -> io::Result<()> {
    let mut reader = sevenz::open(archive)?;
    reader
        .for_each_entries(|entry, mut content| {
            visit_sevenz_entry(entry, &mut content, writer).map_err(sevenz_rust2::Error::from)?;
            Ok(true)
        })
        .map_err(sevenz::io_error)
}

fn visit_sevenz_entry<P: Progress>(
    entry: &sevenz_rust2::ArchiveEntry,
    content: &mut &mut dyn Read,
    writer: &mut Writer<P>,
) -> io::Result<()> {
    writer.progress.keep_going()?;
    let kind = sevenz::kind_of(entry);
    let destination = match safe_components(entry.name()) {
        Some(components) => writer.plan.destination(&components, kind)?,
        None => None,
    };
    match destination {
        Some(dest) if kind == EntryKind::Dir => writer.folder(&dest),
        Some(dest) => writer.file(
            &dest,
            content,
            sevenz::unix_mode(entry),
            sevenz::modified(entry),
        ),
        None => writer.skip(content),
    }
}

fn unpack_tar<P: Progress>(
    archive: &Path,
    format: ArchiveFormat,
    writer: &mut Writer<P>,
) -> io::Result<()> {
    for item in open_tar(archive, format)?.entries()? {
        writer.progress.keep_going()?;
        let mut entry = item?;
        let kind = EntryKind::of_tar(entry.header().entry_type());
        let Some(components) = safe_components(&String::from_utf8_lossy(&entry.path_bytes()))
        else {
            continue;
        };
        let Some(dest) = writer.plan.destination(&components, kind)? else {
            continue;
        };
        if kind == EntryKind::Dir {
            writer.folder(&dest)?;
        } else {
            let mode = entry.header().mode().ok();
            let modified = tar_time(entry.header().mtime());
            writer.file(&dest, &mut entry, mode, modified)?;
        }
    }
    Ok(())
}

struct Writer<'a, P> {
    plan: &'a Plan<'a>,
    progress: &'a mut P,
    done: usize,
}

impl<P: Progress> Writer<'_, P> {
    fn folder(&mut self, dest: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dest)?;
        self.step()
    }

    fn file<R: Read>(
        &mut self,
        dest: &Path,
        content: &mut R,
        mode: Option<u32>,
        modified: Option<SystemTime>,
    ) -> io::Result<()> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // The staging folder is ours and holds no links, because none is ever
        // created, so opening for write cannot be steered elsewhere.
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(dest)?;
        loop {
            self.progress.keep_going()?;
            if io::copy(&mut content.by_ref().take(CHUNK), &mut file)? == 0 {
                break;
            }
        }
        apply_mode(&file, mode)?;
        // A time that cannot be set is a nicety lost, not a failed copy.
        let _best_effort = modified.map(|time| file.set_modified(time));
        self.step()
    }

    /// Reads `content` to its end without keeping it.
    fn skip<R: Read>(&mut self, content: &mut R) -> io::Result<()> {
        loop {
            self.progress.keep_going()?;
            if io::copy(&mut content.by_ref().take(CHUNK), &mut io::sink())? == 0 {
                return Ok(());
            }
        }
    }

    fn step(&mut self) -> io::Result<()> {
        self.done += 1;
        self.progress.advanced(self.done, self.plan.total)
    }
}

#[cfg(unix)]
fn apply_mode(file: &File, mode: Option<u32>) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    match mode {
        Some(bits) => file.set_permissions(std::fs::Permissions::from_mode(bits & 0o777)),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
fn apply_mode(_file: &File, _mode: Option<u32>) -> io::Result<()> {
    Ok(())
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::super::fixtures::{
        scratch, write_archive, Item, ALL_FORMATS, BSDTAR_ARCHIVES, BSDTAR_UMLAUT_CONTENT,
        BSDTAR_UMLAUT_NAME,
    };
    use super::*;

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn sample() -> Vec<Item<'static>> {
        vec![
            Item::File("top.txt", b"top"),
            Item::File("photos/2024/a.jpg", b"aaaa"),
            Item::File("photos/b.jpg", b"bb"),
            Item::Dir("photos/empty"),
        ]
    }

    fn out_dir(root: &Path) -> PathBuf {
        let out = root.join("out");
        std::fs::create_dir(&out).unwrap();
        out
    }

    #[test]
    fn a_file_comes_out_with_its_content_in_every_format() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("file");
            let archive = dir.path().join(file);
            write_archive(&archive, &sample());
            let out = out_dir(dir.path());

            let written = copy_out(
                &archive.join("photos").join("2024").join("a.jpg"),
                &out,
                &mut Counting,
                OnConflict::Fail,
            )
            .unwrap();

            assert_eq!(written, 1, "{file}");
            assert_eq!(std::fs::read(out.join("a.jpg")).unwrap(), b"aaaa", "{file}");
            assert_eq!(names_in(&out), ["a.jpg"], "{file}");
        }
    }

    #[test]
    fn a_folder_comes_out_whole_in_every_format() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("folder");
            let archive = dir.path().join(file);
            write_archive(&archive, &sample());
            let out = out_dir(dir.path());

            let written = copy_out(
                &archive.join("photos"),
                &out,
                &mut Counting,
                OnConflict::Fail,
            )
            .unwrap();

            assert_eq!(written, 3, "{file}");
            assert_eq!(std::fs::read(out.join("photos/b.jpg")).unwrap(), b"bb");
            assert_eq!(
                std::fs::read(out.join("photos/2024/a.jpg")).unwrap(),
                b"aaaa"
            );
            assert!(out.join("photos/empty").is_dir(), "{file}");
            assert!(!out.join("top.txt").exists(), "{file}");
            assert_eq!(names_in(&out), ["photos"], "{file}");
        }
    }

    #[test]
    fn an_archive_written_by_bsdtar_comes_out_with_names_content_and_the_empty_file() {
        for (file, bytes) in BSDTAR_ARCHIVES {
            let dir = scratch("bsdtar-out");
            let archive = dir.path().join(file);
            std::fs::write(&archive, bytes).unwrap();
            let out = out_dir(dir.path());

            let folder = copy_out(
                &archive.join("Unterordner"),
                &out,
                &mut Counting,
                OnConflict::Fail,
            )
            .unwrap();
            let empty = copy_out(
                &archive.join("leer.txt"),
                &out,
                &mut Counting,
                OnConflict::Fail,
            )
            .unwrap();

            assert_eq!((folder, empty), (2, 1));
            assert_eq!(names_in(&out), ["Unterordner", "leer.txt"]);
            assert_eq!(names_in(&out.join("Unterordner")), [BSDTAR_UMLAUT_NAME]);
            assert_eq!(
                std::fs::read_to_string(out.join("Unterordner").join(BSDTAR_UMLAUT_NAME)).unwrap(),
                BSDTAR_UMLAUT_CONTENT
            );
            assert_eq!(std::fs::read(out.join("leer.txt")).unwrap(), b"");
        }
    }

    #[test]
    fn a_name_that_only_starts_like_the_folder_is_not_taken_along() {
        let dir = scratch("prefix");
        let archive = dir.path().join("p.tar");
        write_archive(
            &archive,
            &[Item::File("ab/x.txt", b"x"), Item::File("abc/y.txt", b"y")],
        );
        let out = out_dir(dir.path());

        copy_out(&archive.join("ab"), &out, &mut Counting, OnConflict::Fail).unwrap();

        assert_eq!(names_in(&out.join("ab")), ["x.txt"]);
    }

    #[test]
    fn a_missing_entry_is_not_found_and_leaves_nothing() {
        let dir = scratch("gone");
        let archive = dir.path().join("g.zip");
        write_archive(&archive, &sample());
        let out = out_dir(dir.path());

        let err =
            copy_out(&archive.join("nope"), &out, &mut Counting, OnConflict::Fail).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        let whole = copy_out(&archive, &out, &mut Counting, OnConflict::Fail).unwrap_err();
        assert_eq!(whole.kind(), io::ErrorKind::InvalidInput);
        let plain = copy_out(dir.path(), &out, &mut Counting, OnConflict::Fail).unwrap_err();
        assert_eq!(plain.kind(), io::ErrorKind::InvalidInput);
        assert!(names_in(&out).is_empty());
    }

    fn hostile() -> Vec<Item<'static>> {
        vec![
            Item::File("../evil.txt", b"evil"),
            Item::File("/abs.txt", b"abs"),
            Item::Link("link", "/etc"),
            Item::File("ok/a.txt", b"fine"),
            Item::File("ok/../../evil2.txt", b"evil"),
            Item::Link("ok/inner-link", "../../outside"),
            Item::File("ok/sub/../../evil3.txt", b"evil"),
            Item::File("C:/x.txt", b"drive"),
        ]
    }

    #[test]
    fn hostile_entries_are_left_out_and_never_written() {
        for (file, _) in ALL_FORMATS.iter().filter(|(file, _)| {
            file.ends_with(".zip") || file.ends_with(".tar.gz") || file.ends_with(".7z")
        }) {
            let dir = scratch("hostile");
            let archive = dir.path().join(file);
            write_archive(&archive, &hostile());
            let location = locate_in_archive(&archive).unwrap();
            let listing = super::super::list(&location).unwrap();
            assert_eq!(listing.left_out, 7, "{file}");

            let out = out_dir(dir.path());
            for entry in &listing.entries {
                copy_out(&entry.path, &out, &mut Counting, OnConflict::Fail).unwrap();
            }

            assert_eq!(names_in(&out), ["ok"], "{file}");
            assert_eq!(names_in(&out.join("ok")), ["a.txt"], "{file}");
            assert_eq!(
                names_in(dir.path()),
                ["out".to_owned(), (*file).to_owned()],
                "{file}"
            );
            let parent = dir.path().parent().unwrap();
            assert!(!parent.join("evil.txt").exists());
            assert!(!parent.join("evil2.txt").exists());
            assert!(!dir.path().join("evil3.txt").exists());
        }
    }

    #[test]
    fn a_solid_block_gives_each_entry_its_own_content() {
        use sevenz_rust2::{ArchiveEntry, ArchiveWriter, SourceReader};
        let dir = scratch("solid");
        let archive = dir.path().join("solid.7z");
        let mut writer = ArchiveWriter::new(File::create(&archive).unwrap()).unwrap();
        writer
            .push_archive_entries(
                vec![
                    ArchiveEntry::new_file("a/one.txt"),
                    ArchiveEntry::new_file("a/two.txt"),
                    ArchiveEntry::new_file("a/three.txt"),
                ],
                vec![
                    SourceReader::from(&b"one"[..]),
                    SourceReader::from(&b"twotwo"[..]),
                    SourceReader::from(&b"three"[..]),
                ],
            )
            .unwrap();
        writer.finish().unwrap();
        let out = out_dir(dir.path());

        copy_out(
            &archive.join("a").join("two.txt"),
            &out,
            &mut Counting,
            OnConflict::Fail,
        )
        .unwrap();
        copy_out(
            &archive.join("a").join("three.txt"),
            &out,
            &mut Counting,
            OnConflict::Fail,
        )
        .unwrap();

        assert_eq!(std::fs::read(out.join("two.txt")).unwrap(), b"twotwo");
        assert_eq!(std::fs::read(out.join("three.txt")).unwrap(), b"three");
    }

    #[test]
    fn join_inside_takes_plain_names_only() {
        let root = Path::new("/staging/x");
        let ok = join_inside(root, &["a".to_owned(), "b.txt".to_owned()]).unwrap();
        assert_eq!(ok, root.join("a").join("b.txt"));
        for bad in ["..", ".", "/abs", "a/b", ""] {
            let err = join_inside(root, &[bad.to_owned()]).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{bad:?}");
        }
    }

    #[test]
    fn a_taken_name_fails_is_skipped_or_replaced_as_asked() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("conflict");
            let archive = dir.path().join(file);
            write_archive(&archive, &sample());
            let out = out_dir(dir.path());
            std::fs::create_dir_all(out.join("photos")).unwrap();
            std::fs::write(out.join("photos/old.txt"), b"old").unwrap();
            let source = archive.join("photos");

            let err = copy_out(&source, &out, &mut Counting, OnConflict::Fail).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{file}");
            assert_eq!(names_in(&out.join("photos")), ["old.txt"], "{file}");

            assert_eq!(
                copy_out(&source, &out, &mut Counting, OnConflict::Skip).unwrap(),
                0
            );
            assert_eq!(names_in(&out.join("photos")), ["old.txt"], "{file}");

            copy_out(&source, &out, &mut Counting, OnConflict::Overwrite).unwrap();
            assert_eq!(
                names_in(&out.join("photos")),
                ["2024", "b.jpg", "empty"],
                "{file}"
            );
            assert_eq!(names_in(&out), ["photos"], "{file}");
        }
    }

    #[test]
    fn overwriting_a_single_file_replaces_its_content() {
        let dir = scratch("overwrite-file");
        let archive = dir.path().join("o.tar.zst");
        write_archive(&archive, &sample());
        let out = out_dir(dir.path());
        std::fs::write(out.join("top.txt"), b"older and longer").unwrap();

        copy_out(
            &archive.join("top.txt"),
            &out,
            &mut Counting,
            OnConflict::Overwrite,
        )
        .unwrap();

        assert_eq!(std::fs::read(out.join("top.txt")).unwrap(), b"top");
        assert_eq!(names_in(&out), ["top.txt"]);
    }

    struct StopAfter {
        steps: usize,
    }

    impl Progress for StopAfter {
        fn advanced(&mut self, done: usize, _total: usize) -> io::Result<()> {
            if done >= self.steps {
                return Err(io::Error::other("cancelled"));
            }
            Ok(())
        }
    }

    #[test]
    fn a_cancel_leaves_neither_the_target_nor_the_staging_folder() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("cancel");
            let archive = dir.path().join(file);
            write_archive(&archive, &sample());
            let out = out_dir(dir.path());

            let err = copy_out(
                &archive.join("photos"),
                &out,
                &mut StopAfter { steps: 2 },
                OnConflict::Fail,
            )
            .unwrap_err();

            assert_eq!(err.to_string(), "cancelled", "{file}");
            assert!(names_in(&out).is_empty(), "{file}: {:?}", names_in(&out));
        }
    }

    struct StopInsideFile {
        checks: usize,
    }

    impl Progress for StopInsideFile {
        fn advanced(&mut self, _done: usize, _total: usize) -> io::Result<()> {
            Ok(())
        }

        fn keep_going(&mut self) -> io::Result<()> {
            self.checks += 1;
            if self.checks > 4 {
                return Err(io::Error::other("cancelled"));
            }
            Ok(())
        }
    }

    #[test]
    fn a_cancel_stops_in_the_middle_of_a_big_file() {
        let big = vec![7; 5 * (CHUNK as usize)];
        for (file, _) in ALL_FORMATS {
            let dir = scratch("cancel-big");
            let archive = dir.path().join(file);
            write_archive(&archive, &[Item::File("big.bin", &big)]);
            let out = out_dir(dir.path());
            let mut progress = StopInsideFile { checks: 0 };

            let err = copy_out(
                &archive.join("big.bin"),
                &out,
                &mut progress,
                OnConflict::Fail,
            )
            .unwrap_err();

            assert_eq!(err.to_string(), "cancelled", "{file}");
            assert!(progress.checks <= 6, "{file}: {}", progress.checks);
            assert!(names_in(&out).is_empty(), "{file}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_permission_bits_of_a_file_survive_without_setuid() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = scratch("mode");
        let archive = dir.path().join("m.tar");
        write_archive(&archive, &[Item::File("run.sh", b"#!/bin/sh")]);
        let out = out_dir(dir.path());

        copy_out(
            &archive.join("run.sh"),
            &out,
            &mut Counting,
            OnConflict::Fail,
        )
        .unwrap();

        let mode = std::fs::metadata(out.join("run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o644);
    }
}
