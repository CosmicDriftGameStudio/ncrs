//! Names for files that land flat in one folder or archive although they come
//! from different folders, so two `invoice.pdf` do not collide.
use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

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
    let mut order: Vec<(usize, &PathBuf)> = sources.iter().enumerate().collect();
    order.sort_by(|a, b| a.1.cmp(b.1));
    let mut taken = HashSet::new();
    let mut names: Vec<Option<OsString>> = vec![None; sources.len()];
    let mut seen = HashSet::new();
    let (firsts, others): (Vec<_>, Vec<_>) = order.into_iter().partition(|(_, source)| {
        source
            .file_name()
            .is_some_and(|name| seen.insert(name.to_string_lossy().to_lowercase()))
    });
    for (index, source) in firsts.into_iter().chain(others) {
        if let Some(slot) = names.get_mut(index) {
            *slot = target_name(source, &mut taken);
        }
    }
    names
}

/// Every source whose name is shared with another one, with the name it gets,
/// in the order of `sources`. Empty when the sources come from one folder.
pub fn members_of_shared_names(sources: &[PathBuf]) -> Vec<(PathBuf, OsString)> {
    let mut count: HashMap<String, usize> = HashMap::new();
    let lowered = |source: &Path| {
        source
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    };
    for source in sources {
        if let Some(name) = lowered(source) {
            *count.entry(name).or_default() += 1;
        }
    }
    let several = from_several_folders(sources);
    flat_names(sources)
        .into_iter()
        .zip(sources)
        .filter(|(_, source)| {
            several
                && lowered(source).is_some_and(|name| count.get(&name).copied().unwrap_or(0) > 1)
        })
        .filter_map(|(name, source)| Some((source.clone(), name?)))
        .collect()
}

/// The entry name per source for an archive: the flat numbered name, or with
/// `keep_under` the path relative to that folder, `/`-joined. A source outside
/// the folder, or whose relative path is not made of plain names, falls back to
/// its flat name, which never takes a name another entry has.
pub fn archive_names(sources: &[PathBuf], keep_under: Option<&Path>) -> Vec<(PathBuf, String)> {
    let flat = flat_names(sources);
    let relative = |source: &Path| -> Option<String> {
        let parts = source
            .strip_prefix(keep_under?)
            .ok()?
            .components()
            .map(|component| match component {
                Component::Normal(part) => part.to_str(),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        (!parts.is_empty()).then(|| parts.join("/"))
    };
    let kept: Vec<Option<String>> = sources.iter().map(|source| relative(source)).collect();
    let mut taken: HashSet<String> = kept
        .iter()
        .flatten()
        .map(|name| name.to_lowercase())
        .collect();
    sources
        .iter()
        .zip(kept)
        .zip(flat)
        .map(|((source, kept_name), flat_name)| {
            let name = match kept_name {
                Some(name) => name,
                None if keep_under.is_some() => target_name(source, &mut taken)
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                None => flat_name
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            };
            (source.clone(), name)
        })
        .collect()
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
            .map(|name| {
                name.map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect()
    }

    #[test]
    fn equal_names_from_different_folders_are_numbered() {
        assert_eq!(
            names(&[
                "/a/invoice.pdf",
                "/b/invoice.pdf",
                "/c/other.pdf",
                "/c/invoice.pdf"
            ]),
            [
                "invoice.pdf",
                "invoice (2).pdf",
                "other.pdf",
                "invoice (3).pdf"
            ]
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

    fn archive(list: &[&str], keep_under: Option<&str>) -> Vec<String> {
        archive_names(&paths(list), keep_under.map(Path::new))
            .into_iter()
            .map(|(_, name)| name)
            .collect()
    }

    #[test]
    fn every_member_of_a_shared_name_is_listed_with_its_new_name() {
        let sources = paths(&["/a/invoice.pdf", "/b/other.pdf", "/c/invoice.pdf"]);
        let members: Vec<(String, String)> = members_of_shared_names(&sources)
            .into_iter()
            .map(|(source, name)| {
                (
                    source.to_string_lossy().into_owned(),
                    name.to_string_lossy().into_owned(),
                )
            })
            .collect();
        assert_eq!(
            members,
            [
                ("/a/invoice.pdf".to_owned(), "invoice.pdf".to_owned()),
                ("/c/invoice.pdf".to_owned(), "invoice (2).pdf".to_owned()),
            ]
        );
        assert!(members_of_shared_names(&paths(&["/a/x", "/a/y"])).is_empty());
    }

    #[test]
    fn flat_archive_names_carry_no_folder() {
        let names = archive(&["/s/a/invoice.pdf", "/s/b/invoice.pdf"], None);
        assert_eq!(names, ["invoice.pdf", "invoice (2).pdf"]);
        assert!(names.iter().all(|name| !name.contains('/')));
    }

    #[test]
    fn kept_folders_are_relative_to_the_scope_and_outsiders_fall_back_flat() {
        assert_eq!(
            archive(
                &[
                    "/s/a/invoice.pdf",
                    "/s/invoice.pdf",
                    "/elsewhere/invoice.pdf",
                    "/s/../etc/passwd",
                ],
                Some("/s"),
            ),
            ["a/invoice.pdf", "invoice.pdf", "invoice (2).pdf", "passwd"]
        );
    }

    #[test]
    fn nothing_is_numbered_when_all_sources_share_one_folder() {
        assert_eq!(names(&["/a/x.txt", "/a/X.txt"]), ["x.txt", "X.txt"]);
    }
}
