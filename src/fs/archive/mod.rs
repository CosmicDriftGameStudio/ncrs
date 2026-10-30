//! Archives read like folders. A path inside an archive is virtual, the
//! archive file followed by the way through it (`/x/fotos.zip/2024/a.jpg`), so
//! the rest of the app needs no path type of its own for it.
//!
//! Reading never changes an archive. Writing (`pack`, `add`, `delete`) builds
//! a complete new archive next to it and replaces the old one at the end.

mod extract;
mod index;
mod names;
mod sevenz;
mod view;
mod write;

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

pub use extract::copy_out;
pub use index::{ArchiveIndex, Totals};
pub use names::safe_components;
pub use sevenz::{refusal_of, SevenZipRefusal};
pub use view::restrict_to_owner;
pub use view::{extract_for_view, ViewCopies};
pub use write::{add, delete, pack, WriteReport};

use super::FileEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    TarZst,
    SevenZip,
}

impl ArchiveFormat {
    pub fn of_name(name: &OsStr) -> Option<Self> {
        let lowered = name.to_string_lossy().to_lowercase();
        let ends = |suffixes: &[&str]| suffixes.iter().any(|suffix| lowered.ends_with(suffix));
        if ends(&[".zip"]) {
            Some(Self::Zip)
        } else if ends(&[".tar"]) {
            Some(Self::Tar)
        } else if ends(&[".tar.gz", ".tgz"]) {
            Some(Self::TarGz)
        } else if ends(&[".tar.zst", ".tzst"]) {
            Some(Self::TarZst)
        } else if ends(&[".7z"]) {
            Some(Self::SevenZip)
        } else {
            None
        }
    }
}

pub fn is_archive_name(name: &OsStr) -> bool {
    ArchiveFormat::of_name(name).is_some()
}

/// Blocking: asks the filesystem whether `path` is a file.
pub fn is_archive_file(path: &Path) -> bool {
    path.file_name().is_some_and(is_archive_name)
        && std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveLocation {
    pub archive: PathBuf,
    pub format: ArchiveFormat,
    /// The way through the archive; empty at its root.
    pub inner: Vec<String>,
}

/// Where in an archive `path` points, or `None` when it is no archive path.
/// Blocking.
///
/// The first ancestor that exists decides: a path under a real folder is never
/// an archive path, whatever its name looks like.
pub fn locate_in_archive(path: &Path) -> Option<ArchiveLocation> {
    let (archive, meta) = path
        .ancestors()
        .find_map(|ancestor| Some((ancestor, std::fs::metadata(ancestor).ok()?)))?;
    if !meta.is_file() {
        return None;
    }
    let format = ArchiveFormat::of_name(archive.file_name()?)?;
    let inner = path
        .strip_prefix(archive)
        .ok()?
        .components()
        .map(|part| part.as_os_str().to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    Some(ArchiveLocation {
        archive: archive.to_path_buf(),
        format,
        inner,
    })
}

#[derive(Debug)]
pub struct ArchiveListing {
    pub entries: Vec<FileEntry>,
    pub left_out: usize,
}

/// The entries directly inside `location`.
pub fn list(location: &ArchiveLocation) -> io::Result<ArchiveListing> {
    let index = index_of(location)?;
    index
        .node(&location.inner)
        .filter(|node| node.is_dir)
        .ok_or_else(not_in_archive)?;
    let folder = location
        .inner
        .iter()
        .fold(location.archive.clone(), |path, part| path.join(part));
    let entries = index
        .children_of(&location.inner)
        .into_iter()
        .flatten()
        .map(|(name, node)| FileEntry {
            name: OsString::from(name),
            path: folder.join(name),
            is_dir: node.is_dir,
            is_symlink: false,
            is_parent: false,
            size: if node.is_dir { 0 } else { node.size },
            modified: node.modified,
            folder_size: None,
        })
        .collect();
    Ok(ArchiveListing {
        entries,
        left_out: index.left_out(),
    })
}

/// What copying the archive entry at `path` would move, for the transfer
/// dialog. `None` when `path` is no entry of an archive.
pub fn measure(path: &Path) -> Option<Totals> {
    let location = locate_in_archive(path).filter(|location| !location.inner.is_empty())?;
    index_of(&location).ok()?.totals(&location.inner)
}

fn not_in_archive() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "not in the archive")
}

type IndexKey = (PathBuf, u64, Option<SystemTime>);

// A tar.gz has to be decompressed from the start to find any one folder, so
// without keeping the index every level the user enters would unpack it again.
static RECENT: Mutex<Vec<(IndexKey, Arc<ArchiveIndex>)>> = Mutex::new(Vec::new());
const RECENT_LIMIT: usize = 4;

