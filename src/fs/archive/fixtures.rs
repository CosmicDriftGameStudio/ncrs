//! Archives written for the tests, one writer per format.

use std::fs::File;
use std::io::Write as _;
use std::path::Path;

use super::ArchiveFormat;

pub const ALL_FORMATS: [(&str, ArchiveFormat); 4] = [
    ("sample.zip", ArchiveFormat::Zip),
    ("sample.tar", ArchiveFormat::Tar),
    ("sample.tar.gz", ArchiveFormat::TarGz),
    ("sample.tar.zst", ArchiveFormat::TarZst),
];

/// A zip made by libarchive's bsdtar (`tar --format zip`) holding `Unterordner/`,
/// `Unterordner/Grüße.txt` ("Grüße aus dem Archiv\n") and an empty `leer.txt`:
/// what a real tool writes, unlike the zip writer the other fixtures use.
pub const BSDTAR_ZIP: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/bsdtar.zip"
));

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
    }
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
