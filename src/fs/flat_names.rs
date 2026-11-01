//! Names for files that land flat in one folder or archive although they come
//! from different folders, so two `invoice.pdf` do not collide.
use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The name a file gets in the target: its own, or the first free
/// `stem (n).ext` when a name already taken takes it. Names are compared
/// without case, since the target may sit on a case-insensitive file system,
/// and against the generated names too. `None` for a path without a file name.
pub fn target_name(path: &Path, taken: &mut HashSet<String>) -> Option<OsString> {
    let name = path.file_name()?;
    let mut claim = |candidate: &OsStr| taken.insert(candidate.to_string_lossy().to_lowercase());
    if claim(name) {
        return Some(name.to_owned());
    }
    let stem = path.file_stem().unwrap_or(name);
    (2..).find_map(|number: usize| {
        let mut renamed = stem.to_owned();
        renamed.push(format!(" ({number})"));
        if let Some(extension) = path.extension() {
            renamed.push(".");
            renamed.push(extension);
        }
        claim(&renamed).then_some(renamed)
    })
}

/// Whether the sources come from more than one folder, which is when equal
/// names can occur at all.
pub fn from_several_folders(sources: &[PathBuf]) -> bool {
    let mut parents = sources.iter().map(|source| source.parent());
    parents
        .next()
        .is_some_and(|first| parents.any(|parent| parent != first))
}

/// The target name per source, in the order of `sources`. Sources of one folder
/// keep their own names. Otherwise the first of every group of equal names (by
/// full path, so the result does not depend on the row order) keeps its name,
/// the others are numbered and avoid every name another source has. A source
/// without a file name yields `None`.
pub fn flat_names(sources: &[PathBuf]) -> Vec<Option<OsString>> {
    if !from_several_folders(sources) {
        return sources
            .iter()
            .map(|source| source.file_name().map(OsStr::to_owned))
            .collect();
    }
    let mut order: Vec<usize> = (0..sources.len()).collect();
    order.sort_by(|&a, &b| sources[a].cmp(&sources[b]));
    let mut taken = HashSet::new();
    let mut names: Vec<Option<OsString>> = vec![None; sources.len()];
    let mut seen = HashMap::new();
    let (firsts, others): (Vec<usize>, Vec<usize>) = order.into_iter().partition(|&index| {
        sources[index]
            .file_name()
            .is_some_and(|name| seen.insert(name.to_string_lossy().to_lowercase(), ()).is_none())
    });
    for index in firsts.into_iter().chain(others) {
        names[index] = target_name(&sources[index], &mut taken);
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    fn names(list: &[&str]) -> Vec<String> {
        flat_names(&paths(list))
            .into_iter()
            .map(|name| name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
            .collect()
    }

    #[test]
    fn equal_names_from_different_folders_are_numbered() {
        assert_eq!(
            names(&["/a/invoice.pdf", "/b/invoice.pdf", "/c/other.pdf", "/c/invoice.pdf"]),
            ["invoice.pdf", "invoice (2).pdf", "other.pdf", "invoice (3).pdf"]
        );
    }

    #[test]
    fn the_smallest_path_keeps_the_name_whatever_the_row_order() {
        assert_eq!(
            names(&["/privat/invoice.pdf", "/firma/invoice.pdf"]),
            ["invoice (2).pdf", "invoice.pdf"]
        );
    }

    #[test]
    fn names_are_compared_without_case() {
        assert_eq!(
            names(&["/a/Invoice.PDF", "/b/invoice.pdf"]),
            ["Invoice.PDF", "invoice (2).pdf"]
        );
    }

    #[test]
    fn a_number_another_source_already_has_is_skipped() {
        assert_eq!(
            names(&["/a/invoice.pdf", "/b/invoice.pdf", "/c/invoice (2).pdf"]),
            ["invoice.pdf", "invoice (3).pdf", "invoice (2).pdf"]
        );
    }

    #[test]
    fn nothing_is_numbered_when_all_sources_share_one_folder() {
        assert_eq!(names(&["/a/x.txt", "/a/X.txt"]), ["x.txt", "X.txt"]);
    }
}
