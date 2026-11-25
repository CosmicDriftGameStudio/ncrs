//! The trees of the targets: the folders that get something, the new files in
//! them and a few of the files that are there already.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ncrs_index::TargetId;

use crate::run::file_name;
use crate::state::Session;

/// How many of the files that are there already a folder lists.
pub const EXISTING_SHOWN: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Folder {
        /// Below the target; empty for the target itself.
        path: PathBuf,
        /// With the folders above it that hold nothing else.
        label: String,
        depth: usize,
        open: bool,
        /// For the target itself all files that go below it.
        new_files: usize,
    },
    NewFile {
        /// Below the source.
        from: PathBuf,
        depth: usize,
        new_name: String,
        /// The name it had, when it is renamed.
        old_name: Option<String>,
    },
    Existing {
        folder: PathBuf,
        depth: usize,
        name: String,
    },
    More {
        folder: PathBuf,
        depth: usize,
        count: usize,
    },
}

struct NewFile<'a> {
    from: &'a Path,
    name: String,
}

/// Every file that goes into a target, by the folder of the target.
fn new_files<'a>(session: &'a Session, target: &TargetId) -> BTreeMap<PathBuf, Vec<NewFile<'a>>> {
    let mut by_folder: BTreeMap<PathBuf, Vec<NewFile<'a>>> = BTreeMap::new();
    for row in &session.rows {
        for (id, relative) in session.places(row).iter() {
            if id != target {
                continue;
            }
            let folder = relative.parent().unwrap_or(Path::new("")).to_path_buf();
            by_folder.entry(folder).or_default().push(NewFile {
                from: &row.relative,
                name: file_name(relative),
            });
        }
    }
    by_folder
}

/// The folders of the targets that get something.
pub fn folders_with_new(session: &Session) -> BTreeSet<(TargetId, PathBuf)> {
    session
        .rows
        .iter()
        .flat_map(|row| session.places(row).into_owned())
        .map(|(id, relative)| {
            let folder = relative.parent().unwrap_or(Path::new("")).to_path_buf();
            (id, folder)
        })
        .collect()
}

/// The target itself, the folders with new files and their parents, and with
/// `only_new` off the other folders of the target too.
fn folders_shown(
    session: &Session,
    target: &TargetId,
    new: &BTreeMap<PathBuf, Vec<NewFile<'_>>>,
) -> BTreeSet<PathBuf> {
    let there = session
        .target_folders
        .iter()
        .filter(|(id, _)| !session.only_new && id == target)
        .map(|(_, folder)| folder);
    new.keys()
        .chain(there)
        .flat_map(|folder| folder.ancestors())
        .chain([Path::new("")])
        .map(Path::to_path_buf)
        .collect()
}

pub fn tree_items(session: &Session, target: &TargetId) -> Vec<Item> {
    let mut new = new_files(session, target);
    let total = new.values().map(Vec::len).sum();
    let closed = |folder: &Path| {
        session
            .closed_folders
            .contains(&(target.clone(), folder.to_path_buf()))
    };
    let there_in = |folder: &Path| {
        session
            .existing
            .get(&(target.clone(), folder.to_path_buf()))
            .map_or(&[][..], Vec::as_slice)
    };
    let shown = folders_shown(session, target, &new);
    let mut children: BTreeMap<&Path, usize> = BTreeMap::new();
    for parent in shown.iter().filter_map(|folder| folder.parent()) {
        *children.entry(parent).or_default() += 1;
    }
    // Folders that only lead to one other folder share its row.
    let joined: BTreeSet<&Path> = shown
        .iter()
        .map(PathBuf::as_path)
        .filter(|folder| {
            folder.parent().is_some()
                && children.get(folder) == Some(&1)
                && !new.contains_key(*folder)
                && there_in(folder).is_empty()
                && !closed(folder)
        })
        .collect();
    let mut items = Vec::new();
    for folder in &shown {
        if joined.contains(folder.as_path()) || folder.ancestors().skip(1).any(closed) {
            continue;
        }
        let mut above = folder
            .ancestors()
            .skip(1)
            .filter(|parent| !joined.contains(parent));
        let label = above
            .next()
            .and_then(|parent| folder.strip_prefix(parent).ok())
            .map_or_else(String::new, |below| below.display().to_string());
        let depth = above.count() + usize::from(folder.parent().is_some());
        let open = !closed(folder);
        let files = new.remove(folder).unwrap_or_default();
        items.push(Item::Folder {
            path: folder.clone(),
            label,
            depth,
            open,
            new_files: if depth == 0 { total } else { files.len() },
        });
        if !open {
            continue;
        }
        for file in files {
            let old = file_name(file.from);
            items.push(Item::NewFile {
                from: file.from.to_path_buf(),
                depth: depth + 1,
                old_name: (old != file.name).then_some(old),
                new_name: file.name,
            });
        }
        let there = there_in(folder);
        for name in there.iter().take(EXISTING_SHOWN) {
            items.push(Item::Existing {
                folder: folder.clone(),
                depth: depth + 1,
                name: name.clone(),
            });
        }
        if there.len() > EXISTING_SHOWN {
            items.push(Item::More {
                folder: folder.clone(),
                depth: depth + 1,
                count: there.len() - EXISTING_SHOWN,
            });
        }
    }
    items
}
