//! Carrying the entries of the old archive over into the new one.

use std::fs::File;
use std::io::{self, BufWriter, Read};
use std::path::Path;

use super::super::index::{open_tar, open_zip, zip_error};
use super::super::names::safe_components;
use super::super::sevenz;
use super::super::ArchiveFormat;
use super::{Job, Selector, Sink, TarOut, Watched};

pub(super) fn copy_original(
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
    writer
        .set_raw_comment(zip.comment().into())
        .map_err(zip_error)?;
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

/// The extended header records of the entry that go into the new archive. The
/// path, link path and size are written again by the builder from what it is
/// given; everything else (times, owner, extended attributes) is carried over.
fn pax_records<R: Read>(entry: &mut tar::Entry<'_, R>) -> io::Result<Vec<(String, Vec<u8>)>> {
    let Some(records) = entry.pax_extensions()? else {
        return Ok(Vec::new());
    };
    let mut kept = Vec::new();
    for record in records {
        let record = record?;
        let key = record
            .key()
            .map_err(|_| invalid("an extended header with a key that is not text"))?;
        if key.starts_with("GNU.sparse") {
            return Err(invalid("a sparse entry, which ncrs cannot rewrite"));
        }
        if matches!(key, "path" | "linkpath" | "size") {
            continue;
        }
        kept.push((key.to_owned(), record.value_bytes().to_vec()));
    }
    Ok(kept)
}

fn invalid(text: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, text.to_owned())
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
        if kind == tar::EntryType::GNUSparse {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{} is a sparse entry, which ncrs cannot rewrite",
                    path.display()
                ),
            ));
        }
        let records = pax_records(&mut entry)
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
        builder.append_pax_extensions(
            records
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_slice())),
        )?;
        match kind {
            tar::EntryType::Symlink | tar::EntryType::Link => {
                let target = entry
                    .link_name()?
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "a link without a target")
                    })?
                    .into_owned();
                // A hard link has no content of its own: with its target gone
                // it would point at nothing, or at the file that replaced it.
                if kind == tar::EntryType::Link {
                    let linked = safe_components(&target.to_string_lossy());
                    if linked.is_some_and(|parts| selector.drops(&parts)) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "{} is a hard link to {}, which this change would remove",
                                path.display(),
                                target.display()
                            ),
                        ));
                    }
                }
                builder.append_link(&mut header, &path, &target)?;
            }
            _ => {
                let size = entry.size();
                header.set_size(size);
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

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::{self, Write as _};
    use std::path::Path;

    use super::super::super::fixtures::{scratch, write_archive, Item};
    use super::super::{add, delete};
    use crate::transfer::{Counting, OnConflict};

    fn tar_with_hard_link(path: &Path) {
        let mut builder = tar::Builder::new(File::create(path).unwrap());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(4);
        header.set_mode(0o644);
        builder
            .append_data(&mut header, "a.txt", &b"data"[..])
            .unwrap();
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Link);
        link.set_size(0);
        link.set_mode(0o644);
        builder.append_link(&mut link, "b.txt", "a.txt").unwrap();
        builder.into_inner().unwrap().flush().unwrap();
    }

    #[test]
    fn deleting_what_a_hard_link_points_at_is_refused_and_changes_nothing() {
        let dir = scratch("hardlink");
        let archive = dir.path().join("h.tar");
        tar_with_hard_link(&archive);
        let before = std::fs::read(&archive).unwrap();

        let err = delete(&archive, &[vec!["a.txt".to_owned()]], &mut Counting).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("hard link"), "{err}");
        assert_eq!(std::fs::read(&archive).unwrap(), before);
    }

    #[test]
    fn extended_header_records_survive_a_rewrite() {
        let dir = scratch("pax");
        let archive = dir.path().join("p.tar");
        let mut builder = tar::Builder::new(File::create(&archive).unwrap());
        builder
            .append_pax_extensions([
                ("SCHILY.xattr.user.tag", &b"blue"[..]),
                ("mtime", &b"1700000000.5"[..]),
            ])
            .unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(1);
        header.set_mode(0o644);
        builder
            .append_data(&mut header, "x.txt", &b"x"[..])
            .unwrap();
        builder.into_inner().unwrap().flush().unwrap();
        let note = dir.path().join("note.txt");
        std::fs::write(&note, b"n").unwrap();

        add(&archive, &[], &[note], OnConflict::Fail, &mut Counting).unwrap();

        let mut found = Vec::new();
        for entry in tar::Archive::new(File::open(&archive).unwrap())
            .entries()
            .unwrap()
        {
            let mut entry = entry.unwrap();
            if entry.path().unwrap().to_string_lossy() == "x.txt" {
                for record in entry.pax_extensions().unwrap().unwrap() {
                    let record = record.unwrap();
                    found.push((
                        record.key().unwrap().to_owned(),
                        record.value().unwrap().to_owned(),
                    ));
                }
            }
        }
        assert!(found.contains(&("SCHILY.xattr.user.tag".to_owned(), "blue".to_owned())));
        assert!(found.contains(&("mtime".to_owned(), "1700000000.5".to_owned())));
    }

    #[test]
    fn the_comment_of_a_zip_survives_a_rewrite() {
        let dir = scratch("zip-comment");
        let archive = dir.path().join("c.zip");
        write_archive(&archive, &[Item::File("a.txt", b"a")]);
        let mut writer = zip::ZipWriter::new_append(
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&archive)
                .unwrap(),
        )
        .unwrap();
        writer
            .set_raw_comment(b"from the sender".as_slice().into())
            .unwrap();
        writer.finish().unwrap();
        let note = dir.path().join("note.txt");
        std::fs::write(&note, b"n").unwrap();

        add(&archive, &[], &[note], OnConflict::Fail, &mut Counting).unwrap();

        let zip = zip::ZipArchive::new(File::open(&archive).unwrap()).unwrap();
        assert_eq!(zip.comment(), b"from the sender");
    }

    #[test]
    fn a_file_does_not_overwrite_a_folder_of_the_archive() {
        let dir = scratch("file-over-folder");
        let archive = dir.path().join("f.zip");
        write_archive(&archive, &[Item::File("2024/b.jpg", b"bb")]);
        let file = dir.path().join("2024");
        std::fs::write(&file, b"x").unwrap();
        let before = std::fs::read(&archive).unwrap();

        let err = add(&archive, &[], &[file], OnConflict::Overwrite, &mut Counting).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::IsADirectory);
        assert_eq!(std::fs::read(&archive).unwrap(), before);
    }
}
