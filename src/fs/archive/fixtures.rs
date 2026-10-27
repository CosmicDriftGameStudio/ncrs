//! Archives written for the tests, one writer per format.

use std::fs::File;
use std::io::Write as _;
use std::path::Path;

use super::ArchiveFormat;

pub const ALL_FORMATS: [(&str, ArchiveFormat); 5] = [
    ("sample.zip", ArchiveFormat::Zip),
    ("sample.tar", ArchiveFormat::Tar),
    ("sample.tar.gz", ArchiveFormat::TarGz),
    ("sample.tar.zst", ArchiveFormat::TarZst),
    ("sample.7z", ArchiveFormat::SevenZip),
];

/// 7z archives with one AES-encrypted file `secret.txt`, one with the file
/// list readable and one with it encrypted too. Made with sevenz-rust2 0.24.1
/// (`aes256` on, password `x`, LZMA2 level 1) in a scratch crate, because the
/// build here has no encoder for it and 7-Zip was not installed.
pub const ENCRYPTED_7Z: [(&str, &[u8]); 2] = [
    (
        "encrypted-content.7z",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/encrypted-content.7z"
        )),
    ),
    (
        "encrypted-header.7z",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/encrypted-header.7z"
        )),
    ),
];

/// 7z archives with one small entry `a.txt` and a large dictionary, as
/// `(file name, bytes)`: 512 MiB (refused) and 64 MiB (the biggest 7-Zip
/// preset, Ultra, which must open), each packed with LZMA and with LZMA2. Made
/// once with sevenz-rust2 0.24.1 (`LzmaOptions`/`Lzma2Options::from_level(1)`
/// plus `set_dictionary_size`) and checked in, because encoding them at test
/// time costs up to 1.5 GB of memory.
pub const DICTIONARY_512M_7Z: [(&str, &[u8]); 2] = [
    (
        "dictionary-512m-lzma.7z",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/dictionary-512m-lzma.7z"
        )),
    ),
    (
        "dictionary-512m-lzma2.7z",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/dictionary-512m-lzma2.7z"
        )),
    ),
];

/// See [`DICTIONARY_512M_7Z`].
pub const DICTIONARY_64M_7Z: [(&str, &[u8]); 2] = [
    (
        "dictionary-64m-lzma.7z",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/dictionary-64m-lzma.7z"
        )),
    ),
    (
        "dictionary-64m-lzma2.7z",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/dictionary-64m-lzma2.7z"
        )),
    ),
];

/// A zip made by libarchive's bsdtar (`tar --format zip`) holding `Unterordner/`,
/// `Unterordner/Grüße.txt` ("Grüße aus dem Archiv\n") and an empty `leer.txt`:
/// what a real tool writes, unlike the zip writer the other fixtures use.
pub const BSDTAR_ZIP: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/bsdtar.zip"
));

/// The same content as [`BSDTAR_ZIP`], written by `tar --format 7zip`.
pub const BSDTAR_7Z: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/bsdtar.7z"
));

pub const BSDTAR_ARCHIVES: [(&str, &[u8]); 2] =
    [("bsdtar.zip", BSDTAR_ZIP), ("bsdtar.7z", BSDTAR_7Z)];

pub const BSDTAR_UMLAUT_NAME: &str = "Gr\u{fc}\u{df}e.txt";
pub const BSDTAR_UMLAUT_CONTENT: &str = "Gr\u{fc}\u{df}e aus dem Archiv\n";

#[derive(Debug, Clone, Copy)]
pub enum Item<'a> {
    File(&'a str, &'a [u8]),
    Dir(&'a str),
    Link(&'a str, &'a str),
}

pub fn scratch(label: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("ncrs-archive-{label}-"))
        .tempdir()
        .unwrap()
}

/// Writes `items` in the format the file name asks for. Names go in as given,
/// so a hostile `../evil.txt` ends up in the archive like in a real one.
pub fn write_archive(path: &Path, items: &[Item]) {
    let format = ArchiveFormat::of_name(path.file_name().unwrap()).unwrap();
    let file = File::create(path).unwrap();
    match format {
        ArchiveFormat::Zip => write_zip(file, items),
        ArchiveFormat::Tar => write_tar(file, items).flush().unwrap(),
        ArchiveFormat::TarGz => {
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            write_tar(encoder, items).finish().unwrap();
        }
        ArchiveFormat::TarZst => {
            let encoder = zstd::Encoder::new(file, 0).unwrap();
            write_tar(encoder, items).finish().unwrap();
        }
        ArchiveFormat::SevenZip => write_sevenz(file, items),
    }
}

/// Every file is its own block here; solid blocks are covered by
/// `a_solid_block_gives_each_entry_its_own_content` in `extract`.
fn write_sevenz(file: File, items: &[Item]) {
    use sevenz_rust2::{ArchiveEntry, ArchiveWriter, NtTime};
    let mut writer = ArchiveWriter::new(file).unwrap();
    for item in items {
        let (mut entry, data): (ArchiveEntry, Option<&[u8]>) = match *item {
            Item::File(name, data) => (ArchiveEntry::new_file(name), Some(data)),
            Item::Dir(name) => (ArchiveEntry::new_directory(name), None),
            Item::Link(name, target) => {
                let mut link = ArchiveEntry::new_file(name);
                link.has_windows_attributes = true;
                // The unix extension bit, then `S_IFLNK | 0o777` above it.
                link.windows_attributes = 0x8000 | (0o120_777 << 16);
                (link, Some(target.as_bytes()))
            }
        };
        entry.has_last_modified_date = true;
        entry.last_modified_date = NtTime::now();
        writer.push_archive_entry(entry, data).unwrap();
    }
    writer.finish().unwrap();
}

fn write_zip(file: File, items: &[Item]) {
    let options = zip::write::SimpleFileOptions::default();
    let mut writer = zip::ZipWriter::new(file);
    for item in items {
        match *item {
            Item::File(name, data) => {
                writer.start_file(name, options).unwrap();
                writer.write_all(data).unwrap();
            }
            Item::Dir(name) => writer.add_directory(name, options).unwrap(),
            Item::Link(name, target) => writer.add_symlink(name, target, options).unwrap(),
        }
    }
    writer.finish().unwrap();
}

fn write_tar<W: std::io::Write>(sink: W, items: &[Item]) -> W {
    let mut builder = tar::Builder::new(sink);
    for item in items {
        match *item {
            Item::File(name, data) => {
                append(&mut builder, name, tar::EntryType::Regular, None, data);
            }
            Item::Dir(name) => append(&mut builder, name, tar::EntryType::Directory, None, b""),
            Item::Link(name, target) => {
                append(
                    &mut builder,
                    name,
                    tar::EntryType::Symlink,
                    Some(target),
                    b"",
                );
            }
        }
    }
    builder.into_inner().unwrap()
}

/// `Builder::append_data` refuses a `..` name; filling the raw header does not.
fn append<W: std::io::Write>(
    builder: &mut tar::Builder<W>,
    name: &str,
    kind: tar::EntryType,
    link: Option<&str>,
    data: &[u8],
) {
    let mut header = tar::Header::new_gnu();
    header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
    if let Some(target) = link {
        header.set_link_name(target).unwrap();
    }
    header.set_entry_type(kind);
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(1_700_000_000);
    header.set_cksum();
    builder.append(&header, data).unwrap();
}
