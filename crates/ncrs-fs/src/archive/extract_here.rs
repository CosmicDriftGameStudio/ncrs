//! Unpacking a whole archive next to itself, the way the macOS Archive Utility
//! does: one top-level entry lands as it is, anything else in a new folder
//! named after the archive.

use std::io;
use std::path::{Path, PathBuf};

use super::extract::{unpack, Plan};
use super::{index_of, locate_in_archive, without_archive_ending};
use crate::transfer::Progress;

#[derive(Debug)]
pub struct Extracted {
    /// The file or folder that is new next to the archive.
    pub placed: PathBuf,
    pub written: usize,
}

/// Unpacks `archive` next to itself and returns what was made. Nothing
/// appears under its final name until everything is written, and a taken
/// name is never overwritten: it becomes `x 2`, `x 3`.
pub fn extract_here<P: Progress>(archive: &Path, progress: &mut P) -> io::Result<Extracted> {
    let location = locate_in_archive(archive)
        .filter(|location| location.inner.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not an archive file"))?;
    let parent = archive
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the archive has no folder"))?;
    let index = index_of(&location)?;
    let total = index
        .totals(&[])
        .map_or(0, |totals| totals.entries().saturating_sub(1));
    let single_top_entry = index
        .children_of(&[])
        .filter(|top| top.len() == 1)
        .and_then(|top| top.iter().next())
        .map(|(name, node)| (name.clone(), node.is_dir));

    let staging = tempfile::Builder::new()
        .prefix(".ncrs-partial-")
        .tempdir_in(parent)?;
    let content = staging.path().join("content");
    std::fs::create_dir(&content)?;
    let plan = Plan::new(&[], true, content.clone(), total);
    let written = unpack(&location, &plan, progress)?;

    let (from, wanted, is_dir) = match single_top_entry {
        Some((name, is_dir)) => (content.join(&name), name, is_dir),
        None => {
            let file_name = archive.file_name().map(|each| each.to_string_lossy());
            let stem =
                file_name.map_or_else(String::new, |each| without_archive_ending(&each).to_owned());
            (content, stem, true)
        }
    };
    let placed = free_name_in(parent, &wanted, is_dir);
    std::fs::rename(&from, &placed)?;
    let finished = total.max(written);
    let _best_effort = progress.advanced(finished, finished);
    Ok(Extracted { placed, written })
}

fn free_name_in(folder: &Path, wanted: &str, is_dir: bool) -> PathBuf {
    let first = folder.join(wanted);
    if std::fs::symlink_metadata(&first).is_err() {
        return first;
    }
    let (stem, extension) = match Path::new(wanted).extension() {
        Some(extension) if !is_dir => (
            Path::new(wanted)
                .file_stem()
                .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()),
            format!(".{}", extension.to_string_lossy()),
        ),
        _ => (wanted.to_owned(), String::new()),
    };
    (2..)
        .map(|number| folder.join(format!("{stem} {number}{extension}")))
        .find(|candidate| std::fs::symlink_metadata(candidate).is_err())
        .unwrap_or(first)
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::super::fixtures::{scratch, write_archive, Item};
    use super::*;
    use crate::transfer::Counting;

    const FORMATS: [&str; 2] = ["pack.zip", "pack.tar.gz"];

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn one_top_level_folder_is_unpacked_as_it_is() {
        for file in FORMATS {
            let dir = scratch("single");
            let archive = dir.path().join(file);
            write_archive(
                &archive,
                &[
                    Item::File("album/a.jpg", b"a"),
                    Item::File("album/b/c.jpg", b"c"),
                ],
            );

            let done = extract_here(&archive, &mut Counting).unwrap();

            assert_eq!(done.placed, dir.path().join("album"), "{file}");
            assert_eq!(
                std::fs::read(dir.path().join("album/b/c.jpg")).unwrap(),
                b"c"
            );
            assert_eq!(names_in(dir.path()), ["album", file], "{file}");
        }
    }

    #[test]
    fn one_top_level_file_lands_next_to_the_archive() {
        let dir = scratch("single-file");
        let archive = dir.path().join("pack.zip");
        write_archive(&archive, &[Item::File("note.txt", b"hi")]);

        let done = extract_here(&archive, &mut Counting).unwrap();

        assert_eq!(done.placed, dir.path().join("note.txt"));
        assert_eq!(names_in(dir.path()), ["note.txt", "pack.zip"]);
    }

    #[test]
    fn several_top_level_entries_go_into_a_folder_named_after_the_archive() {
        for file in FORMATS {
            let dir = scratch("many");
            let archive = dir.path().join(file);
            write_archive(
                &archive,
                &[Item::File("a.txt", b"a"), Item::File("sub/b.txt", b"b")],
            );

            let done = extract_here(&archive, &mut Counting).unwrap();

            assert_eq!(done.placed, dir.path().join("pack"), "{file}");
            assert_eq!(std::fs::read(done.placed.join("sub/b.txt")).unwrap(), b"b");
            assert_eq!(names_in(dir.path()), ["pack", file], "{file}");
        }
    }

    #[test]
    fn a_taken_name_gets_a_number_and_nothing_is_overwritten() {
        for file in FORMATS {
            let dir = scratch("taken");
            let archive = dir.path().join(file);
            write_archive(
                &archive,
                &[Item::File("a.txt", b"new"), Item::File("b.txt", b"new")],
            );
            std::fs::create_dir(dir.path().join("pack")).unwrap();
            std::fs::create_dir(dir.path().join("pack 2")).unwrap();

            let done = extract_here(&archive, &mut Counting).unwrap();

            assert_eq!(done.placed, dir.path().join("pack 3"), "{file}");
            assert!(names_in(&dir.path().join("pack")).is_empty());
        }
    }

    #[test]
    fn a_taken_file_name_keeps_its_ending() {
        let dir = scratch("taken-file");
        let archive = dir.path().join("pack.zip");
        write_archive(&archive, &[Item::File("note.txt", b"new")]);
        std::fs::write(dir.path().join("note.txt"), b"old").unwrap();

        let done = extract_here(&archive, &mut Counting).unwrap();

        assert_eq!(done.placed, dir.path().join("note 2.txt"));
        assert_eq!(std::fs::read(dir.path().join("note.txt")).unwrap(), b"old");
    }
}