/// Keyed by size and modification time, so a replaced archive is read again.
fn index_of(location: &ArchiveLocation) -> io::Result<Arc<ArchiveIndex>> {
    let meta = std::fs::metadata(&location.archive)?;
    let key = (location.archive.clone(), meta.len(), meta.modified().ok());
    if let Some(hit) = recently_used(&key) {
        return Ok(hit);
    }
    let index = Arc::new(ArchiveIndex::build(&location.archive, location.format)?);
    let mut recent = RECENT.lock().unwrap_or_else(PoisonError::into_inner);
    recent.retain(|(known, _)| *known != key);
    recent.push((key, Arc::clone(&index)));
    if recent.len() > RECENT_LIMIT {
        recent.remove(0);
    }
    Ok(index)
}

fn forget(archive: &Path) {
    let mut recent = RECENT.lock().unwrap_or_else(PoisonError::into_inner);
    recent.retain(|((known, _, _), _)| known != archive);
}

fn recently_used(key: &IndexKey) -> Option<Arc<ArchiveIndex>> {
    let mut recent = RECENT.lock().unwrap_or_else(PoisonError::into_inner);
    let position = recent.iter().position(|(known, _)| known == key)?;
    let hit = recent.remove(position);
    let index = Arc::clone(&hit.1);
    recent.push(hit);
    Some(index)
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
pub mod fixtures;

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::fixtures::{
        scratch, write_archive, Item, ALL_FORMATS, BSDTAR_ARCHIVES, BSDTAR_UMLAUT_NAME,
        DICTIONARY_512M_7Z, DICTIONARY_64M_7Z, ENCRYPTED_7Z,
    };
    use super::*;

    fn name(text: &str) -> OsString {
        OsString::from(text)
    }

    #[test]
    fn formats_are_told_apart_by_their_ending_in_any_case() {
        let table = [
            ("a.zip", Some(ArchiveFormat::Zip)),
            ("A.ZIP", Some(ArchiveFormat::Zip)),
            ("a.tar", Some(ArchiveFormat::Tar)),
            ("a.tar.gz", Some(ArchiveFormat::TarGz)),
            ("a.TGZ", Some(ArchiveFormat::TarGz)),
            ("a.tar.zst", Some(ArchiveFormat::TarZst)),
            ("a.tzst", Some(ArchiveFormat::TarZst)),
            ("a.gz", None),
            ("a.7z", Some(ArchiveFormat::SevenZip)),
            ("A.7Z", Some(ArchiveFormat::SevenZip)),
            ("a.7z.txt", None),
            ("zip", None),
            ("a.tar.bz2", None),
            ("a.zip.txt", None),
        ];
        for (file, expected) in table {
            assert_eq!(ArchiveFormat::of_name(&name(file)), expected, "{file}");
            assert_eq!(is_archive_name(&name(file)), expected.is_some(), "{file}");
        }
    }

    fn sample_items() -> Vec<Item<'static>> {
        vec![
            Item::File("top.txt", b"top"),
            Item::File("2024/jan/a.jpg", b"aaaa"),
            Item::File("2024/b.jpg", b"bb"),
            Item::Dir("empty"),
        ]
    }

    fn at(archive: &Path, inner: &[&str]) -> ArchiveLocation {
        ArchiveLocation {
            archive: archive.to_path_buf(),
            format: ArchiveFormat::of_name(archive.file_name().unwrap()).unwrap(),
            inner: inner.iter().map(|part| (*part).to_owned()).collect(),
        }
    }

    fn names_in(location: &ArchiveLocation) -> Vec<(String, bool)> {
        list(location)
            .unwrap()
            .entries
            .iter()
            .map(|entry| (entry.name.to_string_lossy().into_owned(), entry.is_dir))
            .collect()
    }

    #[test]
    fn every_format_lists_folders_level_by_level_with_implicit_parents() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("list");
            let archive = dir.path().join(file);
            write_archive(&archive, &sample_items());

            assert_eq!(
                names_in(&at(&archive, &[])),
                [
                    ("2024".to_owned(), true),
                    ("empty".to_owned(), true),
                    ("top.txt".to_owned(), false)
                ],
                "{file}"
            );
            assert_eq!(
                names_in(&at(&archive, &["2024"])),
                [("b.jpg".to_owned(), false), ("jan".to_owned(), true)],
                "{file}"
            );
            assert_eq!(
                names_in(&at(&archive, &["2024", "jan"])),
                [("a.jpg".to_owned(), false)],
                "{file}"
            );
            assert_eq!(names_in(&at(&archive, &["empty"])), [], "{file}");
        }
    }

    #[test]
    fn an_archive_written_by_bsdtar_lists_its_folder_umlaut_name_and_empty_file() {
        for (file, bytes) in BSDTAR_ARCHIVES {
            let dir = scratch("bsdtar-list");
            let archive = dir.path().join(file);
            std::fs::write(&archive, bytes).unwrap();

            assert_eq!(
                names_in(&at(&archive, &[])),
                [
                    ("Unterordner".to_owned(), true),
                    ("leer.txt".to_owned(), false)
                ]
            );
            let inside = list(&at(&archive, &["Unterordner"])).unwrap();
            let names: Vec<_> = inside
                .entries
                .iter()
                .map(|entry| (entry.name.to_string_lossy().into_owned(), entry.size))
                .collect();
            assert_eq!(names, [(BSDTAR_UMLAUT_NAME.to_owned(), 23)]);
            let top = list(&at(&archive, &[])).unwrap();
            let empty_file = top.entries.iter().find(|entry| entry.name == "leer.txt");
            assert_eq!(empty_file.map(|entry| entry.size), Some(0));
        }
    }

    #[test]
    fn entries_carry_their_virtual_path_and_size() {
        let dir = scratch("paths");
        let archive = dir.path().join("p.tar");
        write_archive(&archive, &sample_items());

        let listing = list(&at(&archive, &["2024", "jan"])).unwrap();
        let entry = listing.entries.first().unwrap();
        assert_eq!(entry.path, archive.join("2024").join("jan").join("a.jpg"));
        assert_eq!(entry.size, 4);
        assert!(!entry.is_parent && !entry.is_symlink);
        assert!(entry.modified.is_some());
    }

    #[test]
    fn a_missing_folder_or_a_file_is_not_found() {
        let dir = scratch("missing");
        let archive = dir.path().join("m.zip");
        write_archive(&archive, &sample_items());

        for inner in [&["nope"][..], &["top.txt"][..], &["2024", "x"][..]] {
            let err = list(&at(&archive, inner)).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::NotFound, "{inner:?}");
        }
    }

    #[test]
    fn a_broken_archive_is_an_error_not_a_panic() {
        let dir = scratch("broken");
        for (file, _) in ALL_FORMATS {
            let archive = dir.path().join(file);
            std::fs::write(&archive, b"this is not an archive at all, just text").unwrap();
            assert!(list(&at(&archive, &[])).is_err(), "{file}");
        }
    }

    #[test]
    fn an_encrypted_7z_is_refused_with_its_own_error_for_listing_and_copying() {
        for (file, bytes) in ENCRYPTED_7Z {
            let dir = scratch("encrypted");
            let archive = dir.path().join(file);
            std::fs::write(&archive, bytes).unwrap();

            let listed = list(&at(&archive, &[])).unwrap_err();
            assert_eq!(
                refusal_of(&listed),
                Some(SevenZipRefusal::Encrypted),
                "{file}: {listed}"
            );

            let out = dir.path().join("out");
            std::fs::create_dir(&out).unwrap();
            let copied = copy_out(
                &archive.join("secret.txt"),
                &out,
                &mut crate::fs::transfer::Counting,
                crate::fs::transfer::OnConflict::Fail,
            )
            .unwrap_err();
            assert_eq!(
                refusal_of(&copied),
                Some(SevenZipRefusal::Encrypted),
                "{file}: {copied}"
            );
            assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0, "{file}");
        }
    }

    #[test]
    fn a_7z_with_a_huge_dictionary_is_refused_but_the_largest_preset_opens() {
        let dir = scratch("dictionary");
        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();
        for (file, bytes) in DICTIONARY_512M_7Z {
            let huge = dir.path().join(file);
            std::fs::write(&huge, bytes).unwrap();
            let listed = list(&at(&huge, &[])).unwrap_err();
            assert_eq!(
                refusal_of(&listed),
                Some(SevenZipRefusal::DictionaryTooLarge),
                "{file}: {listed}"
            );
            let copied = copy_out(
                &huge.join("a.txt"),
                &out,
                &mut crate::fs::transfer::Counting,
                crate::fs::transfer::OnConflict::Fail,
            )
            .unwrap_err();
            assert_eq!(
                refusal_of(&copied),
                Some(SevenZipRefusal::DictionaryTooLarge),
                "{file}: {copied}"
            );
        }
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);

        for (file, bytes) in DICTIONARY_64M_7Z {
            let ultra = dir.path().join(file);
            std::fs::write(&ultra, bytes).unwrap();
            assert_eq!(
                names_in(&at(&ultra, &[])),
                [("a.txt".to_owned(), false)],
                "{file}"
            );
        }
    }

    #[test]
    fn a_broken_7z_is_not_called_encrypted() {
        let dir = scratch("not-encrypted");
        let archive = dir.path().join("b.7z");
        std::fs::write(&archive, b"7z but not really, just some text here").unwrap();
        assert_eq!(refusal_of(&list(&at(&archive, &[])).unwrap_err()), None);
    }

    #[test]
    fn a_path_is_located_by_its_first_existing_ancestor() {
        let dir = scratch("locate");
        let archive = dir.path().join("fotos.zip");
        write_archive(&archive, &sample_items());

        let found = locate_in_archive(&archive.join("2024").join("b.jpg")).unwrap();
        assert_eq!(found.archive, archive);
        assert_eq!(found.format, ArchiveFormat::Zip);
        assert_eq!(found.inner, ["2024", "b.jpg"]);
        assert_eq!(
            locate_in_archive(&archive).unwrap().inner,
            Vec::<String>::new()
        );

        assert_eq!(locate_in_archive(dir.path()), None);
        assert_eq!(
            locate_in_archive(&dir.path().join("nothing.zip").join("x")),
            None
        );

        let folder_named_like_an_archive = dir.path().join("folder.zip");
        std::fs::create_dir(&folder_named_like_an_archive).unwrap();
        assert_eq!(
            locate_in_archive(&folder_named_like_an_archive.join("x")),
            None
        );
        let plain = dir.path().join("plain.txt");
        std::fs::write(&plain, b"x").unwrap();
        assert_eq!(locate_in_archive(&plain.join("x")), None);
    }

    #[test]
    fn is_archive_file_needs_a_real_file_with_an_archive_name() {
        let dir = scratch("is-file");
        let archive = dir.path().join("a.zip");
        write_archive(&archive, &sample_items());
        let folder = dir.path().join("b.zip");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(dir.path().join("c.txt"), b"x").unwrap();

        assert!(is_archive_file(&archive));
        assert!(!is_archive_file(&folder));
        assert!(!is_archive_file(&dir.path().join("c.txt")));
        assert!(!is_archive_file(&dir.path().join("gone.zip")));
    }

    #[test]
    fn apple_double_entries_are_not_listed_or_counted_in_any_format() {
        for (file, _) in ALL_FORMATS {
            let dir = scratch("apple");
            let archive = dir.path().join(file);
            write_archive(&archive, &apple_items());

            assert_eq!(
                names_in(&at(&archive, &[])),
                [("a.txt".to_owned(), false), ("dir".to_owned(), true)],
                "{file}"
            );
            assert_eq!(
                names_in(&at(&archive, &["dir"])),
                [("b.txt".to_owned(), false)],
                "{file}"
            );
            assert_eq!(list(&at(&archive, &[])).unwrap().left_out, 0, "{file}");
            let index = index_of(&at(&archive, &[])).unwrap();
            assert_eq!(index.totals(&[]).unwrap().files, 2, "{file}");
        }
    }

    fn apple_items() -> Vec<Item<'static>> {
        vec![
            Item::File("a.txt", b"a"),
            Item::File("dir/b.txt", b"b"),
            Item::File("__MACOSX/._a.txt", b"x"),
            Item::File("__MACOSX/dir/._b.txt", b"x"),
            Item::File("._a.txt", b"x"),
            Item::File("dir/._b.txt", b"x"),
        ]
    }

    #[test]
    fn measure_counts_what_a_copy_would_take() {
        let dir = scratch("measure");
        let archive = dir.path().join("m.tar.gz");
        write_archive(&archive, &sample_items());

        let year = measure(&archive.join("2024")).unwrap();
        assert_eq!((year.files, year.folders, year.bytes), (2, 2, 6));
        let file = measure(&archive.join("top.txt")).unwrap();
        assert_eq!((file.files, file.folders, file.bytes), (1, 0, 3));
        assert_eq!(measure(&archive), None);
        assert_eq!(measure(&archive.join("missing")), None);
        assert_eq!(measure(dir.path()), None);
    }

    #[test]
    fn a_replaced_archive_is_read_again() {
        let dir = scratch("replaced");
        let archive = dir.path().join("r.tar");
        write_archive(&archive, &[Item::File("one.txt", b"1")]);
        assert_eq!(
            names_in(&at(&archive, &[])),
            [("one.txt".to_owned(), false)]
        );

        write_archive(
            &archive,
            &[Item::File("one.txt", b"1"), Item::File("two.txt", b"22")],
        );
        assert_eq!(names_in(&at(&archive, &[])).len(), 2);
    }

    #[test]
    fn only_the_most_recent_archives_stay_cached() {
        let dir = scratch("lru");
        for number in 0..=RECENT_LIMIT + 2 {
            let archive = dir.path().join(format!("{number}.tar"));
            write_archive(&archive, &[Item::File("f.txt", b"x")]);
            assert_eq!(names_in(&at(&archive, &[])).len(), 1);
        }
        let held = RECENT.lock().unwrap_or_else(PoisonError::into_inner).len();
        assert!(held <= RECENT_LIMIT, "{held}");
    }
}
